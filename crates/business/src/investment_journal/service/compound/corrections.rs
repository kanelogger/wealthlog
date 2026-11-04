use super::*;
use crate::investment_journal::{
    sources::{current_reference, reference_reconciliation},
    PurposeResourceBalance, UsedSourceReference,
};

#[derive(Debug, Clone, Serialize, Deserialize, schemars::JsonSchema)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct ReverseFundOperation {
    pub operation_id: String,
    pub reason: String,
    pub expected_preview_hash: String,
    pub request_key: String,
}

#[derive(Debug, Clone, Serialize, Deserialize, schemars::JsonSchema)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct CorrectFundOperation {
    pub operation_id: String,
    pub entries: Vec<AllocationDelta>,
    pub reason: String,
    pub expected_preview_hash: String,
    pub request_key: String,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct FundOperationEditPreview {
    pub schema_version: u32,
    pub mode: String,
    pub target: Value,
    pub funding: AllocationPreview,
    pub entries: Vec<AllocationDelta>,
    pub balances_after: Vec<PurposeResourceBalance>,
    pub source_anchors: Vec<UsedSourceReference>,
    pub pending_reconciliation: Vec<FundReconciliationItem>,
    pub preview_hash: String,
}

#[derive(Debug, Clone, Copy)]
enum EditKind {
    Reverse,
    Correct,
}
impl EditKind {
    fn as_str(self) -> &'static str {
        match self {
            Self::Reverse => "REVERSE",
            Self::Correct => "RECONCILE",
        }
    }
}

impl InvestmentJournalService {
    pub async fn preview_fund_reversal(
        &self,
        operation_id: &str,
        caller: &JournalCaller,
    ) -> JournalResult<FundOperationEditPreview> {
        self.preview_operation_edit(operation_id, vec![], EditKind::Reverse, caller)
            .await
    }
    pub async fn preview_fund_correction(
        &self,
        operation_id: &str,
        entries: Vec<AllocationDelta>,
        caller: &JournalCaller,
    ) -> JournalResult<FundOperationEditPreview> {
        self.preview_operation_edit(operation_id, entries, EditKind::Correct, caller)
            .await
    }
    async fn preview_operation_edit(
        &self,
        id: &str,
        entries: Vec<AllocationDelta>,
        kind: EditKind,
        caller: &JournalCaller,
    ) -> JournalResult<FundOperationEditPreview> {
        caller.require_read()?;
        let resources = {
            let mut conn = get_connection(self.store.pool())?;
            let target = load_operation(&mut conn, id)?;
            operation_resources(&target, &entries)?
        };
        let (day, _) = self.business_day()?;
        let funding = self.preview(resources, day).await?;
        let observations: Vec<_> = funding
            .resources
            .iter()
            .map(|r| r.observation.clone())
            .collect();
        let mut conn = get_connection(self.store.pool())?;
        conn.transaction::<_, JournalError, _>(|conn| {
            self.resources.validate(conn, &observations)?;
            build_edit_preview(conn, id, entries, kind, funding)
        })
    }
    pub async fn reverse_fund_operation(
        &self,
        input: ReverseFundOperation,
        caller: &JournalCaller,
    ) -> JournalResult<Value> {
        self.execute_operation_edit(
            &input.operation_id,
            vec![],
            &input.reason,
            &input.expected_preview_hash,
            &input.request_key,
            EditKind::Reverse,
            &input,
            caller,
        )
        .await
    }
    pub async fn correct_fund_operation(
        &self,
        input: CorrectFundOperation,
        caller: &JournalCaller,
    ) -> JournalResult<Value> {
        self.execute_operation_edit(
            &input.operation_id,
            input.entries.clone(),
            &input.reason,
            &input.expected_preview_hash,
            &input.request_key,
            EditKind::Correct,
            &input,
            caller,
        )
        .await
    }
    #[allow(clippy::too_many_arguments)]
    async fn execute_operation_edit<T: Serialize>(
        &self,
        id: &str,
        entries: Vec<AllocationDelta>,
        reason: &str,
        expected: &str,
        key: &str,
        kind: EditKind,
        input: &T,
        caller: &JournalCaller,
    ) -> JournalResult<Value> {
        caller.require_write()?;
        nonblank(reason, "reason")?;
        let operation = match kind {
            EditKind::Reverse => "investment_fund_reverse",
            EditKind::Correct => "investment_fund_correct",
        };
        let record = self.request(operation, key, input, caller)?;
        {
            let mut conn = get_connection(self.store.pool())?;
            if let Some(result) = replay(&mut conn, &record)? {
                return Ok(result);
            }
        }
        let preview = self
            .preview_operation_edit(id, entries.clone(), kind, caller)
            .await?;
        if preview.preview_hash != expected {
            return Err(JournalError::StateConflict);
        }
        let reader = self.resources.clone();
        let caller = caller.clone();
        let ledger = self.ledger_id.clone();
        let id = id.to_string();
        let expected = expected.to_string();
        let reason = reason.to_string();
        let (day, zone) = self.business_day()?;
        transact(&self.store,move |conn| {
            if let Some(result)=replay(conn,&record)? { return Ok(result); }
            let observations:Vec<_>=preview.funding.resources.iter().map(|r| r.observation.clone()).collect();
            reader.validate(conn,&observations)?;
            let funding=build_preview(conn,observations,day)?;
            let current=build_edit_preview(conn,&id,entries,kind,funding)?;
            if current.preview_hash!=expected { return Err(JournalError::StateConflict); }
            let operation_id=uuid::Uuid::new_v4().to_string(); let request_id=uuid::Uuid::new_v4().to_string();
            let mut result=json!({"operationId":operation_id,"correctionOf":id,"kind":kind.as_str(),"factsChanged":false});
            let request=record_completed_request(conn,&record,&ledger,&request_id,&mut result)?;
            let balances=allocation_rows(conn)?; let baselines=confirmed_observations(conn)?;
            let before:Vec<_>=current.funding.resources.iter().map(|r| r.observation.clone()).collect();
            let mut after=before.clone();
            let version=reader.committed_state_version(conn,day)?;
            for observation in &mut after { observation.state_version=version.clone(); }
            let mut acknowledgements=vec![];
            let mut source_kind=None; let mut source_id=None; let mut counterparty=None; let mut source_version=None;
            match kind {
                EditKind::Reverse=>carry_unresolved_sources(&baselines,&before,&balances,&mut after)?,
                EditKind::Correct=>{
                    for observation in &mut after { observation.unresolved_sources.clear(); }
                    acknowledgements=current.funding.source_references.clone();
                    if let Some(anchor)=current.source_anchors.first() {
                        source_kind=Some(anchor.source_kind.clone()); source_id=Some(anchor.source_id.clone()); counterparty=anchor.source_counterparty_id.clone(); source_version=anchor.current_version.clone();
                    }
                    for mut anchor in current.source_anchors.clone() { anchor.operation_id=operation_id.clone(); anchor.origin_version=anchor.current_version.clone(); acknowledgements.push(anchor); }
                }
            }
            let confirmation=json!({"schemaVersion":1,"sourceReferences":acknowledgements,"resources":after.iter().map(|o| json!({"resource":o.resource,"sources":o.sources})).collect::<Vec<_>>()});
            let operation=json!({"id":operation_id,"kind":kind.as_str(),"requestId":request_id,"reason":reason,"sourceKind":source_kind,"sourceId":source_id,"sourceCounterpartyId":counterparty,"sourceVersion":source_version,"firstApplicationKey":null,"correctionOf":id,
                "observation":{"schemaVersion":1,"resources":after,"before":before},"confirmation":confirmation,"detail":{"schemaVersion":1,"target":current.target,"entries":current.entries,"factsChanged":false}});
            insert_compound_operation(conn,&operation,&caller)?;
            save_allocation_entries(conn,&operation_id,&balances,current.entries.iter())?;
            audit_completed_request(conn,&record,&caller,&ledger,&operation_id,&request_id,&request.created_at,day,&zone)?;
            Ok(result)
        }).await
    }
}

#[derive(Debug, Clone, Serialize, QueryableByName)]
struct Operation {
    #[diesel(sql_type=Text)]
    id: String,
    #[diesel(sql_type=Text)]
    operation_kind: String,
    #[diesel(sql_type=Nullable<Text>)]
    source_kind: Option<String>,
    #[diesel(sql_type=Nullable<Text>)]
    source_id: Option<String>,
    #[diesel(sql_type=Nullable<Text>)]
    source_counterparty_id: Option<String>,
    #[diesel(sql_type=Nullable<Text>)]
    source_version: Option<String>,
    #[diesel(sql_type=Nullable<Text>)]
    correction_of: Option<String>,
    #[diesel(sql_type=Text)]
    observation_json: String,
    #[diesel(sql_type=Text)]
    detail_json: String,
}
fn load_operation(conn: &mut SqliteConnection, id: &str) -> JournalResult<Operation> {
    Ok(diesel::sql_query("SELECT id,operation_kind,source_kind,source_id,source_counterparty_id,source_version,correction_of,observation_json,detail_json FROM fund_allocation_operations WHERE id=?1").bind::<Text,_>(id).get_result::<Operation>(conn)?)
}
fn operation_resources(
    target: &Operation,
    entries: &[AllocationDelta],
) -> JournalResult<Vec<FundResource>> {
    let body: Value = serde_json::from_str(&target.observation_json).map_err(json_error)?;
    let mut resources: BTreeSet<_> = entries.iter().map(|e| e.resource.clone()).collect();
    for row in body["resources"]
        .as_array()
        .ok_or_else(|| invalid("operation has no resource observations"))?
    {
        resources.insert(
            serde_json::from_value::<FundResource>(row["resource"].clone()).map_err(json_error)?,
        );
    }
    Ok(resources.into_iter().collect())
}
fn build_edit_preview(
    conn: &mut SqliteConnection,
    id: &str,
    mut entries: Vec<AllocationDelta>,
    kind: EditKind,
    funding: AllocationPreview,
) -> JournalResult<FundOperationEditPreview> {
    let target = load_operation(conn, id)?;
    if matches!(kind, EditKind::Reverse) {
        if target.operation_kind == "REVERSE" {
            return Err(invalid(
                "use explicit correction to adjust an ownership reversal",
            ));
        }
        #[derive(QueryableByName)]
        struct Count {
            #[diesel(sql_type=diesel::sql_types::BigInt)]
            count: i64,
        }
        if diesel::sql_query("SELECT COUNT(*) AS count FROM fund_allocation_operations WHERE operation_kind='REVERSE' AND correction_of=?1").bind::<Text,_>(id).get_result::<Count>(conn)?.count!=0 { return Err(JournalError::AlreadyApplied); }
        entries = reversal_entries(conn, id)?;
        if entries.is_empty() {
            return Err(invalid("there are no ownership entries to reverse; source acknowledgements remain historical facts"));
        }
    }
    let balances = allocation_rows(conn)?;
    let actual = funding
        .resources
        .iter()
        .map(|r| (r.observation.resource.clone(), r.observation.actual))
        .collect();
    let final_balances = apply_deltas(&balances, &entries, &actual)?;
    let purposes = purpose_rows(conn)?;
    for entry in &entries {
        let purpose = purposes
            .iter()
            .find(|p| p["id"].as_str() == Some(&entry.purpose_id))
            .ok_or_else(|| invalid("purpose not found"))?;
        if purpose["isArchived"] == true && entry.delta > ExactDecimal::ZERO {
            return Err(invalid("cannot add ownership to an archived purpose"));
        }
    }
    let source_anchors = source_anchors(conn, &target, &funding)?;
    let baselines = confirmed_observations(conn)?;
    let mut pending = reference_reconciliation(conn, &funding.source_references)?;
    for row in &funding.resources {
        if let Some(b) = baselines.get(&row.observation.resource) {
            pending.extend(compare_fund_observation(
                b,
                &row.observation,
                row.balance.allocated,
            )?);
        }
    }
    let changed: BTreeSet<_> = entries
        .iter()
        .map(|e| (e.purpose_id.clone(), e.resource.clone()))
        .collect();
    let after: Vec<_> = final_balances
        .into_iter()
        .filter(|(k, _)| changed.contains(k))
        .map(|((purpose_id, resource), amount)| PurposeResourceBalance {
            purpose_id,
            resource,
            amount,
        })
        .collect();
    let target = serde_json::to_value(target).map_err(json_error)?;
    let preview_hash = crate::requests::hash_json(&crate::requests::canonicalize(
        json!({"schemaVersion":1,"mode":kind.as_str(),"target":target,"entries":entries,"funding":funding.preview_hash,"sourceAnchors":source_anchors,"pending":pending}),
    ));
    Ok(FundOperationEditPreview {
        schema_version: 1,
        mode: kind.as_str().into(),
        target,
        funding,
        entries,
        balances_after: after,
        source_anchors,
        pending_reconciliation: pending,
        preview_hash,
    })
}
fn reversal_entries(conn: &mut SqliteConnection, id: &str) -> JournalResult<Vec<AllocationDelta>> {
    #[derive(QueryableByName)]
    struct Row {
        #[diesel(sql_type=Text)]
        body: String,
    }
    let rows=diesel::sql_query("SELECT json_object('purposeId',purpose_id,'resource',json(CASE WHEN resource_kind='CASH' THEN json_object('kind','CASH','accountId',account_id,'currency',currency) ELSE json_object('kind','SECURITY','accountId',account_id,'assetId',asset_id) END),'delta',COALESCE(cash_delta,quantity_delta)) AS body FROM fund_allocation_entries WHERE operation_id=?1 ORDER BY rowid").bind::<Text,_>(id).load::<Row>(conn)?;
    let mut totals: BTreeMap<(String, FundResource), ExactDecimal> = BTreeMap::new();
    for row in rows {
        let entry: AllocationDelta = serde_json::from_str(&row.body).map_err(json_error)?;
        let key = (entry.purpose_id, entry.resource);
        let old = totals.get(&key).copied().unwrap_or_default();
        totals.insert(key, old.checked_add(entry.delta)?);
    }
    Ok(totals
        .into_iter()
        .filter(|(_, v)| *v != ExactDecimal::ZERO)
        .map(|((purpose_id, resource), v)| AllocationDelta {
            purpose_id,
            resource,
            delta: ExactDecimal::new(-v.value()),
        })
        .collect())
}
fn source_anchors(
    conn: &mut SqliteConnection,
    target: &Operation,
    funding: &AllocationPreview,
) -> JournalResult<Vec<UsedSourceReference>> {
    let mut anchor = target.clone();
    let mut seen = BTreeSet::new();
    loop {
        if !seen.insert(anchor.id.clone()) {
            return Err(invalid("allocation correction ancestry contains a cycle"));
        }
        if let (Some(kind), Some(id)) = (&anchor.source_kind, &anchor.source_id) {
            let (current_version, state) =
                current_reference(conn, kind, id, anchor.source_counterparty_id.as_deref())?;
            return Ok(funding
                .resources
                .iter()
                .map(|r| UsedSourceReference {
                    operation_id: anchor.id.clone(),
                    resource: r.observation.resource.clone(),
                    source_kind: kind.clone(),
                    source_id: id.clone(),
                    source_counterparty_id: anchor.source_counterparty_id.clone(),
                    origin_version: anchor.source_version.clone(),
                    current_version: current_version.clone(),
                    state,
                })
                .collect());
        }
        let Some(parent) = &anchor.correction_of else {
            return Ok(vec![]);
        };
        anchor = load_operation(conn, parent)?;
    }
}

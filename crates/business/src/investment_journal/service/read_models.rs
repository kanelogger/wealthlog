//! Module status and immutable ownership history; onboarding is a separate preference.
use super::*;

#[derive(Debug, Clone, Default, Serialize, Deserialize, schemars::JsonSchema)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct FundHistoryQuery {
    pub purpose_id: Option<String>,
    pub operation_id: Option<String>,
    pub limit: Option<u32>,
    pub offset: Option<u32>,
}
#[derive(Debug, Clone, Serialize, Deserialize, schemars::JsonSchema)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct CompleteInvestmentOnboarding {
    pub request_key: String,
}

impl InvestmentJournalService {
    pub async fn investment_journal_account_state(
        &self,
        account_id: &str,
        caller: &JournalCaller,
    ) -> JournalResult<Value> {
        caller.require_read()?;
        nonblank(account_id, "accountId")?;
        self.bookkeeping
            .as_ref()
            .ok_or(JournalError::ResourceUnavailable)?
            .get_state(account_id)
            .await
            .map_err(Into::into)
    }
    pub(crate) fn require_ready(&self) -> JournalResult<()> {
        let mut conn = get_connection(self.store.pool())?;
        if !schema_ready(&mut conn)? {
            return Err(JournalError::ResourceUnavailable);
        }
        Ok(())
    }
    pub fn investment_journal_status(&self, caller: &JournalCaller) -> JournalResult<Value> {
        caller.require_read()?;
        let (day, zone) = self.business_day()?;
        let mut conn = get_connection(self.store.pool())?;
        conn.transaction::<_,JournalError,_>(|conn| {
            #[derive(QueryableByName)]struct Setting {#[diesel(sql_type=Text)]setting_key:String,#[diesel(sql_type=Text)]setting_value:String}
            let settings=diesel::sql_query("SELECT setting_key,setting_value FROM app_settings WHERE setting_key LIKE 'investments.%' OR setting_key LIKE 'investment_projection.backfill.%' OR setting_key='investment_projection.background_error'").load::<Setting>(conn)?.into_iter().map(|row|(row.setting_key,row.setting_value)).collect::<BTreeMap<_,_>>();
            let complete=["ACTIVITY","MANUAL_TRADE","SNAPSHOT"].map(|kind|(kind,settings.get(&format!("investment_projection.backfill.{kind}.complete")).is_some_and(|v|v=="true"))).into_iter().collect::<BTreeMap<_,_>>();
            #[derive(QueryableByName)]struct Count {#[diesel(sql_type=diesel::sql_types::BigInt)]count:i64}
            let pending=diesel::sql_query("SELECT COUNT(*) AS count FROM investment_projection_state WHERE processed_generation<generation").get_result::<Count>(conn)?.count;
            Ok(json!({"schemaVersion":1,"ledgerId":self.ledger_id,"asOfDate":day,"timezone":zone,"ready":schema_ready(conn)?,"enabledAt":settings.get("investments.enabled_at"),"enabledOn":settings.get("investments.enabled_on"),"enabledTimezone":settings.get("investments.enabled_timezone"),"onboardingCompletedAt":settings.get("investments.onboarding_completed_at"),"projection":{"pendingSources":pending,"backfillComplete":complete,"error":settings.get("investment_projection.background_error")}}))
        })
    }
    /// Saving/skipping the UI introduction must not set the business enable date.
    pub async fn complete_investment_onboarding(
        &self,
        input: CompleteInvestmentOnboarding,
        caller: &JournalCaller,
    ) -> JournalResult<Value> {
        caller.require_write()?;
        if !matches!(caller.actor_kind, JournalActorKind::GuiSession)
            || caller.channel != BusinessChannel::Gui
        {
            return Err(JournalError::PermissionDenied);
        }
        let record = self.request(
            "investment_onboarding_complete",
            &input.request_key,
            &input,
            caller,
        )?;
        let caller = caller.clone();
        let ledger = self.ledger_id.clone();
        transact(&self.store,move|conn|{
            if let Some(result)=replay(conn,&record)? {return Ok(result);}
            diesel::sql_query("INSERT INTO app_settings(setting_key,setting_value) VALUES('investments.onboarding_completed_at',?1) ON CONFLICT(setting_key) DO NOTHING").bind::<Text,_>(now()).execute(conn)?;
            let at=diesel::sql_query("SELECT setting_value AS body FROM app_settings WHERE setting_key='investments.onboarding_completed_at'").get_result::<JsonRow>(conn)?.body;
            let id=uuid::Uuid::new_v4().to_string();let mut result=json!({"onboardingCompletedAt":at});
            record_completed_request(conn,&record,&ledger,&id,&mut result)?;
            insert_completed_audit(conn,&record,&caller,&ledger,"onboarding",&id)?;
            Ok(result)
        }).await
    }
    pub fn fund_history(
        &self,
        query: FundHistoryQuery,
        caller: &JournalCaller,
    ) -> JournalResult<Vec<Value>> {
        caller.require_read()?;
        if let Some(id) = &query.purpose_id {
            nonblank(id, "purposeId")?;
        }
        if let Some(id) = &query.operation_id {
            nonblank(id, "operationId")?;
        }
        let limit = query.limit.unwrap_or(100);
        if !(1..=500).contains(&limit) {
            return Err(invalid("limit must be between 1 and 500"));
        }
        let mut conn = get_connection(self.store.pool())?;
        let rows = diesel::sql_query(HISTORY)
            .bind::<Nullable<Text>, _>(query.purpose_id)
            .bind::<Nullable<Text>, _>(query.operation_id)
            .bind::<Integer, _>(limit as i32)
            .bind::<diesel::sql_types::BigInt, _>(i64::from(query.offset.unwrap_or(0)))
            .load::<JsonRow>(&mut conn)?;
        rows.into_iter()
            .map(|row| serde_json::from_str(&row.body).map_err(json_error))
            .collect()
    }
}
fn schema_ready(conn: &mut SqliteConnection) -> JournalResult<bool> {
    #[derive(QueryableByName)]
    struct Count {
        #[diesel(sql_type=diesel::sql_types::BigInt)]
        count: i64,
    }
    Ok(diesel::sql_query("SELECT COUNT(*) AS count FROM sqlite_master WHERE type='table' AND name IN ('fund_purposes','fund_allocation_operations','fund_allocation_entries','fund_allocations','investment_executions','investment_projection_state','investment_decisions','investment_decision_revisions','investment_decision_options','investment_decision_evidence','investment_decision_funding','investment_execution_links','investment_reviews','investment_principles','investment_principle_revisions','investment_decision_principles','investment_review_principles')").get_result::<Count>(conn)?.count==17)
}
const HISTORY: &str = r#"SELECT json_object('id',o.id,'operationKind',o.operation_kind,'requestId',o.request_id,'reason',o.reason,'sourceKind',o.source_kind,'sourceId',o.source_id,'sourceCounterpartyId',o.source_counterparty_id,'sourceVersion',o.source_version,'correctionOf',o.correction_of,'observations',json(o.observation_json),'sourceConfirmation',json(o.source_confirmation_json),'detail',json(o.detail_json),'createdAt',o.created_at,'createdChannel',o.created_channel,'createdActorKind',o.created_actor_kind,'createdActorId',o.created_actor_id,'createdCredentialId',o.created_credential_id,'entries',json((SELECT json_group_array(json_object('id',e.id,'purposeId',e.purpose_id,'resource',json(CASE WHEN e.resource_kind='CASH' THEN json_object('kind','CASH','accountId',e.account_id,'currency',e.currency) ELSE json_object('kind','SECURITY','accountId',e.account_id,'assetId',e.asset_id) END),'delta',COALESCE(e.cash_delta,e.quantity_delta))) FROM (SELECT * FROM fund_allocation_entries WHERE operation_id=o.id ORDER BY rowid) e))) AS body FROM fund_allocation_operations o WHERE (?1 IS NULL OR EXISTS(SELECT 1 FROM fund_allocation_entries e WHERE e.operation_id=o.id AND e.purpose_id=?1)) AND (?2 IS NULL OR o.id=?2) ORDER BY o.rowid DESC LIMIT ?3 OFFSET ?4"#;

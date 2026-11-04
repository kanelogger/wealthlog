use std::collections::{BTreeMap, BTreeSet};
use std::sync::{Arc, RwLock};

use chrono::NaiveDate;
use diesel::prelude::*;
use diesel::sql_types::{Integer, Nullable, Text};
use serde::{Deserialize, Serialize};
use serde_json::{json, Value};
use wealthfolio_core::utils::time_utils::{parse_user_timezone, user_today};
use wealthfolio_storage_sqlite::db::get_connection;
use wealthfolio_storage_sqlite::schema::{business_audit, business_requests};

use crate::store::{NewRequestRow, RequestRow};
use crate::{BusinessAuditEntry, BusinessChannel, BusinessStore, RequestRecord, RequestReplay};

use super::model::{apply_deltas, invalid, nonblank, validate_currency};
use super::{
    AllocationDelta, ExactDecimal, FundResource, JournalError, JournalResult, ResourceBalance,
    ResourceObservation, UsedSourceReference,
};

mod cases;
mod compound;
pub use cases::*;
pub use compound::*;
mod decisions;
pub use decisions::*;
mod execution_reconciliation;
pub use execution_reconciliation::*;
mod principle_references;
mod principles;
pub use principles::*;
mod queue;
mod read_models;
mod reviews;
pub use queue::*;
pub use read_models::*;
pub(crate) use reviews::review_observations;
pub use reviews::ReviewSubmissionPreview;
mod links;
pub use links::*;

/// Constructed by a trusted desktop/CLI/MCP host, never deserialized from args.
/// Permissions already include dependencies on account, holdings and activity reads.
#[derive(Debug, Clone)]
pub struct JournalCaller {
    pub channel: BusinessChannel,
    pub actor_id: String,
    pub credential_id: Option<String>,
    pub actor_kind: JournalActorKind,
    pub authenticated: bool,
    pub can_read: bool,
    pub can_write: bool,
    pub can_confirm: bool,
    pub fact_permissions: JournalFactPermissions,
}

#[derive(Debug, Clone, Copy, Default)]
pub struct JournalFactPermissions {
    pub holdings_write: bool,
    pub transfers_write: bool,
    pub activities_write: bool,
}

#[derive(Debug, Clone, Copy)]
pub enum JournalActorKind {
    GuiSession,
    Program,
}

impl JournalActorKind {
    fn as_str(self) -> &'static str {
        match self {
            Self::GuiSession => "GUI_SESSION",
            Self::Program => "PROGRAM",
        }
    }
}

impl JournalCaller {
    pub fn require_read(&self) -> JournalResult<()> {
        if !self.authenticated || !self.can_read || matches!(self.channel, BusinessChannel::Rest) {
            Err(JournalError::PermissionDenied)
        } else {
            Ok(())
        }
    }
    pub fn require_write(&self) -> JournalResult<()> {
        self.require_read()?;
        if !self.can_write {
            Err(JournalError::PermissionDenied)
        } else {
            Ok(())
        }
    }
    pub fn require_confirm(&self) -> JournalResult<()> {
        self.require_write()?;
        if !self.can_confirm {
            Err(JournalError::PermissionDenied)
        } else {
            Ok(())
        }
    }
}

/// Reads only authoritative accounting resources. Implementations must compare
/// source state in `validate` inside the writer transaction. UI values, quotes
/// and purpose entries must never be used to reconstruct actual balances.
#[async_trait::async_trait]
pub trait InvestmentResourceReader: Send + Sync {
    async fn observe(
        &self,
        resources: &[FundResource],
        day: NaiveDate,
    ) -> JournalResult<Vec<ResourceObservation>>;
    fn validate(
        &self,
        conn: &mut SqliteConnection,
        observations: &[ResourceObservation],
    ) -> JournalResult<()>;

    /// Version of committed facts, read on the same writer connection. Used
    /// only after committing an exactly prepared compound fact mutation.
    fn committed_state_version(
        &self,
        _conn: &mut SqliteConnection,
        _day: NaiveDate,
    ) -> JournalResult<String> {
        Err(invalid(
            "accounting reader does not support compound writes",
        ))
    }

    async fn prepare_ledger_facts(
        &self,
        _proposed: &[wealthfolio_storage_sqlite::activities::ActivityDB],
        _day: NaiveDate,
    ) -> JournalResult<PreparedLedgerFacts> {
        Err(invalid(
            "accounting reader does not support ledger previews",
        ))
    }
    fn validate_ledger_facts(
        &self,
        _conn: &mut SqliteConnection,
        _prepared: &PreparedLedgerFacts,
    ) -> JournalResult<()> {
        Err(invalid(
            "accounting reader does not support ledger previews",
        ))
    }
}

#[derive(Debug, Clone)]
pub struct PreparedLedgerFacts {
    pub raw: wealthfolio_portfolio_engine::model::RawFacts,
    pub state_version: String,
    pub as_of_date: NaiveDate,
}

#[derive(Debug, Clone, Serialize, Deserialize, schemars::JsonSchema)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct SaveFundPurpose {
    pub id: Option<String>,
    pub expected_version: Option<i32>,
    pub name: String,
    pub category: String,
    pub currency: String,
    pub target_amount: Option<ExactDecimal>,
    pub use_on: Option<NaiveDate>,
    pub liquidity: Option<String>,
    pub minimum_cash: Option<ExactDecimal>,
    pub loss_budget: Option<ExactDecimal>,
    pub single_asset_limit_bps: Option<i32>,
    #[serde(default)]
    pub is_archived: bool,
    pub request_key: String,
}

#[derive(Debug, Clone, Copy, Serialize, Deserialize, schemars::JsonSchema)]
#[serde(rename_all = "SCREAMING_SNAKE_CASE")]
pub enum AllocationOperationKind {
    Allocate,
    Reallocate,
    Reconcile,
}

impl AllocationOperationKind {
    fn as_str(self) -> &'static str {
        match self {
            Self::Allocate => "ALLOCATE",
            Self::Reallocate => "REALLOCATE",
            Self::Reconcile => "RECONCILE",
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize, schemars::JsonSchema)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct ApplyFundAllocation {
    pub operation_kind: AllocationOperationKind,
    pub reason: String,
    pub entries: Vec<AllocationDelta>,
    /// Zero-entry reconciliation must still identify what is being confirmed.
    pub resources: Vec<FundResource>,
    pub expected_preview_hash: String,
    pub request_key: String,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct FundResourceStatus {
    pub observation: ResourceObservation,
    pub balance: ResourceBalance,
    pub purposes: BTreeMap<String, ExactDecimal>,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct AllocationPreview {
    pub schema_version: u32,
    pub as_of_date: NaiveDate,
    pub resources: Vec<FundResourceStatus>,
    pub source_references: Vec<UsedSourceReference>,
    pub preview_hash: String,
}

#[derive(Clone)]
pub struct InvestmentJournalService {
    pub(crate) store: BusinessStore,
    pub(crate) ledger_id: String,
    resources: Arc<dyn InvestmentResourceReader>,
    timezone: Arc<RwLock<String>>,
    bookkeeping: Option<Arc<crate::investments::InvestmentService>>,
    context_reader: Option<Arc<dyn super::InvestmentContextReader>>,
    review_facts_reader: Option<Arc<dyn super::ReviewFactsReader>>,
}

impl InvestmentJournalService {
    pub fn new(
        store: BusinessStore,
        ledger_id: String,
        resources: Arc<dyn InvestmentResourceReader>,
        timezone: Arc<RwLock<String>>,
    ) -> Self {
        Self {
            store,
            ledger_id,
            resources,
            timezone,
            bookkeeping: None,
            context_reader: None,
            review_facts_reader: None,
        }
    }

    pub fn with_bookkeeping(mut self, service: Arc<crate::investments::InvestmentService>) -> Self {
        self.bookkeeping = Some(service);
        self
    }

    pub fn with_context_reader(mut self, reader: Arc<dyn super::InvestmentContextReader>) -> Self {
        self.context_reader = Some(reader);
        self
    }

    pub fn with_review_facts_reader(mut self, reader: Arc<dyn super::ReviewFactsReader>) -> Self {
        self.review_facts_reader = Some(reader);
        self
    }

    fn business_day(&self) -> JournalResult<(NaiveDate, String)> {
        let zone = self
            .timezone
            .read()
            .unwrap_or_else(|e| e.into_inner())
            .clone();
        let tz = parse_user_timezone(&zone)?;
        Ok((user_today(tz), tz.name().to_string()))
    }

    pub fn list_purposes(&self, caller: &JournalCaller) -> JournalResult<Vec<Value>> {
        caller.require_read()?;
        let mut conn = get_connection(self.store.pool())?;
        purpose_rows(&mut conn)
    }

    pub async fn save_purpose(
        &self,
        input: SaveFundPurpose,
        caller: &JournalCaller,
    ) -> JournalResult<Value> {
        caller.require_write()?;
        nonblank(&input.name, "name")?;
        nonblank(&input.category, "category")?;
        validate_currency(&input.currency)?;
        for amount in [input.target_amount, input.minimum_cash, input.loss_budget]
            .into_iter()
            .flatten()
        {
            amount.nonnegative()?;
        }
        if input
            .single_asset_limit_bps
            .is_some_and(|v| !(0..=10000).contains(&v))
        {
            return Err(invalid("singleAssetLimitBps must be between 0 and 10000"));
        }
        if input.id.is_some() != input.expected_version.is_some()
            || input.expected_version.is_some_and(|v| v <= 0)
        {
            return Err(invalid(
                "editing a purpose requires its id and expectedVersion",
            ));
        }
        let next_version = input
            .expected_version
            .unwrap_or_default()
            .checked_add(1)
            .ok_or_else(|| invalid("purpose version is outside the supported range"))?;
        let record = self.request(
            "investment_purpose_save",
            &input.request_key,
            &input,
            caller,
        )?;
        let (day, timezone) = self.business_day()?;
        let caller = caller.clone();
        let ledger = self.ledger_id.clone();
        transact(&self.store, move |conn| {
            if let Some(replay) = replay(conn, &record)? {
                return Ok(replay);
            }
            let id = input
                .id
                .clone()
                .unwrap_or_else(|| uuid::Uuid::new_v4().to_string());
            let now = now();
            let data = serde_json::to_string(&input).map_err(json_error)?;
            let changed = if input.id.is_some() {
                diesel::sql_query(UPDATE_PURPOSE)
                    .bind::<Text, _>(&data)
                    .bind::<Text, _>(&now)
                    .bind::<Text, _>(&id)
                    .bind::<Integer, _>(input.expected_version.unwrap_or_default())
                    .execute(conn)?
            } else {
                diesel::sql_query(INSERT_PURPOSE)
                    .bind::<Text, _>(&id)
                    .bind::<Text, _>(&data)
                    .bind::<Text, _>(&now)
                    .execute(conn)?
            };
            if changed != 1 {
                return Err(JournalError::StateConflict);
            }
            let result = json!({"purposeId":id,"version":next_version});
            complete(conn, &record, &caller, &ledger, &id, result, day, &timezone)
        })
        .await
    }

    pub async fn preview_allocations(
        &self,
        resources: Vec<FundResource>,
        caller: &JournalCaller,
    ) -> JournalResult<AllocationPreview> {
        caller.require_read()?;
        let (day, _) = self.business_day()?;
        self.preview(resources, day).await
    }

    async fn preview(
        &self,
        resources: Vec<FundResource>,
        day: NaiveDate,
    ) -> JournalResult<AllocationPreview> {
        let resources = unique_resources(resources)?;
        let observations = self.resources.observe(&resources, day).await?;
        verify_observations(&resources, &observations, day)?;
        let mut conn = get_connection(self.store.pool())?;
        conn.transaction::<_, JournalError, _>(|conn| {
            self.resources.validate(conn, &observations)?;
            build_preview(conn, observations, day)
        })
    }

    pub async fn apply_allocation(
        &self,
        input: ApplyFundAllocation,
        caller: &JournalCaller,
    ) -> JournalResult<Value> {
        self.apply_allocation_internal(input, None, None, caller)
            .await
    }

    async fn apply_allocation_internal(
        &self,
        input: ApplyFundAllocation,
        request_override: Option<RequestRecord>,
        execution_ack: Option<execution_reconciliation::PreparedExecutionAck>,
        caller: &JournalCaller,
    ) -> JournalResult<Value> {
        caller.require_write()?;
        nonblank(&input.reason, "reason")?;
        if input.entries.is_empty()
            && !matches!(input.operation_kind, AllocationOperationKind::Reconcile)
        {
            return Err(invalid("only reconciliation permits zero entries"));
        }
        let record = match request_override {
            Some(record) => record,
            None => self.request(
                "investment_allocation_apply",
                &input.request_key,
                &input,
                caller,
            )?,
        };
        // Replay before observing today's state: a committed retry must work
        // even when the source has since changed or disappeared.
        {
            let mut conn = get_connection(self.store.pool())?;
            if let Some(result) = replay(&mut conn, &record)? {
                return Ok(result);
            }
        }
        let mut resources = input.resources.clone();
        resources.extend(input.entries.iter().map(|r| r.resource.clone()));
        let (day, timezone) = self.business_day()?;
        let preview = if resources.is_empty() && execution_ack.is_some() {
            let mut conn = get_connection(self.store.pool())?;
            build_preview(&mut conn, vec![], day)?
        } else {
            self.preview(resources, day).await?
        };
        if preview.preview_hash != input.expected_preview_hash {
            return Err(JournalError::StateConflict);
        }
        let observations: Vec<_> = preview
            .resources
            .iter()
            .map(|r| r.observation.clone())
            .collect();
        let reader = self.resources.clone();
        let ledger = self.ledger_id.clone();
        let caller = caller.clone();
        transact(&self.store, move |conn| {
            if let Some(result) = replay(conn, &record)? { return Ok(result); }
            if !observations.is_empty(){reader.validate(conn, &observations)?;}
            let current = build_preview(conn, observations.clone(), day)?;
            let acknowledgement=execution_ack.as_ref().map(|ack|execution_reconciliation::validate_ack(conn,ack,&current,&input.entries)).transpose()?;
            let detail=json!({"schemaVersion":1,"executionAcknowledgement":acknowledgement});
            if current.preview_hash != input.expected_preview_hash { return Err(JournalError::StateConflict); }
            let balances = allocation_rows(conn)?;
            let actual = observations.iter().map(|o| (o.resource.clone(),o.actual)).collect();
            let after = apply_deltas(&balances, &input.entries, &actual)?;
            if matches!(input.operation_kind, AllocationOperationKind::Reallocate) {
                let mut totals: BTreeMap<FundResource, ExactDecimal> = BTreeMap::new();
                for line in &input.entries {
                    let old = totals.get(&line.resource).copied().unwrap_or_default();
                    totals.insert(line.resource.clone(),old.checked_add(line.delta)?);
                }
                if totals.values().any(|v| *v != ExactDecimal::ZERO) { return Err(invalid("reallocation must conserve each resource")); }
            }
            let purposes = purpose_rows(conn)?;
            for line in &input.entries {
                let purpose = purposes.iter().find(|p| p["id"].as_str()==Some(&line.purpose_id)).ok_or_else(|| invalid("purpose not found"))?;
                if purpose["isArchived"] == true && line.delta > ExactDecimal::ZERO { return Err(invalid("cannot add ownership to an archived purpose")); }
            }
            let operation_id = uuid::Uuid::new_v4().to_string();
            let request_id = uuid::Uuid::new_v4().to_string();
            let observation = json!({"schemaVersion":1,"resources":observations,"balancesBefore":current.resources});
            let prior_baselines=confirmed_observations(conn)?;
            let mut initial_resources=BTreeSet::new();
            for o in &observations {
                if !prior_baselines.contains_key(&o.resource)||!resource_has_entries(conn,&o.resource)? {initial_resources.insert(o.resource.clone());}
            }
            let references=if matches!(input.operation_kind,AllocationOperationKind::Reconcile) { current.source_references.clone() } else { vec![] };
            let confirmations = json!({"schemaVersion":1,"sourceReferences":references,"resources":observations.iter()
                .filter(|o| matches!(input.operation_kind,AllocationOperationKind::Reconcile) || initial_resources.contains(&o.resource))
                .map(|o| json!({"resource":o.resource,"sources":o.sources})).collect::<Vec<_>>()});
            diesel::sql_query(INSERT_ALLOCATION_OPERATION)
                .bind::<Text,_>(&operation_id).bind::<Text,_>(input.operation_kind.as_str()).bind::<Text,_>(&request_id).bind::<Text,_>(&input.reason)
                .bind::<Text,_>(observation.to_string()).bind::<Text,_>(confirmations.to_string()).bind::<Text,_>(now()).bind::<Text,_>(caller.channel.as_str())
                .bind::<Text,_>(caller.actor_kind.as_str()).bind::<Text,_>(&caller.actor_id).bind::<Nullable<Text>,_>(&caller.credential_id).bind::<Text,_>(detail.to_string()).execute(conn)?;
            for line in &input.entries {
                let amount = after.get(&(line.purpose_id.clone(),line.resource.clone())).copied()
                    .ok_or_else(|| invalid("validated allocation balance is missing"))?;
                save_entry(conn, &operation_id, line, amount)?;
            }
            let mut result=json!({"operationId":operation_id});
            if let Some(ack)=acknowledgement {result["executionAcknowledgement"]=ack;}
            complete_with_id(conn, &record, &caller, &ledger, &operation_id, result, day, &timezone, &request_id)
        }).await
    }

    pub async fn fund_reconciliation(
        &self,
        resources: Vec<FundResource>,
        caller: &JournalCaller,
    ) -> JournalResult<Vec<super::FundReconciliationItem>> {
        caller.require_read()?;
        let preview = self.preview_allocations(resources, caller).await?;
        let mut conn = get_connection(self.store.pool())?;
        conn.transaction::<_, JournalError, _>(|conn| {
            let current: Vec<_> = preview
                .resources
                .iter()
                .map(|r| r.observation.clone())
                .collect();
            self.resources.validate(conn, &current)?;
            if build_preview(conn, current, preview.as_of_date)?.preview_hash
                != preview.preview_hash
            {
                return Err(JournalError::StateConflict);
            }
            fund_reconciliation_items(conn, &preview)
        })
    }

    /// Rebuild only the disposable ownership balances. No source fact,
    /// immutable entry, request, or first-use marker is created or replayed.
    pub async fn rebuild_allocations(&self, caller: &JournalCaller) -> JournalResult<usize> {
        caller.require_write()?;
        transact(&self.store, move |conn| {
            #[derive(QueryableByName)]
            struct Entry {
                #[diesel(sql_type=Text)]
                operation_id: String,
                #[diesel(sql_type=Text)]
                body: String,
            }
            let rows = diesel::sql_query(REBUILD_ENTRIES).load::<Entry>(conn)?;
            let mut balances: BTreeMap<
                (String, FundResource),
                (AllocationDelta, ExactDecimal, String),
            > = BTreeMap::new();
            for row in rows {
                let entry: AllocationDelta = serde_json::from_str(&row.body).map_err(json_error)?;
                let key = (entry.purpose_id.clone(), entry.resource.clone());
                let previous = balances
                    .get(&key)
                    .map(|(_, amount, _)| *amount)
                    .unwrap_or_default();
                let amount = previous.checked_add(entry.delta)?;
                if amount < ExactDecimal::ZERO {
                    return Err(JournalError::NegativeAllocation);
                }
                balances.insert(key, (entry, amount, row.operation_id));
            }
            diesel::sql_query("DELETE FROM fund_allocations").execute(conn)?;
            for (entry, amount, operation) in balances.values() {
                save_balance(conn, operation, entry, *amount)?;
            }
            Ok(balances.len())
        })
        .await
    }

    fn request<T: Serialize>(
        &self,
        operation: &str,
        key: &str,
        input: &T,
        caller: &JournalCaller,
    ) -> JournalResult<RequestRecord> {
        nonblank(key, "requestKey")?;
        if key.len() > 256 {
            return Err(invalid("requestKey is too long"));
        }
        Ok(RequestRecord::new(
            &self.ledger_id,
            caller.credential_id.as_deref().unwrap_or(&caller.actor_id),
            operation,
            key,
            serde_json::to_value(input).map_err(json_error)?,
        ))
    }
}

pub(crate) async fn transact<T, F>(store: &BusinessStore, job: F) -> JournalResult<T>
where
    T: Send + 'static,
    F: FnOnce(&mut SqliteConnection) -> JournalResult<T> + Send + 'static,
{
    // The savepoint rolls back typed business rejections too; the outer writer
    // keeps the existing single-writer transaction and DatabaseOwner lifecycle.
    store
        .transaction(move |tx| Ok(tx.conn().transaction::<_, JournalError, _>(job)))
        .await?
}

fn unique_resources(resources: Vec<FundResource>) -> JournalResult<Vec<FundResource>> {
    for resource in &resources {
        resource.validate()?;
    }
    let resources: Vec<_> = resources
        .into_iter()
        .collect::<BTreeSet<_>>()
        .into_iter()
        .collect();
    if resources.is_empty() {
        return Err(invalid("at least one resource is required"));
    }
    Ok(resources)
}

fn verify_observations(
    resources: &[FundResource],
    observations: &[ResourceObservation],
    day: NaiveDate,
) -> JournalResult<()> {
    let observed: BTreeSet<_> = observations.iter().map(|o| &o.resource).collect();
    if observations.len() != resources.len()
        || observed.len() != resources.len()
        || resources.iter().any(|r| !observed.contains(r))
        || observations
            .iter()
            .any(|o| o.as_of_date != day || o.state_version.is_empty())
    {
        return Err(invalid(
            "accounting returned inconsistent resource observations",
        ));
    }
    Ok(())
}

pub(super) fn build_preview(
    conn: &mut SqliteConnection,
    mut observations: Vec<ResourceObservation>,
    day: NaiveDate,
) -> JournalResult<AllocationPreview> {
    let rows = allocation_rows(conn)?;
    observations.sort_by(|a, b| a.resource.cmp(&b.resource));
    let mut resources = Vec::new();
    for observation in observations {
        let purposes: BTreeMap<_, _> = rows
            .iter()
            .filter(|((_, r), _)| *r == observation.resource)
            .map(|((id, _), value)| (id.clone(), *value))
            .collect();
        let allocated = purposes
            .values()
            .try_fold(ExactDecimal::ZERO, |sum, v| sum.checked_add(*v))?;
        resources.push(FundResourceStatus {
            balance: ResourceBalance::calculate(observation.actual, allocated)?,
            observation,
            purposes,
        });
    }
    let mut preview = AllocationPreview {
        schema_version: 1,
        as_of_date: day,
        resources,
        source_references: vec![],
        preview_hash: String::new(),
    };
    let identities: Vec<_> = preview
        .resources
        .iter()
        .map(|r| r.observation.resource.clone())
        .collect();
    preview.source_references = super::sources::used_source_references(conn, &identities)?;
    let mut stable = serde_json::to_value(&preview).map_err(json_error)?;
    // Observation refresh timestamps are presentation metadata, not facts.
    if let Some(rows) = stable["resources"].as_array_mut() {
        for row in rows {
            row["observation"]
                .as_object_mut()
                .map(|o| o.remove("observedAt"));
        }
    }
    preview.preview_hash = crate::requests::hash_json(&crate::requests::canonicalize(stable));
    Ok(preview)
}

pub(super) fn fund_reconciliation_items(
    conn: &mut SqliteConnection,
    preview: &AllocationPreview,
) -> JournalResult<Vec<super::FundReconciliationItem>> {
    let baselines = confirmed_observations(conn)?;
    let mut items = super::sources::reference_reconciliation(conn, &preview.source_references)?;
    for row in &preview.resources {
        let baseline = baselines
            .get(&row.observation.resource)
            .unwrap_or(&row.observation);
        items.extend(super::compare_fund_observation(
            baseline,
            &row.observation,
            row.balance.allocated,
        )?);
    }
    Ok(items)
}

#[derive(QueryableByName)]
struct JsonRow {
    #[diesel(sql_type = Text)]
    body: String,
}

pub(super) fn confirmed_observations(
    conn: &mut SqliteConnection,
) -> JournalResult<BTreeMap<FundResource, ResourceObservation>> {
    #[derive(QueryableByName)]
    struct BaselineRow {
        #[diesel(sql_type=Text)]
        observation_json: String,
        #[diesel(sql_type=Text)]
        source_confirmation_json: String,
    }
    let rows=diesel::sql_query("SELECT observation_json,source_confirmation_json FROM fund_allocation_operations ORDER BY rowid").load::<BaselineRow>(conn)?;
    let mut baselines = BTreeMap::new();
    for row in rows {
        let observed: Value = serde_json::from_str(&row.observation_json).map_err(json_error)?;
        let confirmed: Value =
            serde_json::from_str(&row.source_confirmation_json).map_err(json_error)?;
        let observations: Vec<ResourceObservation> =
            serde_json::from_value(observed["resources"].clone()).map_err(json_error)?;
        let resources = confirmed["resources"]
            .as_array()
            .ok_or_else(|| invalid("stored source confirmation is invalid"))?;
        for resource in resources {
            let identity: FundResource =
                serde_json::from_value(resource["resource"].clone()).map_err(json_error)?;
            let observation = observations
                .iter()
                .find(|o| o.resource == identity)
                .ok_or_else(|| invalid("source confirmation is missing its observation"))?;
            baselines.insert(identity, observation.clone());
        }
    }
    Ok(baselines)
}

pub(super) fn purpose_rows(conn: &mut SqliteConnection) -> JournalResult<Vec<Value>> {
    diesel::sql_query(SELECT_PURPOSES)
        .load::<JsonRow>(conn)?
        .into_iter()
        .map(|r| serde_json::from_str(&r.body).map_err(json_error))
        .collect()
}

#[derive(QueryableByName)]
struct AllocationRow {
    #[diesel(sql_type=Text)]
    purpose_id: String,
    #[diesel(sql_type=Text)]
    resource_kind: String,
    #[diesel(sql_type=Text)]
    account_id: String,
    #[diesel(sql_type=Nullable<Text>)]
    currency: Option<String>,
    #[diesel(sql_type=Nullable<Text>)]
    asset_id: Option<String>,
    #[diesel(sql_type=Text)]
    amount: String,
}

pub(super) fn allocation_rows(
    conn: &mut SqliteConnection,
) -> JournalResult<BTreeMap<(String, FundResource), ExactDecimal>> {
    diesel::sql_query("SELECT purpose_id,resource_kind,account_id,currency,asset_id,COALESCE(cash_amount,quantity) AS amount FROM fund_allocations ORDER BY id")
        .load::<AllocationRow>(conn)?.into_iter().map(|r| {
            let resource = match r.resource_kind.as_str() {
                "CASH" => FundResource::Cash { account_id:r.account_id,currency:r.currency.ok_or_else(|| invalid("stored cash has no currency"))? },
                "SECURITY" => FundResource::Security { account_id:r.account_id,asset_id:r.asset_id.ok_or_else(|| invalid("stored security has no asset"))? },
                _ => return Err(invalid("unknown stored resource kind")),
            };
            Ok(((r.purpose_id,resource),ExactDecimal::parse(&r.amount)?.nonnegative()?))
        }).collect()
}

fn resource_has_entries(
    conn: &mut SqliteConnection,
    resource: &FundResource,
) -> JournalResult<bool> {
    #[derive(QueryableByName)]
    struct Count {
        #[diesel(sql_type=diesel::sql_types::BigInt)]
        count: i64,
    }
    Ok(diesel::sql_query("SELECT COUNT(*) AS count FROM fund_allocation_entries WHERE resource_kind=json_extract(?1,'$.kind') AND account_id=json_extract(?1,'$.accountId') AND currency IS json_extract(?1,'$.currency') AND asset_id IS json_extract(?1,'$.assetId')")
        .bind::<Text,_>(serde_json::to_string(resource).map_err(json_error)?).get_result::<Count>(conn)?.count>0)
}

fn save_entry(
    conn: &mut SqliteConnection,
    operation_id: &str,
    line: &AllocationDelta,
    balance: ExactDecimal,
) -> JournalResult<()> {
    let (kind, currency, asset, cash_delta, quantity_delta) = match &line.resource {
        FundResource::Cash { currency, .. } => (
            "CASH",
            Some(currency.as_str()),
            None,
            Some(line.delta.to_string()),
            None,
        ),
        FundResource::Security { asset_id, .. } => (
            "SECURITY",
            None,
            Some(asset_id.as_str()),
            None,
            Some(line.delta.to_string()),
        ),
    };
    let entry = json!({"id":uuid::Uuid::new_v4().to_string(),"operation":operation_id,"purpose":line.purpose_id,"kind":kind,"account":line.resource.account_id(),"currency":currency,"asset":asset,"cashDelta":cash_delta,"quantityDelta":quantity_delta,"at":now()}).to_string();
    diesel::sql_query(INSERT_ALLOCATION_ENTRY)
        .bind::<Text, _>(entry)
        .execute(conn)?;
    save_balance(conn, operation_id, line, balance)
}

fn save_balance(
    conn: &mut SqliteConnection,
    operation_id: &str,
    line: &AllocationDelta,
    balance: ExactDecimal,
) -> JournalResult<()> {
    let (kind, currency, asset, cash, quantity) = match &line.resource {
        FundResource::Cash { currency, .. } => (
            "CASH",
            Some(currency.as_str()),
            None,
            Some(balance.to_string()),
            None,
        ),
        FundResource::Security { asset_id, .. } => (
            "SECURITY",
            None,
            Some(asset_id.as_str()),
            None,
            Some(balance.to_string()),
        ),
    };
    // Remove and insert only the rebuildable balance; immutable entries remain.
    diesel::sql_query("DELETE FROM fund_allocations WHERE purpose_id=?1 AND account_id=?2 AND resource_kind=?3 AND currency IS ?4 AND asset_id IS ?5")
        .bind::<Text,_>(&line.purpose_id).bind::<Text,_>(line.resource.account_id()).bind::<Text,_>(kind).bind::<Nullable<Text>,_>(currency).bind::<Nullable<Text>,_>(asset).execute(conn)?;
    let row = json!({"id":uuid::Uuid::new_v4().to_string(),"operation":operation_id,"purpose":line.purpose_id,"kind":kind,"account":line.resource.account_id(),"currency":currency,"asset":asset,"cash":cash,"quantity":quantity}).to_string();
    diesel::sql_query(INSERT_ALLOCATION_BALANCE)
        .bind::<Text, _>(row)
        .execute(conn)?;
    Ok(())
}

fn replay(conn: &mut SqliteConnection, record: &RequestRecord) -> JournalResult<Option<Value>> {
    let row = business_requests::table
        .filter(business_requests::ledger_id.eq(&record.ledger_id))
        .filter(business_requests::subject.eq(&record.subject))
        .filter(business_requests::operation.eq(&record.operation))
        .filter(business_requests::request_key.eq(&record.request_key))
        .first::<RequestRow>(conn)
        .optional()?;
    match RequestReplay::decide(row.as_ref(), &record.request_hash) {
        RequestReplay::Conflict => Err(JournalError::RequestConflict),
        RequestReplay::Completed(text) => {
            let mut result: Value = serde_json::from_str(&text).map_err(json_error)?;
            result["replayed"] = json!(true);
            Ok(Some(result))
        }
        _ => Ok(None),
    }
}

fn now() -> String {
    wealthfolio_core::utils::clock::now().to_rfc3339()
}
fn json_error(e: serde_json::Error) -> JournalError {
    invalid(e.to_string())
}

#[allow(clippy::too_many_arguments)]
fn complete(
    conn: &mut SqliteConnection,
    record: &RequestRecord,
    caller: &JournalCaller,
    ledger: &str,
    entity: &str,
    result: Value,
    day: NaiveDate,
    timezone: &str,
) -> JournalResult<Value> {
    complete_with_id(
        conn,
        record,
        caller,
        ledger,
        entity,
        result,
        day,
        timezone,
        &uuid::Uuid::new_v4().to_string(),
    )
}

#[allow(clippy::too_many_arguments)]
fn complete_with_id(
    conn: &mut SqliteConnection,
    record: &RequestRecord,
    caller: &JournalCaller,
    ledger: &str,
    entity: &str,
    mut result: Value,
    day: NaiveDate,
    timezone: &str,
    request_id: &str,
) -> JournalResult<Value> {
    let row = record_completed_request(conn, record, ledger, request_id, &mut result)?;
    audit_completed_request(
        conn,
        record,
        caller,
        ledger,
        entity,
        request_id,
        &row.created_at,
        day,
        timezone,
    )?;
    Ok(result)
}

fn record_completed_request(
    conn: &mut SqliteConnection,
    record: &RequestRecord,
    ledger: &str,
    request_id: &str,
    result: &mut Value,
) -> JournalResult<RequestRow> {
    result["requestId"] = json!(request_id);
    result["replayed"] = json!(false);
    let at = now();
    let row = NewRequestRow {
        id: request_id.into(),
        ledger_id: ledger.into(),
        subject: record.subject.clone(),
        operation: record.operation.clone(),
        request_key: record.request_key.clone(),
        request_hash: record.request_hash.clone(),
        request_snapshot: record.request_snapshot.to_string(),
        state: "completed".into(),
        result_json: Some(result.to_string()),
        error_message: None,
        created_at: at.clone(),
        updated_at: at.clone(),
    };
    diesel::insert_into(business_requests::table)
        .values(&row)
        .execute(conn)?;
    Ok(RequestRow::from_new(row))
}

#[allow(clippy::too_many_arguments)]
fn audit_completed_request(
    conn: &mut SqliteConnection,
    record: &RequestRecord,
    caller: &JournalCaller,
    ledger: &str,
    entity: &str,
    request_id: &str,
    at: &str,
    day: NaiveDate,
    timezone: &str,
) -> JournalResult<()> {
    insert_completed_audit(conn, record, caller, ledger, entity, request_id)?;
    ensure_enabled(conn, at, day, timezone)
}

fn insert_completed_audit(
    conn: &mut SqliteConnection,
    record: &RequestRecord,
    caller: &JournalCaller,
    ledger: &str,
    entity: &str,
    request_id: &str,
) -> JournalResult<()> {
    let audit = BusinessAuditEntry::new(ledger,caller.channel,&caller.actor_id,caller.authenticated,&record.operation,"investment_journal")
        .entity(entity).request(request_id).detail(json!({"schemaVersion":1,"request":record.request_snapshot,"actorKind":caller.actor_kind.as_str(),"credentialId":caller.credential_id})).to_row();
    diesel::insert_into(business_audit::table)
        .values(&audit)
        .execute(conn)?;
    Ok(())
}

pub(crate) fn ensure_enabled(
    conn: &mut SqliteConnection,
    at: &str,
    day: NaiveDate,
    timezone: &str,
) -> JournalResult<()> {
    // One insert per marker, in the business transaction. Settings belong to
    // this ledger DB and survive later channel or timezone changes.
    for (key, value) in [
        ("investments.enabled_at", at.to_string()),
        ("investments.enabled_on", day.to_string()),
        ("investments.enabled_timezone", timezone.to_string()),
    ] {
        diesel::sql_query("INSERT INTO app_settings(setting_key,setting_value) VALUES(?1,?2) ON CONFLICT(setting_key) DO NOTHING")
            .bind::<Text,_>(key).bind::<Text,_>(value).execute(conn)?;
    }
    Ok(())
}

const UPDATE_PURPOSE: &str = r#"UPDATE fund_purposes
SET name=json_extract(?1,'$.name'),category=json_extract(?1,'$.category'),currency=json_extract(?1,'$.currency'),target_amount=json_extract(?1,'$.targetAmount'),use_on=json_extract(?1,'$.useOn'),liquidity=json_extract(?1,'$.liquidity'),minimum_cash=json_extract(?1,'$.minimumCash'),loss_budget=json_extract(?1,'$.lossBudget'),single_asset_limit_bps=json_extract(?1,'$.singleAssetLimitBps'),is_archived=json_extract(?1,'$.isArchived'),version=version+1,updated_at=?2
WHERE id=?3 AND version=?4"#;

const INSERT_PURPOSE: &str = r#"INSERT INTO fund_purposes(id,name,category,currency,target_amount,use_on,liquidity,minimum_cash,loss_budget,single_asset_limit_bps,is_archived,created_at,updated_at)
VALUES( ?1,json_extract(?2,'$.name'),json_extract(?2,'$.category'),json_extract(?2,'$.currency'),json_extract(?2,'$.targetAmount'),json_extract(?2,'$.useOn'),json_extract(?2,'$.liquidity'),json_extract(?2,'$.minimumCash'),json_extract(?2,'$.lossBudget'),json_extract(?2,'$.singleAssetLimitBps'),json_extract(?2,'$.isArchived'),?3,?3)"#;

const INSERT_ALLOCATION_OPERATION: &str = r#"INSERT INTO fund_allocation_operations(id,operation_kind,request_id,reason,observation_json,source_confirmation_json,detail_json,created_at,created_channel,created_actor_kind,created_actor_id,created_credential_id)
VALUES( ?1,?2,?3,?4,?5,?6,?12,?7,?8,?9,?10,?11)"#;

const REBUILD_ENTRIES: &str = r#"SELECT operation_id,json_object('purposeId',purpose_id,'resource',json(CASE WHEN resource_kind='CASH' THEN json_object('kind','CASH','accountId',account_id,'currency',currency) ELSE json_object('kind','SECURITY','accountId',account_id,'assetId',asset_id) END),'delta',COALESCE(cash_delta,quantity_delta)) AS body
FROM fund_allocation_entries
ORDER BY rowid"#;

const SELECT_PURPOSES: &str = r#"SELECT json_object('id',id,'name',name,'category',category,'currency',currency,'targetAmount',target_amount,'useOn',use_on,'liquidity',liquidity,'minimumCash',minimum_cash,'lossBudget',loss_budget,'singleAssetLimitBps',single_asset_limit_bps,'isArchived',json(CASE WHEN is_archived=1 THEN 'true' ELSE 'false' END),'version',version) AS body
FROM fund_purposes
ORDER BY id"#;

const INSERT_ALLOCATION_ENTRY: &str = r#"INSERT INTO fund_allocation_entries(id,operation_id,purpose_id,resource_kind,account_id,currency,asset_id,cash_delta,quantity_delta,created_at)
VALUES( json_extract(?1,'$.id'),json_extract(?1,'$.operation'),json_extract(?1,'$.purpose'),json_extract(?1,'$.kind'),json_extract(?1,'$.account'),json_extract(?1,'$.currency'),json_extract(?1,'$.asset'),json_extract(?1,'$.cashDelta'),json_extract(?1,'$.quantityDelta'),json_extract(?1,'$.at'))"#;

const INSERT_ALLOCATION_BALANCE: &str = r#"INSERT INTO fund_allocations(id,last_operation_id,purpose_id,resource_kind,account_id,currency,asset_id,cash_amount,quantity)
VALUES( json_extract(?1,'$.id'),json_extract(?1,'$.operation'),json_extract(?1,'$.purpose'),json_extract(?1,'$.kind'),json_extract(?1,'$.account'),json_extract(?1,'$.currency'),json_extract(?1,'$.asset'),json_extract(?1,'$.cash'),json_extract(?1,'$.quantity'))"#;

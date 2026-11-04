//! Restartable execution indexing. It never replays ownership or enables the journal.

use std::sync::Arc;

use chrono::NaiveDate;
use diesel::prelude::*;
use diesel::sql_types::{BigInt, Integer, Text};
use serde::{Deserialize, Serialize};
use serde_json::{json, Value};
use wealthfolio_storage_sqlite::{
    activities::ActivityDB,
    db::get_connection,
    portfolio::snapshot::AccountStateSnapshotDB,
    schema::{activities, business_requests, holdings_snapshots},
};

use super::{
    model::invalid, service::transact, ExactDecimal, InvestmentSourceKind, JournalError,
    JournalResult,
};
use crate::{store::RequestRow, BusinessStore};

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, schemars::JsonSchema)]
#[serde(rename_all = "SCREAMING_SNAKE_CASE")]
pub enum ExecutionStatus {
    Valid,
    Reconcile,
    Invalid,
    Deleted,
}

#[derive(Debug, Clone, Serialize, Deserialize, schemars::JsonSchema)]
#[serde(rename_all = "camelCase")]
pub struct InvestmentExecution {
    pub id: String,
    pub source_kind: InvestmentSourceKind,
    pub source_id: String,
    pub source_version: String,
    pub status: ExecutionStatus,
    pub account_id: String,
    pub asset_id: Option<String>,
    pub side: String,
    pub quantity: Option<ExactDecimal>,
    pub gross_amount: Option<ExactDecimal>,
    pub activity_currency: String,
    pub net_cash: Option<ExactDecimal>,
    pub settlement_currency: Option<String>,
    pub fee: Option<ExactDecimal>,
    pub tax: Option<ExactDecimal>,
    pub cash_basis: String,
    pub occurred_at: String,
    pub time_precision: String,
    pub snapshot: Value,
}

impl InvestmentExecution {
    pub fn stable_id(kind: &InvestmentSourceKind, id: &str) -> String {
        format!(
            "execution-{}",
            crate::requests::hash_json(&json!([kind, id]))
        )
    }
}

#[derive(Debug, Clone)]
pub struct ProjectionSource {
    pub kind: InvestmentSourceKind,
    pub id: String,
    pub generation: i64,
    pub deleted: bool,
    pub snapshot: Value,
}

#[derive(Debug, Clone)]
pub struct PreparedExecutionProjection {
    pub as_of_date: NaiveDate,
    /// Other-fact guard for ledger trades. Frozen manual requests and non-trade
    /// markers only need their own persistent generation check.
    pub state_version: Option<String>,
    pub execution: Option<InvestmentExecution>,
    pub resource_impact: Value,
}

#[async_trait::async_trait]
pub trait InvestmentExecutionReader: Send + Sync {
    async fn prepare_execution(
        &self,
        source: &ProjectionSource,
    ) -> JournalResult<PreparedExecutionProjection>;
    fn validate_execution(
        &self,
        conn: &mut SqliteConnection,
        prepared: &PreparedExecutionProjection,
    ) -> JournalResult<()>;
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct BackfillProgress {
    pub last_source_id: Option<String>,
    pub scanned: usize,
    pub exhausted: bool,
}

#[derive(Debug, Clone, Default, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ProjectionProgress {
    pub processed: usize,
    pub conflicted: usize,
    pub remaining: i64,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ProjectionMaintenanceProgress {
    pub scanned: usize,
    pub refreshed: ProjectionProgress,
    pub historical_complete: bool,
}

#[derive(Clone)]
pub struct InvestmentProjectionService {
    store: BusinessStore,
    reader: Arc<dyn InvestmentExecutionReader>,
}

impl InvestmentProjectionService {
    pub fn new(store: BusinessStore, reader: Arc<dyn InvestmentExecutionReader>) -> Self {
        Self { store, reader }
    }

    /// One restartable background slice. Cursor progress is persisted by the
    /// same backfill command, never inferred from an empty pending queue.
    pub async fn maintenance_batch(
        &self,
        limit: u32,
    ) -> JournalResult<ProjectionMaintenanceProgress> {
        batch_limit(limit)?;
        let _window = self.store.investment_read_window().write().await;
        let next = {
            let mut conn = get_connection(self.store.pool())?;
            #[derive(QueryableByName)]
            struct Setting {
                #[diesel(sql_type=Text)]
                setting_key: String,
                #[diesel(sql_type=Text)]
                setting_value: String,
            }
            let settings=diesel::sql_query("SELECT setting_key,setting_value FROM app_settings WHERE setting_key LIKE 'investment_projection.backfill.%'").load::<Setting>(&mut conn)?.into_iter().map(|row|(row.setting_key,row.setting_value)).collect::<std::collections::HashMap<_,_>>();
            [
                InvestmentSourceKind::Activity,
                InvestmentSourceKind::ManualTrade,
                InvestmentSourceKind::Snapshot,
            ]
            .into_iter()
            .find_map(|kind| {
                let prefix = format!("investment_projection.backfill.{}", kind.as_str());
                let cursor = settings.get(&format!("{prefix}.cursor")).cloned();
                let complete = cursor.is_some()
                    && settings
                        .get(&format!("{prefix}.complete"))
                        .is_some_and(|value| value == "true");
                (!complete).then_some((kind, cursor))
            })
        };
        let scanned = if let Some((kind, cursor)) = next {
            self.backfill_batch(kind, cursor, limit).await?.scanned
        } else {
            0
        };
        let refreshed = self.refresh_batch(limit).await?;
        let mut conn = get_connection(self.store.pool())?;
        let complete=diesel::sql_query("SELECT COUNT(*) AS count FROM app_settings WHERE setting_key IN ('investment_projection.backfill.ACTIVITY.complete','investment_projection.backfill.MANUAL_TRADE.complete','investment_projection.backfill.SNAPSHOT.complete') AND setting_value='true'").get_result::<Count>(&mut conn)?.count==3;
        Ok(ProjectionMaintenanceProgress {
            scanned,
            refreshed,
            historical_complete: complete,
        })
    }

    /// Operational metadata only; this never enables the journal or records an allocation.
    pub async fn record_background_error(&self, code: Option<&str>) -> JournalResult<()> {
        let _window = self.store.investment_read_window().write().await;
        let code = code.map(str::to_string);
        transact(&self.store,move|conn|{
            if let Some(code)=code {
                diesel::sql_query("INSERT INTO app_settings(setting_key,setting_value) VALUES('investment_projection.background_error',?1) ON CONFLICT(setting_key) DO UPDATE SET setting_value=excluded.setting_value").bind::<Text,_>(code).execute(conn)?;
            } else {diesel::sql_query("DELETE FROM app_settings WHERE setting_key='investment_projection.background_error'").execute(conn)?;}
            Ok(())
        }).await
    }

    /// Restart with no cursor after interruption if necessary: existing marker
    /// generations are never overwritten. New inserts/deletes are captured by
    /// triggers even when their IDs sort before a saved cursor.
    pub async fn backfill_batch(
        &self,
        kind: InvestmentSourceKind,
        after_id: Option<String>,
        limit: u32,
    ) -> JournalResult<BackfillProgress> {
        batch_limit(limit)?;
        transact(&self.store, move |conn| {
            let cursor = after_id.unwrap_or_default();
            let rows: Vec<(String, Value)> = match kind {
                InvestmentSourceKind::Activity => activities::table
                    .filter(activities::id.gt(&cursor))
                    .filter(
                        diesel::dsl::sql::<Text>(&format!(
                            "UPPER({})",
                            wealthfolio_storage_sqlite::activities::effective_type_sql(
                                "activities"
                            )
                        ))
                        .eq_any(["BUY", "SELL"]),
                    )
                    .order(activities::id.asc())
                    .limit(i64::from(limit))
                    .load::<ActivityDB>(conn)?
                    .into_iter()
                    .map(|r| Ok((r.id.clone(), serde_json::to_value(r).map_err(json_error)?)))
                    .collect::<JournalResult<_>>()?,
                InvestmentSourceKind::ManualTrade => business_requests::table
                    .filter(business_requests::id.gt(&cursor))
                    .filter(
                        business_requests::operation
                            .eq_any(["manual_trade_record", "investment_manual_trade_record"]),
                    )
                    .filter(business_requests::state.eq("completed"))
                    .order(business_requests::id.asc())
                    .limit(i64::from(limit))
                    .load::<RequestRow>(conn)?
                    .into_iter()
                    .map(|r| Ok((r.id.clone(), serde_json::to_value(r).map_err(json_error)?)))
                    .collect::<JournalResult<_>>()?,
                InvestmentSourceKind::Snapshot => holdings_snapshots::table
                    .filter(holdings_snapshots::id.gt(&cursor))
                    .filter(holdings_snapshots::source.ne("CALCULATED"))
                    .order(holdings_snapshots::id.asc())
                    .limit(i64::from(limit))
                    .load::<AccountStateSnapshotDB>(conn)?
                    .into_iter()
                    .map(|r| Ok((r.id.clone(), serde_json::to_value(r).map_err(json_error)?)))
                    .collect::<JournalResult<_>>()?,
            };
            for (id, snapshot) in &rows {
                diesel::sql_query(BACKFILL_SOURCE)
                    .bind::<Text, _>(kind.as_str())
                    .bind::<Text, _>(id)
                    .bind::<Text, _>(
                        json!({"schemaVersion":1,"before":null,"after":snapshot}).to_string(),
                    )
                    .execute(conn)?;
            }
            // Only an uninterrupted cursor chain starting at the beginning
            // proves that legacy sources were covered. Arbitrary range scans
            // remain useful without declaring the full index complete.
            #[derive(QueryableByName)]struct Cursor {#[diesel(sql_type=Text)]setting_value:String}
            let key=format!("investment_projection.backfill.{}.cursor",kind.as_str());
            let previous=diesel::sql_query("SELECT setting_value FROM app_settings WHERE setting_key=?1").bind::<Text,_>(&key).get_result::<Cursor>(conn).optional()?;
            let exhausted=rows.len()<limit as usize;
            if cursor.is_empty() || previous.is_some_and(|p|p.setting_value==cursor) {
                let last=rows.last().map(|(id,_)|id.as_str()).unwrap_or(&cursor);
                for (key,value) in [(key,last.to_string()),(format!("investment_projection.backfill.{}.complete",kind.as_str()),exhausted.to_string())] {
                    diesel::sql_query("INSERT INTO app_settings(setting_key,setting_value) VALUES(?1,?2) ON CONFLICT(setting_key) DO UPDATE SET setting_value=excluded.setting_value")
                        .bind::<Text,_>(key).bind::<Text,_>(value).execute(conn)?;
                }
            }
            Ok(BackfillProgress {last_source_id:rows.last().map(|(id,_)|id.clone()),scanned:rows.len(),exhausted})
        })
        .await
    }

    pub async fn refresh_batch(&self, limit: u32) -> JournalResult<ProjectionProgress> {
        batch_limit(limit)?;
        let pending = {
            let mut conn = get_connection(self.store.pool())?;
            diesel::sql_query("SELECT source_kind,source_id,generation,is_deleted,source_snapshot_json FROM investment_projection_state WHERE processed_generation<generation ORDER BY source_kind,source_id LIMIT ?1")
                .bind::<BigInt,_>(i64::from(limit)).load::<SourceRow>(&mut conn)?
        };
        let mut progress = ProjectionProgress::default();
        for row in pending {
            let source = row.into_source()?;
            let claim = source.clone();
            let claimed=transact(&self.store,move |conn| {
                Ok(diesel::sql_query("UPDATE investment_projection_state SET refresh_target_generation=?1 WHERE source_kind=?2 AND source_id=?3 AND generation=?1 AND processed_generation<?1")
                    .bind::<BigInt,_>(claim.generation).bind::<Text,_>(claim.kind.as_str()).bind::<Text,_>(&claim.id).execute(conn)?==1)
            }).await?;
            if !claimed {
                progress.conflicted += 1;
                continue;
            }
            let prepared = match self.reader.prepare_execution(&source).await {
                Ok(p) => p,
                Err(JournalError::StateConflict) => {
                    progress.conflicted += 1;
                    continue;
                }
                Err(e) => return Err(e),
            };
            let reader = self.reader.clone();
            let committed=transact(&self.store,move |conn| {
                let current=source_row(conn,&source.kind,&source.id)?;
                if current.is_none_or(|r| r.generation!=source.generation) { return Err(JournalError::StateConflict); }
                reader.validate_execution(conn,&prepared)?;
                if let Some(execution)=&prepared.execution {
                    if source.deleted || execution.source_kind!=source.kind || execution.source_id!=source.id {
                        return Err(invalid("projector returned an execution for a different source"));
                    }
                    save_execution(conn,execution)?;
                } else {
                    let status=if source.deleted { "DELETED" } else { "INVALID" };
                    let version=crate::requests::hash_json(&crate::requests::canonicalize(source.snapshot.clone()));
                    diesel::sql_query("UPDATE investment_executions SET status=?1,source_version=?2 WHERE source_kind=?3 AND source_id=?4")
                        .bind::<Text,_>(status).bind::<Text,_>(version).bind::<Text,_>(source.kind.as_str()).bind::<Text,_>(&source.id).execute(conn)?;
                }
                diesel::sql_query("UPDATE investment_projection_state SET processed_generation=?1,refresh_target_generation=NULL,resource_impact_json=?2 WHERE source_kind=?3 AND source_id=?4 AND generation=?1")
                    .bind::<BigInt,_>(source.generation).bind::<Text,_>(prepared.resource_impact.to_string())
                    .bind::<Text,_>(source.kind.as_str()).bind::<Text,_>(&source.id).execute(conn)?;
                Ok(())
            }).await;
            match committed {
                Ok(()) => progress.processed += 1,
                Err(JournalError::StateConflict) => progress.conflicted += 1,
                Err(e) => return Err(e),
            }
        }
        let mut conn = get_connection(self.store.pool())?;
        progress.remaining=diesel::sql_query("SELECT COUNT(*) AS count FROM investment_projection_state WHERE processed_generation<generation").get_result::<Count>(&mut conn)?.count;
        Ok(progress)
    }

    pub fn executions(&self) -> JournalResult<Vec<InvestmentExecution>> {
        let mut conn = get_connection(self.store.pool())?;
        let rows = diesel::sql_query(SELECT_EXECUTIONS).load::<JsonRow>(&mut conn)?;
        rows.into_iter()
            .map(|r| serde_json::from_str(&r.body).map_err(json_error))
            .collect()
    }
}

pub(crate) fn save_execution(
    conn: &mut SqliteConnection,
    execution: &InvestmentExecution,
) -> JournalResult<()> {
    if execution.source_kind == InvestmentSourceKind::Snapshot
        || !matches!(execution.side.as_str(), "BUY" | "SELL")
    {
        return Err(invalid("only completed trades may become executions"));
    }
    diesel::sql_query(UPSERT_EXECUTION)
        .bind::<Text, _>(serde_json::to_string(execution).map_err(json_error)?)
        .execute(conn)?;
    Ok(())
}

fn batch_limit(limit: u32) -> JournalResult<()> {
    if !(1..=1000).contains(&limit) {
        Err(invalid("projection batch limit must be between 1 and 1000"))
    } else {
        Ok(())
    }
}
fn json_error(e: serde_json::Error) -> JournalError {
    invalid(e.to_string())
}

#[derive(QueryableByName)]
struct SourceRow {
    #[diesel(sql_type=Text)]
    source_kind: String,
    #[diesel(sql_type=Text)]
    source_id: String,
    #[diesel(sql_type=BigInt)]
    generation: i64,
    #[diesel(sql_type=Integer)]
    is_deleted: i32,
    #[diesel(sql_type=Text)]
    source_snapshot_json: String,
}
impl SourceRow {
    fn into_source(self) -> JournalResult<ProjectionSource> {
        let kind = match self.source_kind.as_str() {
            "ACTIVITY" => InvestmentSourceKind::Activity,
            "MANUAL_TRADE" => InvestmentSourceKind::ManualTrade,
            "SNAPSHOT" => InvestmentSourceKind::Snapshot,
            _ => return Err(invalid("unknown source kind")),
        };
        Ok(ProjectionSource {
            kind,
            id: self.source_id,
            generation: self.generation,
            deleted: self.is_deleted != 0,
            snapshot: serde_json::from_str(&self.source_snapshot_json).map_err(json_error)?,
        })
    }
}
fn source_row(
    conn: &mut SqliteConnection,
    kind: &InvestmentSourceKind,
    id: &str,
) -> JournalResult<Option<SourceRow>> {
    Ok(diesel::sql_query("SELECT source_kind,source_id,generation,is_deleted,source_snapshot_json FROM investment_projection_state WHERE source_kind=?1 AND source_id=?2")
        .bind::<Text,_>(kind.as_str()).bind::<Text,_>(id).get_result::<SourceRow>(conn).optional()?)
}
#[derive(QueryableByName)]
struct Count {
    #[diesel(sql_type=BigInt)]
    count: i64,
}
#[derive(QueryableByName)]
struct JsonRow {
    #[diesel(sql_type=Text)]
    body: String,
}

const BACKFILL_SOURCE:&str="INSERT INTO investment_projection_state(source_kind,source_id,generation,processed_generation,change_kind,is_deleted,source_snapshot_json) VALUES(?1,?2,1,0,'BACKFILL',0,?3) ON CONFLICT(source_kind,source_id) DO NOTHING";

const UPSERT_EXECUTION: &str = r#"
INSERT INTO investment_executions(id,source_kind,source_id,source_version,status,account_id,asset_id,side,quantity,gross_amount,activity_currency,net_cash,settlement_currency,fee,tax,cash_basis,occurred_at,time_precision,snapshot_json)
VALUES(json_extract(?1,'$.id'),json_extract(?1,'$.sourceKind'),json_extract(?1,'$.sourceId'),json_extract(?1,'$.sourceVersion'),json_extract(?1,'$.status'),json_extract(?1,'$.accountId'),json_extract(?1,'$.assetId'),json_extract(?1,'$.side'),json_extract(?1,'$.quantity'),json_extract(?1,'$.grossAmount'),json_extract(?1,'$.activityCurrency'),json_extract(?1,'$.netCash'),json_extract(?1,'$.settlementCurrency'),json_extract(?1,'$.fee'),json_extract(?1,'$.tax'),json_extract(?1,'$.cashBasis'),json_extract(?1,'$.occurredAt'),json_extract(?1,'$.timePrecision'),json_extract(?1,'$.snapshot'))
ON CONFLICT(source_kind,source_id) DO UPDATE SET source_version=excluded.source_version,status=excluded.status,account_id=excluded.account_id,asset_id=excluded.asset_id,side=excluded.side,quantity=excluded.quantity,gross_amount=excluded.gross_amount,activity_currency=excluded.activity_currency,net_cash=excluded.net_cash,settlement_currency=excluded.settlement_currency,fee=excluded.fee,tax=excluded.tax,cash_basis=excluded.cash_basis,occurred_at=excluded.occurred_at,time_precision=excluded.time_precision,snapshot_json=excluded.snapshot_json
"#;

const SELECT_EXECUTIONS: &str = r#"
SELECT json_object('id',id,'sourceKind',source_kind,'sourceId',source_id,'sourceVersion',source_version,'status',status,'accountId',account_id,'assetId',asset_id,'side',side,'quantity',quantity,'grossAmount',gross_amount,'activityCurrency',activity_currency,'netCash',net_cash,'settlementCurrency',settlement_currency,'fee',fee,'tax',tax,'cashBasis',cash_basis,'occurredAt',occurred_at,'timePrecision',time_precision,'snapshot',json(snapshot_json)) AS body FROM investment_executions ORDER BY occurred_at,id
"#;

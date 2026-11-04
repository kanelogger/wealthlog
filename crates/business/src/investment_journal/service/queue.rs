//! Unified read model. No queue dismissal can erase a numeric ownership deficit.
use super::execution_reconciliation::{acknowledged, original_link_anchor_hash, source_anchor};
use super::*;
use crate::investment_journal::{
    ExecutionStatus, FundReconciliationItem, InvestmentExecution, SourceReferenceState,
};

#[derive(Debug, Clone, Default, Serialize, Deserialize, schemars::JsonSchema)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct InvestmentWorkQuery {
    #[serde(default)]
    pub include_historical_executions: bool,
    pub execution_date_from: Option<NaiveDate>,
    pub execution_date_to: Option<NaiveDate>,
}
#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct InvestmentWorkQueue {
    pub schema_version: u32,
    pub as_of_date: NaiveDate,
    pub enabled_on: Option<NaiveDate>,
    pub boundary_timezone: Option<String>,
    pub items: Vec<Value>,
    pub projection_pending: i64,
    pub historical_backfill_complete: bool,
    pub complete: bool,
}
impl InvestmentJournalService {
    pub async fn investment_work_queue(
        &self,
        query: InvestmentWorkQuery,
        caller: &JournalCaller,
    ) -> JournalResult<InvestmentWorkQueue> {
        caller.require_read()?;
        if query
            .execution_date_from
            .zip(query.execution_date_to)
            .is_some_and(|(from, to)| from > to)
        {
            return Err(invalid("execution date range is reversed"));
        }
        let (day, timezone) = self.business_day()?;
        let resources = {
            let mut conn = get_connection(self.store.pool())?;
            allocation_rows(&mut conn)?
                .keys()
                .map(|(_, r)| r.clone())
                .collect::<BTreeSet<_>>()
                .into_iter()
                .collect::<Vec<_>>()
        };
        let funding = if resources.is_empty() {
            None
        } else {
            Some(self.preview(resources, day).await?)
        };
        let mut conn = get_connection(self.store.pool())?;
        conn.transaction::<_, JournalError, _>(|conn| {
            read_queue(
                conn,
                &query,
                day,
                &timezone,
                funding.as_ref(),
                self.resources.as_ref(),
            )
        })
    }
}
fn read_queue(
    conn: &mut SqliteConnection,
    query: &InvestmentWorkQuery,
    day: NaiveDate,
    timezone: &str,
    funding: Option<&AllocationPreview>,
    reader: &dyn InvestmentResourceReader,
) -> JournalResult<InvestmentWorkQueue> {
    let mut items = Vec::new();
    let mut unknown_dates = false;
    if let Some(funding) = funding {
        let observations: Vec<_> = funding
            .resources
            .iter()
            .map(|r| r.observation.clone())
            .collect();
        reader.validate(conn, &observations)?;
        if build_preview(conn, observations, day)?.preview_hash != funding.preview_hash {
            return Err(JournalError::StateConflict);
        }
        let tasks = fund_reconciliation_items(conn, funding)?;
        for task in tasks {
            items.push(fund_item(task)?);
        }
    }
    let settings = read_settings(conn)?;
    let enabled_on = settings
        .get("investments.enabled_on")
        .map(|s| {
            NaiveDate::parse_from_str(s, "%Y-%m-%d")
                .map_err(|_| invalid("invalid investment enable boundary"))
        })
        .transpose()?;
    let boundary_timezone = settings.get("investments.enabled_timezone").cloned();
    let execution_from = if query.include_historical_executions {
        query.execution_date_from
    } else {
        enabled_on.max(query.execution_date_from)
    };
    let zone = if query.include_historical_executions {
        timezone
    } else {
        boundary_timezone.as_deref().unwrap_or(timezone)
    };
    let default_end = wealthfolio_core::utils::time_utils::user_today(parse_user_timezone(zone)?);
    let rows =
        diesel::sql_query("SELECT id AS body FROM investment_executions ORDER BY occurred_at,id")
            .load::<JsonRow>(conn)?;
    for row in rows {
        let source = source_anchor(conn, &row.body)?;
        let execution = &source.execution;
        let ack = acknowledged(conn, &source)?;
        let valid = source.source_state == SourceReferenceState::Valid
            && matches!(
                execution.status,
                ExecutionStatus::Valid | ExecutionStatus::Reconcile
            );
        if source.active_links.is_empty() && valid {
            match execution_date(execution, zone) {
                Ok(date) => {
                    let selected = (query.include_historical_executions || enabled_on.is_some())
                        && execution_from.is_none_or(|d| date >= d)
                        && date <= query.execution_date_to.unwrap_or(default_end);
                    if selected {
                        items.push(json!({"id":format!("unlinked:{}",execution.id),"kind":"UNLINKED_EXECUTION","execution":execution}));
                    }
                }
                Err(_) if enabled_on.is_some() || query.include_historical_executions => {
                    unknown_dates = true;
                    items.push(json!({"id":format!("execution-date:{}:{}",execution.id,source.source_anchor_hash),"kind":"EXECUTION_DATE_UNAVAILABLE","execution":execution}));
                }
                Err(_) => (),
            }
        }
        let changed = source
            .active_links
            .iter()
            .any(|link| original_link_anchor_hash(link) != source.source_anchor_hash);
        if changed && !ack {
            let kind = if valid {
                "EXECUTION_SOURCE_CHANGED"
            } else {
                "EXECUTION_SOURCE_INVALIDATED"
            };
            items.push(json!({"id":format!("source:{}:{}",execution.id,source.source_anchor_hash),"kind":kind,"source":source}));
        }
        if valid && !ack && ownership_pending(conn, &execution.id)? {
            items.push(json!({"id":format!("ownership:{}:{}",execution.id,source.source_anchor_hash),"kind":"EXECUTION_OWNERSHIP_PENDING","source":source}));
        }
    }
    let checks=diesel::sql_query("SELECT json_object('id','draft-check:'||r.id,'kind','DRAFT_CHECK_DUE','decisionId',d.id,'revisionId',r.id,'revisionNumber',r.revision_number,'title',d.title,'dueOn',r.next_check_at,'version',d.version) AS body FROM investment_decisions d JOIN investment_decision_revisions r ON r.id=d.draft_revision_id WHERE d.lifecycle<>'ARCHIVED' AND r.status='DRAFT' AND r.next_check_at<=?1 ORDER BY r.next_check_at,d.id")
                .bind::<Text,_>(day.to_string()).load::<JsonRow>(conn)?;
    let reviews=diesel::sql_query("SELECT json_object('id','review-due:'||d.id,'kind','REVIEW_DUE','decisionId',d.id,'revisionId',d.current_revision_id,'revisionNumber',r.revision_number,'title',json_extract(r.body_json,'$.title'),'dueOn',d.next_review_at,'version',d.version) AS body FROM investment_decisions d JOIN investment_decision_revisions r ON r.id=d.current_revision_id WHERE d.lifecycle='CONFIRMED' AND d.next_review_at<=?1 ORDER BY d.next_review_at,d.id")
                .bind::<Text,_>(day.to_string()).load::<JsonRow>(conn)?;
    for row in checks.into_iter().chain(reviews) {
        items.push(serde_json::from_str(&row.body).map_err(json_error)?);
    }
    items.sort_by_key(|v| v["id"].as_str().unwrap_or_default().to_string());
    items.dedup_by(|a, b| a["id"] == b["id"]);
    #[derive(QueryableByName)]
    struct Count {
        #[diesel(sql_type=diesel::sql_types::BigInt)]
        count: i64,
    }
    let pending=diesel::sql_query("SELECT COUNT(*) AS count FROM investment_projection_state WHERE processed_generation<generation").get_result::<Count>(conn)?.count;
    let backfill_complete = ["ACTIVITY", "MANUAL_TRADE", "SNAPSHOT"].iter().all(|kind| {
        settings
            .get(&format!("investment_projection.backfill.{kind}.complete"))
            .is_some_and(|v| v == "true")
    });
    Ok(InvestmentWorkQueue {
        schema_version: 1,
        as_of_date: day,
        enabled_on,
        boundary_timezone,
        items,
        projection_pending: pending,
        historical_backfill_complete: backfill_complete,
        complete: pending == 0 && backfill_complete && !unknown_dates,
    })
}

fn fund_item(task: FundReconciliationItem) -> JournalResult<Value> {
    let mut value = serde_json::to_value(task).map_err(json_error)?;
    value["category"] = json!("FUND_RECONCILIATION");
    Ok(value)
}
fn read_settings(conn: &mut SqliteConnection) -> JournalResult<BTreeMap<String, String>> {
    #[derive(QueryableByName)]
    struct Setting {
        #[diesel(sql_type=Text)]
        setting_key: String,
        #[diesel(sql_type=Text)]
        setting_value: String,
    }
    Ok(diesel::sql_query("SELECT setting_key,setting_value FROM app_settings WHERE setting_key LIKE 'investments.%' OR setting_key LIKE 'investment_projection.backfill.%'").load::<Setting>(conn)?.into_iter().map(|s|(s.setting_key,s.setting_value)).collect())
}
pub(super) fn execution_date(
    execution: &InvestmentExecution,
    zone: &str,
) -> JournalResult<NaiveDate> {
    if execution.time_precision == "DATE" {
        // Precision may be date-only even when the legacy source stored midnight.
        // Verify the whole source before extracting its date; a valid prefix alone
        // cannot turn malformed trailing bytes into a verified observation.
        if wealthfolio_storage_sqlite::activities::parse_db_datetime(&execution.occurred_at)
            .is_none()
        {
            return Err(invalid("invalid execution date"));
        }
        return NaiveDate::parse_from_str(
            execution.occurred_at.get(..10).unwrap_or_default(),
            "%Y-%m-%d",
        )
        .map_err(|_| invalid("invalid execution date"));
    }
    let date = wealthfolio_storage_sqlite::activities::parse_db_datetime(&execution.occurred_at)
        .ok_or_else(|| invalid("invalid execution datetime"))?;
    Ok(date.with_timezone(&parse_user_timezone(zone)?).date_naive())
}
fn ownership_pending(conn: &mut SqliteConnection, id: &str) -> JournalResult<bool> {
    #[derive(QueryableByName)]
    struct Count {
        #[diesel(sql_type=diesel::sql_types::BigInt)]
        count: i64,
    }
    Ok(diesel::sql_query("SELECT COUNT(*) AS count FROM business_requests WHERE state='completed' AND operation IN ('investment_manual_trade_record','investment_ledger_trade_record') AND json_valid(result_json) AND json_extract(result_json,'$.executionId')=?1 AND json_extract(result_json,'$.ownershipStatus')='RECONCILIATION_REQUIRED'").bind::<Text,_>(id).get_result::<Count>(conn)?.count>0)
}

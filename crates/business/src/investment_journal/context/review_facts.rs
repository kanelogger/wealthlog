//! Historical observations use native quote selection and original execution sources.
use super::super::{
    model::invalid, service, DecisionBody, ExactDecimal, JournalError, JournalResult,
    ReviewFactsCapture, ReviewFactsQuery, ReviewFactsReader,
};
use super::accounting::{issue, AccountingContextReader};
use super::{ContextQuality, InvestmentContextIssue, InvestmentContextQuery};
use async_trait::async_trait;
use diesel::{prelude::*, sql_types::Text};
use serde_json::{json, Value};
use wealthfolio_core::quotes::Quote;
use wealthfolio_storage_sqlite::db::get_connection;

impl AccountingContextReader {
    fn review_version(
        &self,
        conn: &mut SqliteConnection,
        query: &ReviewFactsQuery,
    ) -> JournalResult<(String, Value)> {
        let observations = service::review_observations(conn, query)?;
        let body: DecisionBody = serde_json::from_value(observations["decision"]["body"].clone())
            .map_err(|e| invalid(e.to_string()))?;
        let financial = super::state::source_version(
            conn,
            &InvestmentContextQuery::for_decision(&body),
            query.observation_cutoff,
            "NATIVE_UNITS",
            &query.observation_timezone,
        )?;
        let version = crate::requests::hash_json(&crate::requests::canonicalize(
            json!({"financial":financial,"query":query,"observations":observations}),
        ));
        Ok((version, observations))
    }

    fn subject_observation(
        &self,
        query: &ReviewFactsQuery,
        observations: &Value,
        issues: &mut Vec<InvestmentContextIssue>,
    ) -> JournalResult<Option<Value>> {
        let Some(asset) = observations["decision"]["body"]["assetId"].as_str() else {
            return Ok(None);
        };
        let mut conn = get_connection(self.store.pool())?;
        #[derive(QueryableByName)]
        struct AssetRow {
            #[diesel(sql_type=Text)]
            body: String,
        }
        let metadata=diesel::sql_query("SELECT json_object('id',id,'name',name,'displayCode',display_code,'currency',quote_ccy,'instrumentType',instrument_type) AS body FROM assets WHERE id=?1")
            .bind::<Text,_>(asset).get_result::<AssetRow>(&mut conn).optional()?.map(|r|serde_json::from_str::<Value>(&r.body).map_err(|e|invalid(e.to_string()))).transpose()?;
        if metadata.is_none() {
            issues.push(issue("ASSET_UNAVAILABLE", Some(asset.into()), None));
        }
        let quotes = self
            .quotes
            .get_latest_quotes_as_of(&[asset.to_string()], query.observation_cutoff);
        let price = match quotes {
            Ok(quotes) => match quotes.get(asset) {
                Some(quote) => {
                    let checks = self
                        .quotes
                        .verify_quote_sources(std::slice::from_ref(quote))
                        .unwrap_or_default();
                    validated_quote(
                        quote,
                        asset,
                        query,
                        checks.get(&quote.id)
                            == Some(&wealthfolio_core::quotes::QuoteSourceQuality::Verified),
                        issues,
                    )?
                }
                None => {
                    issues.push(issue("QUOTE_UNAVAILABLE", Some(asset.into()), None));
                    None
                }
            },
            Err(_) => {
                issues.push(issue("QUOTE_UNAVAILABLE", Some(asset.into()), None));
                None
            }
        };
        Ok(Some(
            json!({"assetId":asset,"metadataAtCapture":metadata,"asOfDate":query.observation_cutoff,"quote":price,"priceBasis":"NATIVE_QUOTE_UNITS","quantityConversion":"NOT_APPLIED"}),
        ))
    }
}

fn validated_quote(
    quote: &Quote,
    asset: &str,
    query: &ReviewFactsQuery,
    verified: bool,
    issues: &mut Vec<InvestmentContextIssue>,
) -> JournalResult<Option<Value>> {
    let valid = verified
        && quote.asset_id == asset
        && quote.timestamp.date_naive() <= query.observation_cutoff;
    if !valid {
        issues.push(issue(
            "QUOTE_SOURCE_INVALID",
            Some(quote.id.clone()),
            Some(quote.currency.clone()),
        ));
        return Ok(None);
    }
    let stale = query
        .observation_cutoff
        .signed_duration_since(quote.timestamp.date_naive())
        .num_days()
        > 90;
    if stale {
        issues.push(issue(
            "QUOTE_STALE",
            Some(quote.id.clone()),
            Some(quote.currency.clone()),
        ));
    }
    Ok(Some(
        json!({"id":quote.id,"price":ExactDecimal::new(quote.close),"currency":quote.currency,"source":quote.data_source,"sourceTimestamp":quote.timestamp,"stale":stale}),
    ))
}

#[async_trait]
impl ReviewFactsReader for AccountingContextReader {
    async fn capture_review(&self, query: &ReviewFactsQuery) -> JournalResult<ReviewFactsCapture> {
        let zone =
            wealthfolio_core::utils::time_utils::parse_user_timezone(&query.observation_timezone)?;
        if query.observation_cutoff > wealthfolio_core::utils::time_utils::user_today(zone) {
            return Err(invalid("observation cutoff cannot be in the future"));
        }
        let _window = self.store.investment_read_window().read().await;
        for _ in 0..2 {
            let mut anchor = get_connection(self.store.pool())?;
            let epoch = super::state::read_epoch(&mut anchor)?;
            let (before, observations) = anchor
                .transaction::<_, JournalError, _>(|conn| self.review_version(conn, query))?;
            let mut issues = Vec::new();
            for observed in observations["executions"].as_array().into_iter().flatten() {
                let source = &observed["currentSource"];
                let id = source["execution"]["id"].as_str().map(str::to_string);
                if observed["observationScope"] == "UNDETERMINED" {
                    issues.push(issue("EXECUTION_DATE_UNAVAILABLE", id.clone(), None));
                }
                if source["projectionPending"] == true {
                    issues.push(issue("EXECUTION_INDEX_PENDING", id.clone(), None));
                }
                if source["currentSourceVersion"] != observed["link"]["sourceVersion"]
                    && observed["sourceAcknowledged"] != true
                {
                    issues.push(issue("LINKED_SOURCE_CHANGED", id.clone(), None));
                }
                if source["sourceState"] != "VALID"
                    || matches!(
                        source["execution"]["status"].as_str(),
                        Some("INVALID" | "DELETED" | "RECONCILE")
                    )
                {
                    issues.push(issue("EXECUTION_REQUIRES_RECONCILIATION", id, None));
                }
            }
            let subject_market = self.subject_observation(query, &observations, &mut issues)?;
            let (after, _) = {
                let mut conn = get_connection(self.store.pool())?;
                conn.transaction::<_, JournalError, _>(|conn| self.review_version(conn, query))?
            };
            if before == after && epoch == super::state::read_epoch(&mut anchor)? {
                return Ok(ReviewFactsCapture {
                    schema_version: 1,
                    query: query.clone(),
                    quality: if issues.is_empty() {
                        ContextQuality::Complete
                    } else {
                        ContextQuality::Partial
                    },
                    issues,
                    observations,
                    subject_market,
                    source_version: before,
                });
            }
        }
        Err(JournalError::StateConflict)
    }
    fn validate_review(
        &self,
        conn: &mut SqliteConnection,
        capture: &ReviewFactsCapture,
    ) -> JournalResult<()> {
        let (version, _) = self.review_version(conn, &capture.query)?;
        if version != capture.source_version {
            return Err(JournalError::StateConflict);
        }
        Ok(())
    }
}

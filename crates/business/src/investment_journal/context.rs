mod accounting;
mod flows;
mod review_facts;
mod risk;
mod state;
pub use accounting::AccountingContextReader;
use async_trait::async_trait;
use chrono::NaiveDate;
use diesel::SqliteConnection;
use serde::{Deserialize, Serialize};
use serde_json::Value;

use super::{model::invalid, DecisionBody, JournalResult};

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq, schemars::JsonSchema)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct InvestmentContextQuery {
    pub purpose_ids: Vec<String>,
    pub account_ids: Vec<String>,
}
impl InvestmentContextQuery {
    pub(crate) fn for_decision(body: &DecisionBody) -> Self {
        let mut purpose_ids: Vec<_> = body
            .purpose_ids
            .iter()
            .cloned()
            .chain(body.funding.iter().map(|f| f.purpose_id.clone()))
            .collect();
        let mut account_ids: Vec<_> = body
            .funding
            .iter()
            .map(|f| f.resource.account_id().to_string())
            .collect();
        purpose_ids.sort();
        purpose_ids.dedup();
        account_ids.sort();
        account_ids.dedup();
        Self {
            purpose_ids,
            account_ids,
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq, schemars::JsonSchema)]
#[serde(rename_all = "SCREAMING_SNAKE_CASE")]
pub enum ContextQuality {
    Complete,
    Partial,
    Unavailable,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq, schemars::JsonSchema)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct InvestmentContextIssue {
    pub code: String,
    pub source_id: Option<String>,
    pub currency: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, schemars::JsonSchema)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct InvestmentContextBlock {
    pub scope: Value,
    pub as_of_date: NaiveDate,
    pub source_data_as_of: Option<String>,
    pub currency: String,
    pub quality: ContextQuality,
    pub stale: bool,
    pub issues: Vec<InvestmentContextIssue>,
    pub data: Value,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, schemars::JsonSchema)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct InvestmentContextResponse {
    pub schema_version: u32,
    pub query: InvestmentContextQuery,
    pub as_of_date: NaiveDate,
    pub currency: String,
    pub funding: InvestmentContextBlock,
    pub recent_cash_flow: InvestmentContextBlock,
    pub household: InvestmentContextBlock,
}

#[derive(Debug, Clone, Serialize, Deserialize, schemars::JsonSchema)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct InvestmentContextCapture {
    pub schema_version: u32,
    pub response: InvestmentContextResponse,
    /// Canonical source anchor, revalidated inside the journal writer transaction.
    pub source_version: String,
}

/// A capture must remain readable if one block fails. Implementations preserve
/// verifiable children and explicitly mark missing totals and source guards.
#[async_trait]
pub trait InvestmentContextReader: Send + Sync {
    async fn capture(
        &self,
        query: &InvestmentContextQuery,
        day: NaiveDate,
    ) -> JournalResult<InvestmentContextCapture>;
    fn validate(
        &self,
        conn: &mut SqliteConnection,
        capture: &InvestmentContextCapture,
    ) -> JournalResult<()>;
}

impl InvestmentContextCapture {
    pub(crate) fn validate_envelope(
        &self,
        query: &InvestmentContextQuery,
        day: NaiveDate,
    ) -> JournalResult<()> {
        let r = &self.response;
        if self.schema_version != 1
            || r.schema_version != 1
            || &r.query != query
            || r.as_of_date != day
            || self.source_version.is_empty()
        {
            return Err(invalid(
                "context returned an inconsistent scope or source version",
            ));
        }
        super::model::validate_currency(&r.currency)?;
        for block in [&r.funding, &r.recent_cash_flow, &r.household] {
            if block.as_of_date != day || block.currency != r.currency {
                return Err(invalid(
                    "context blocks must use one business day and report currency",
                ));
            }
        }
        Ok(())
    }
    pub(crate) fn stable_hash(&self) -> JournalResult<String> {
        let mut value = serde_json::to_value(self).map_err(|e| invalid(e.to_string()))?;
        remove_refresh_times(&mut value);
        Ok(crate::requests::hash_json(&crate::requests::canonicalize(
            value,
        )))
    }
}
fn remove_refresh_times(value: &mut Value) {
    match value {
        Value::Object(map) => {
            // These describe the computation, never the underlying asset date.
            for key in ["calculatedAt", "refreshedAt", "generatedAt", "observedAt"] {
                map.remove(key);
            }
            for child in map.values_mut() {
                remove_refresh_times(child);
            }
        }
        Value::Array(values) => {
            for child in values {
                remove_refresh_times(child);
            }
        }
        _ => (),
    }
}

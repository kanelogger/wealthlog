//! Versioned decision contents. Host identity is deliberately absent from input DTOs.
use std::collections::BTreeSet;

use chrono::{DateTime, NaiveDate};
use serde::{Deserialize, Serialize};

use super::{
    model::invalid, model::nonblank, model::validate_currency, ExactDecimal, FundResource,
    JournalResult,
};

#[derive(
    Debug, Clone, Copy, Default, Serialize, Deserialize, PartialEq, Eq, schemars::JsonSchema,
)]
#[serde(rename_all = "SCREAMING_SNAKE_CASE")]
pub enum ContentOrigin {
    UserProvided,
    AssistedDraft,
    #[default]
    Unspecified,
}

#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq, schemars::JsonSchema)]
#[serde(rename_all = "SCREAMING_SNAKE_CASE")]
pub enum DecisionAction {
    Buy,
    Sell,
    Hold,
    Wait,
    Research,
    Other,
}
impl DecisionAction {
    pub fn is_trade(self) -> bool {
        matches!(self, Self::Buy | Self::Sell)
    }
}

#[derive(
    Debug, Clone, Copy, Default, Serialize, Deserialize, PartialEq, Eq, schemars::JsonSchema,
)]
#[serde(rename_all = "SCREAMING_SNAKE_CASE")]
pub enum DecisionFundingStatus {
    #[default]
    Unknown,
    Planned,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq, schemars::JsonSchema)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct DecisionOption {
    pub action: String,
    #[serde(default)]
    pub description: String,
    pub advantages: Option<String>,
    pub disadvantages: Option<String>,
    pub not_selected_reason: Option<String>,
}

#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq, schemars::JsonSchema)]
#[serde(rename_all = "SCREAMING_SNAKE_CASE")]
pub enum InformationKind {
    Fact,
    Hypothesis,
    Unknown,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq, schemars::JsonSchema)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct DecisionEvidence {
    pub information_kind: InformationKind,
    #[serde(default)]
    pub content: String,
    pub source: Option<String>,
    pub probability_lower_bps: Option<i32>,
    pub probability_upper_bps: Option<i32>,
    pub probability_basis: Option<String>,
    pub purpose_id: Option<String>,
    pub assumed_loss_amount: Option<ExactDecimal>,
    pub loss_currency: Option<String>,
    pub loss_basis: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq, schemars::JsonSchema)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct DecisionFunding {
    pub purpose_id: String,
    pub resource: FundResource,
    /// Amount for CASH, quantity for SECURITY, always in the resource's own unit.
    pub planned: ExactDecimal,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq, schemars::JsonSchema)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct DecisionBody {
    #[serde(default = "schema_one")]
    pub schema_version: u32,
    #[serde(default)]
    pub title: String,
    pub action: Option<DecisionAction>,
    pub asset_id: Option<String>,
    pub planned_amount: Option<ExactDecimal>,
    pub planned_quantity: Option<ExactDecimal>,
    pub currency: Option<String>,
    #[serde(default)]
    pub thesis: String,
    pub accepted_disadvantages: Option<String>,
    pub information_value: Option<String>,
    pub waiting_cost: Option<String>,
    pub declared_at: Option<String>,
    #[serde(default)]
    pub funding_status: DecisionFundingStatus,
    #[serde(default)]
    pub scenario_tags: Vec<String>,
    #[serde(default)]
    pub purpose_ids: Vec<String>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub principle_revision_ids: Vec<String>,
    #[serde(default)]
    pub options: Vec<DecisionOption>,
    #[serde(default)]
    pub evidence: Vec<DecisionEvidence>,
    #[serde(default)]
    pub funding: Vec<DecisionFunding>,
    #[serde(default)]
    pub content_origin: ContentOrigin,
}
fn schema_one() -> u32 {
    1
}

impl DecisionBody {
    pub(crate) fn normalize(&mut self) -> JournalResult<()> {
        if self.schema_version != 1 {
            return Err(invalid("unsupported decision schemaVersion"));
        }
        self.scenario_tags = normalize_scenario_tags(&self.scenario_tags)?;
        for purpose in &self.purpose_ids {
            nonblank(purpose, "purposeId")?;
        }
        self.purpose_ids.sort();
        self.purpose_ids.dedup();
        for id in &self.principle_revision_ids {
            nonblank(id, "principleRevisionId")?;
        }
        self.principle_revision_ids.sort();
        self.principle_revision_ids.dedup();
        if let Some(currency) = &self.currency {
            validate_currency(currency)?;
        }
        if let Some(asset) = &self.asset_id {
            nonblank(asset, "assetId")?;
        }
        for value in [self.planned_amount, self.planned_quantity]
            .into_iter()
            .flatten()
        {
            value.nonnegative()?;
        }
        if let Some(declared) = &self.declared_at {
            if DateTime::parse_from_rfc3339(declared).is_err()
                && NaiveDate::parse_from_str(declared, "%Y-%m-%d").is_err()
            {
                return Err(invalid("declaredAt must be a date or RFC3339 datetime"));
            }
        }
        for option in &self.options {
            nonblank(&option.action, "alternative action")?;
        }
        let mut scenarios = BTreeSet::new();
        for evidence in &self.evidence {
            match (
                evidence.probability_lower_bps,
                evidence.probability_upper_bps,
                &evidence.probability_basis,
            ) {
                (None, None, None) => (),
                (Some(lower), Some(upper), Some(basis))
                    if (0..=10000).contains(&lower)
                        && (lower..=10000).contains(&upper)
                        && !basis.trim().is_empty() => {}
                _ => {
                    return Err(invalid(
                        "probability requires ordered bounds in 0..10000 and a basis",
                    ))
                }
            }
            match (
                &evidence.purpose_id,
                evidence.assumed_loss_amount,
                &evidence.loss_currency,
                &evidence.loss_basis,
            ) {
                (None, None, None, None) => (),
                (Some(purpose), Some(amount), Some(currency), Some(basis))
                    if evidence.information_kind == InformationKind::Hypothesis =>
                {
                    nonblank(purpose, "loss purposeId")?;
                    amount.nonnegative()?;
                    validate_currency(currency)?;
                    nonblank(basis, "loss basis")?;
                    if !scenarios.insert(purpose) {
                        return Err(invalid("only one loss scenario per purpose is allowed"));
                    }
                    if !self.purpose_ids.contains(purpose)
                        && !self.funding.iter().any(|f| &f.purpose_id == purpose)
                    {
                        return Err(invalid(
                            "loss scenario must refer to a purpose involved in this decision",
                        ));
                    }
                }
                _ => {
                    return Err(invalid(
                        "loss scenario fields must be provided together on a hypothesis",
                    ))
                }
            }
        }
        if (self.funding_status == DecisionFundingStatus::Unknown) != self.funding.is_empty() {
            return Err(invalid(
                "unknown funding must have no planned rows; planned funding requires rows",
            ));
        }
        let mut resources = BTreeSet::new();
        for funding in &self.funding {
            nonblank(&funding.purpose_id, "purposeId")?;
            funding.resource.validate()?;
            funding.planned.nonnegative()?;
            if !resources.insert((&funding.purpose_id, &funding.resource)) {
                return Err(invalid("duplicate purpose funding resource"));
            }
        }
        Ok(())
    }

    pub(crate) fn validate_confirmation(&self) -> JournalResult<()> {
        if self.action.is_none() {
            return Err(invalid("confirmation requires an action"));
        }
        nonblank(&self.thesis, "thesis")?;
        if self.options.is_empty() {
            return Err(invalid("confirmation requires at least one alternative"));
        }
        Ok(())
    }
}

pub fn normalize_scenario_tags(tags: &[String]) -> JournalResult<Vec<String>> {
    let mut normalized = BTreeSet::new();
    for tag in tags {
        nonblank(tag, "scenario tag")?;
        normalized.insert(tag.trim().to_string());
    }
    Ok(normalized.into_iter().collect())
}

/// KEEP is the default for an ordinary save. Only an explicit SET/CANCEL changes a reminder.
#[derive(Debug, Clone, Default, Serialize, Deserialize, PartialEq, Eq, schemars::JsonSchema)]
#[serde(
    tag = "operation",
    rename_all = "SCREAMING_SNAKE_CASE",
    deny_unknown_fields
)]
pub enum CheckScheduleChange {
    #[default]
    Keep,
    Set {
        date: NaiveDate,
    },
    Cancel,
}

#[derive(Debug, Clone, Serialize, Deserialize, schemars::JsonSchema)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct SaveDecisionDraft {
    pub decision_id: Option<String>,
    pub draft_revision_id: Option<String>,
    pub expected_version: Option<i32>,
    pub body: DecisionBody,
    #[serde(default)]
    pub check_schedule: CheckScheduleChange,
    pub request_key: String,
}

#[derive(Debug, Clone, Serialize, Deserialize, schemars::JsonSchema)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct DecisionDraftTarget {
    pub decision_id: String,
    pub draft_revision_id: String,
    pub expected_version: i32,
}

#[derive(Debug, Clone, Serialize, Deserialize, schemars::JsonSchema)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct MutateDecision {
    pub decision_id: String,
    pub expected_version: i32,
    pub mutation: DecisionMutation,
    pub request_key: String,
}
#[derive(Debug, Clone, Serialize, Deserialize, schemars::JsonSchema)]
#[serde(
    tag = "operation",
    rename_all = "SCREAMING_SNAKE_CASE",
    deny_unknown_fields
)]
pub enum DecisionMutation {
    DiscardDraft {
        #[serde(rename = "draftRevisionId")]
        draft_revision_id: String,
    },
    Archive,
    Restore,
    Close {
        reason: String,
    },
    Reopen,
    SetReviewDate {
        date: NaiveDate,
    },
    CancelReview,
}

#[derive(Debug, Clone, Default, Serialize, Deserialize, schemars::JsonSchema)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct DecisionQuery {
    #[serde(default)]
    pub scenario_tags: Vec<String>,
    pub lifecycle: Option<String>,
    pub limit: Option<u32>,
    pub offset: Option<u32>,
}

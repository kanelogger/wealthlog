//! Review assessments remain separate from outcomes and alternative explanations.
use super::{
    model::{invalid, nonblank},
    normalize_scenario_tags, ContentOrigin, InformationKind, JournalResult,
};
use chrono::NaiveDate;
use serde::{Deserialize, Serialize};
use std::collections::BTreeSet;
#[derive(Debug, Clone, Serialize, Deserialize, schemars::JsonSchema)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct ReviewEvidence {
    pub information_kind: InformationKind,
    pub content: String,
    pub source: Option<String>,
}
#[derive(Debug, Clone, Copy, Serialize, Deserialize, schemars::JsonSchema)]
#[serde(rename_all = "SCREAMING_SNAKE_CASE")]
pub enum PrincipleDisposition {
    Keep,
    Revise,
    Retire,
    New,
    Undecided,
}
#[derive(Debug, Clone, Serialize, Deserialize, schemars::JsonSchema)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct ReviewPrincipleAssessment {
    pub principle_revision_id: Option<String>,
    pub disposition: PrincipleDisposition,
    pub new_principle_revision_id: Option<String>,
    pub reason: Option<String>,
}
#[derive(Debug, Clone, Serialize, Deserialize, schemars::JsonSchema)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct ReviewBody {
    #[serde(default = "schema_one")]
    pub schema_version: u32,
    #[serde(default)]
    pub judgment: String,
    #[serde(default)]
    pub execution_assessment: String,
    pub deviation_reason: Option<String>,
    #[serde(default)]
    pub outcome_evidence: Vec<ReviewEvidence>,
    pub other_explanations: Option<String>,
    pub open_questions: Option<String>,
    #[serde(default)]
    pub scenario_tags: Vec<String>,
    #[serde(default)]
    pub principles: Vec<ReviewPrincipleAssessment>,
    #[serde(default)]
    pub content_origin: ContentOrigin,
}
fn schema_one() -> u32 {
    1
}
impl ReviewBody {
    pub(crate) fn normalize(&mut self) -> JournalResult<()> {
        if self.schema_version != 1 {
            return Err(invalid("unsupported review schemaVersion"));
        }
        self.scenario_tags = normalize_scenario_tags(&self.scenario_tags)?;
        let mut assessed = BTreeSet::new();
        for item in &self.principles {
            if let Some(id) = &item.principle_revision_id {
                nonblank(id, "principleRevisionId")?;
                if !assessed.insert(id) {
                    return Err(invalid(
                        "a principle version can be assessed only once per review",
                    ));
                }
            } else if !matches!(
                item.disposition,
                PrincipleDisposition::New | PrincipleDisposition::Undecided
            ) {
                return Err(invalid(
                    "an existing-principle disposition requires its exact version",
                ));
            }
            if let Some(id) = &item.new_principle_revision_id {
                nonblank(id, "newPrincipleRevisionId")?;
            }
        }
        Ok(())
    }
    pub(crate) fn validate_submission(&self) -> JournalResult<()> {
        nonblank(&self.judgment, "judgment")?;
        nonblank(&self.execution_assessment, "executionAssessment")?;
        for evidence in &self.outcome_evidence {
            nonblank(&evidence.content, "outcome evidence content")?;
        }
        Ok(())
    }
    pub(crate) fn principle_ids(&self) -> Vec<String> {
        self.principles
            .iter()
            .flat_map(|item| {
                [&item.principle_revision_id, &item.new_principle_revision_id]
                    .into_iter()
                    .flatten()
                    .cloned()
            })
            .collect::<BTreeSet<_>>()
            .into_iter()
            .collect()
    }
}
#[derive(Debug, Clone, Serialize, Deserialize, schemars::JsonSchema)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct SaveReviewDraft {
    pub review_id: Option<String>,
    pub root_id: Option<String>,
    pub expected_version: Option<i32>,
    pub decision_revision_id: String,
    pub observation_cutoff: NaiveDate,
    pub body: ReviewBody,
    pub request_key: String,
}
#[derive(Debug, Clone, Serialize, Deserialize, schemars::JsonSchema)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct ReviewDraftTarget {
    pub review_id: String,
    pub expected_version: i32,
}
#[derive(Debug, Clone, Serialize, Deserialize, schemars::JsonSchema)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct DiscardReviewDraft {
    pub target: ReviewDraftTarget,
    pub request_key: String,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq, schemars::JsonSchema)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct ReviewFactsQuery {
    pub decision_revision_id: String,
    pub observation_cutoff: NaiveDate,
    pub observation_timezone: String,
    pub principle_revision_ids: Vec<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize, schemars::JsonSchema)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct ReviewFactsCapture {
    pub schema_version: u32,
    pub query: ReviewFactsQuery,
    pub quality: super::ContextQuality,
    pub issues: Vec<super::InvestmentContextIssue>,
    /// Original confirmed context and observed, versioned facts are separate.
    pub observations: serde_json::Value,
    pub subject_market: Option<serde_json::Value>,
    pub source_version: String,
}
impl ReviewFactsCapture {
    pub(crate) fn validate_envelope(&self, query: &ReviewFactsQuery) -> JournalResult<()> {
        if self.schema_version != 1 || &self.query != query || self.source_version.is_empty() {
            return Err(invalid(
                "review facts returned an inconsistent scope or version",
            ));
        }
        Ok(())
    }
    pub(crate) fn stable_hash(&self) -> JournalResult<String> {
        let value = serde_json::to_value(self).map_err(|e| invalid(e.to_string()))?;
        Ok(crate::requests::hash_json(&crate::requests::canonicalize(
            value,
        )))
    }
}

#[async_trait::async_trait]
pub trait ReviewFactsReader: Send + Sync {
    async fn capture_review(&self, query: &ReviewFactsQuery) -> JournalResult<ReviewFactsCapture>;
    fn validate_review(
        &self,
        conn: &mut diesel::SqliteConnection,
        capture: &ReviewFactsCapture,
    ) -> JournalResult<()>;
}

#[derive(Debug, Clone, Serialize, Deserialize, schemars::JsonSchema)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct SubmitReview {
    pub ledger_id: String,
    pub target: ReviewDraftTarget,
    pub expected_content_hash: String,
    pub expected_facts_hash: String,
    pub facts: ReviewFactsCapture,
    pub request_key: String,
}

//! Principle contents are declarations; publication authority comes from the host.
use super::{
    model::{invalid, nonblank},
    normalize_scenario_tags, ContentOrigin, JournalResult,
};
use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Serialize, Deserialize, schemars::JsonSchema)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct PrincipleBody {
    #[serde(default = "schema_one")]
    pub schema_version: u32,
    #[serde(default)]
    pub name: String,
    #[serde(default)]
    pub content: String,
    pub applicable_conditions: Option<String>,
    pub exceptions: Option<String>,
    pub revision_reason: Option<String>,
    pub source_review_id: Option<String>,
    pub source_reference: Option<String>,
    #[serde(default)]
    pub scenario_tags: Vec<String>,
    #[serde(default)]
    pub content_origin: ContentOrigin,
}
fn schema_one() -> u32 {
    1
}
impl PrincipleBody {
    pub(crate) fn normalize(&mut self) -> JournalResult<()> {
        if self.schema_version != 1 {
            return Err(invalid("unsupported principle schemaVersion"));
        }
        self.scenario_tags = normalize_scenario_tags(&self.scenario_tags)?;
        if let Some(source) = &self.source_review_id {
            nonblank(source, "sourceReviewId")?;
        }
        Ok(())
    }
    pub(crate) fn validate_publication(&self, is_amendment: bool) -> JournalResult<()> {
        nonblank(&self.name, "principle name")?;
        nonblank(&self.content, "principle content")?;
        if is_amendment {
            nonblank(
                self.revision_reason.as_deref().unwrap_or_default(),
                "revisionReason",
            )?;
        }
        Ok(())
    }
}
#[derive(Debug, Clone, Serialize, Deserialize, schemars::JsonSchema)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct SavePrincipleDraft {
    pub principle_id: Option<String>,
    pub expected_draft_revision_id: Option<String>,
    pub expected_version: Option<i32>,
    pub body: PrincipleBody,
    pub request_key: String,
}
#[derive(Debug, Clone, Serialize, Deserialize, schemars::JsonSchema)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct PrincipleDraftTarget {
    pub principle_id: String,
    pub draft_revision_id: String,
    pub expected_version: i32,
}
#[derive(Debug, Clone, Serialize, Deserialize, schemars::JsonSchema)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct PublishPrinciple {
    pub ledger_id: String,
    pub target: PrincipleDraftTarget,
    pub expected_content_hash: String,
    pub expected_source_context_hash: String,
    pub source_context: serde_json::Value,
    pub request_key: String,
}
#[derive(Debug, Clone, Serialize, Deserialize, schemars::JsonSchema)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct ArchivePrinciple {
    pub principle_id: String,
    pub expected_version: i32,
    pub is_archived: bool,
    pub request_key: String,
}
#[derive(Debug, Clone, Serialize, Deserialize, schemars::JsonSchema)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct DiscardPrincipleDraft {
    pub target: PrincipleDraftTarget,
    pub request_key: String,
}
#[derive(Debug, Clone, Default, Serialize, Deserialize, schemars::JsonSchema)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct PrincipleQuery {
    #[serde(default)]
    pub include_archived: bool,
    #[serde(default)]
    pub scenario_tags: Vec<String>,
    pub limit: Option<u32>,
    pub offset: Option<u32>,
}

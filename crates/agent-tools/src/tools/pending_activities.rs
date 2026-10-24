//! Pending-entry tools (MCP).
//!
//! `create_pending_activity` stores the Agent's extraction as a pending entry;
//! it never writes an activity. The user reviews and confirms it in the app,
//! and confirmation creates the one `POSTED` activity. Entries are attributed
//! to the PAT database id from the trusted call context, so replacing a token
//! never inherits another credential's entries.

use std::sync::Arc;

use serde::{Deserialize, Serialize};

use crate::context::ToolCallContext;
use crate::env::AgentEnvironment;
use crate::scope::AgentScope;
use crate::tool::{AgentTool, AgentToolAccess, AgentToolError, AgentToolResult};
use wealthfolio_business::pending::{PendingActivity, PendingCreate};

const CREATE_PENDING_DESCRIPTION: &str = "Store an extracted income/expense entry for the user to \
    review and confirm. Use this for chat or SMS messages that describe money in or out. Anything \
    you cannot determine stays absent: never guess the account, currency, date or amount. This \
    tool does NOT create a posted activity — the user confirms the pending entry in the app first. \
    Pass source with the channel, upstream message id and the original excerpt or reference so \
    repeated deliveries can be recognized.";

const LIST_PENDING_DESCRIPTION: &str = "List pending income/expense entries waiting for review, \
    newest first. Include resolved=true to also see committed and rejected entries. Read-only.";

#[derive(Debug, Clone, Default, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct CreatePendingArgs {
    pub activity_type: String,
    #[serde(default)]
    pub account_id: Option<String>,
    #[serde(default)]
    pub activity_date: Option<String>,
    #[serde(default)]
    pub currency: Option<String>,
    /// Decimal string; never a float.
    #[serde(default)]
    pub amount: Option<String>,
    #[serde(default)]
    pub fee: Option<String>,
    #[serde(default)]
    pub tax: Option<String>,
    #[serde(default)]
    pub notes: Option<String>,
    #[serde(default)]
    pub asset_symbol: Option<String>,
    /// `{ "channel", "messageId", "entryId", "sourceAccount", "sourceSession", "excerpt" }`.
    #[serde(default)]
    pub source: Option<serde_json::Value>,
    /// Frozen per create; retrying the same request returns the stored entry.
    pub request_key: String,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct CreatePendingOutput {
    pub pending: PendingActivity,
}

#[derive(Debug, Clone, Default, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct ListPendingArgs {
    #[serde(default)]
    pub include_resolved: bool,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ListPendingOutput {
    pub pending: Vec<PendingActivity>,
}

/// Store one pending entry, attributed to the calling credential.
pub struct CreatePendingActivity;

#[async_trait::async_trait]
impl AgentTool for CreatePendingActivity {
    fn name(&self) -> &'static str {
        "create_pending_activity"
    }

    fn description(&self) -> &'static str {
        CREATE_PENDING_DESCRIPTION
    }

    fn input_schema(&self) -> serde_json::Value {
        serde_json::json!({
            "type": "object",
            "properties": {
                "activityType": {
                    "type": "string",
                    "enum": ["DEPOSIT", "WITHDRAWAL"],
                    "description": "Direction of the money movement."
                },
                "accountId": { "type": ["string", "null"], "description": "Account id when known." },
                "activityDate": { "type": ["string", "null"], "description": "Calendar date YYYY-MM-DD when known; never invent a time or date." },
                "currency": { "type": ["string", "null"], "description": "ISO currency when known." },
                "amount": { "type": ["string", "null"], "description": "Decimal string, positive magnitude." },
                "fee": { "type": ["string", "null"] },
                "tax": { "type": ["string", "null"] },
                "notes": { "type": ["string", "null"] },
                "assetSymbol": { "type": ["string", "null"] },
                "source": {
                    "type": ["object", "null"],
                    "description": "Full source identity: channel, message id, entry id, source account/session and original excerpt or reference.",
                    "properties": {
                        "channel": { "type": "string" },
                        "messageId": { "type": "string" },
                        "entryId": { "type": "string" },
                        "sourceAccount": { "type": "string" },
                        "sourceSession": { "type": "string" },
                        "excerpt": { "type": "string" },
                        "reference": { "type": "string" }
                    }
                },
                "requestKey": { "type": "string", "description": "Frozen per submit; reused on retry." }
            },
            "required": ["activityType", "requestKey"]
        })
    }

    fn required_scopes(&self) -> &'static [AgentScope] {
        &[AgentScope::ActivitiesDraft]
    }

    fn access_level(&self) -> AgentToolAccess {
        AgentToolAccess::Draft
    }

    fn sanitize_args_for_audit(&self, args: &serde_json::Value) -> serde_json::Value {
        // Message text and amounts are financial data; keep only shape facts.
        let keys: Vec<&str> = args
            .as_object()
            .map(|map| map.keys().map(String::as_str).collect())
            .unwrap_or_default();
        serde_json::json!({ "fields": keys })
    }

    async fn call(
        &self,
        _env: Arc<dyn AgentEnvironment>,
        _args: serde_json::Value,
    ) -> Result<AgentToolResult, AgentToolError> {
        Err(AgentToolError::ExecutionFailed(
            "pending entries need the trusted call context".to_string(),
        ))
    }

    async fn call_with_context(
        &self,
        env: Arc<dyn AgentEnvironment>,
        context: &ToolCallContext,
        args: serde_json::Value,
    ) -> Result<AgentToolResult, AgentToolError> {
        let args: CreatePendingArgs = serde_json::from_value(args)?;
        let service = env.pending_service().ok_or_else(|| {
            AgentToolError::ExecutionFailed(
                "pending entries are not available on this host".to_string(),
            )
        })?;
        // The creator is the PAT database id, never the fingerprint and never a
        // caller-supplied field.
        let created_by = context
            .credential_id
            .clone()
            .unwrap_or_else(|| context.actor.clone());
        let pending = service
            .create(
                PendingCreate {
                    activity_type: args.activity_type,
                    account_id: args.account_id,
                    activity_date: args.activity_date,
                    currency: args.currency,
                    amount: args.amount,
                    fee: args.fee,
                    tax: args.tax,
                    notes: args.notes,
                    asset_symbol: args.asset_symbol,
                    source: args.source,
                    request_key: args.request_key,
                },
                &created_by,
                context.channel,
                context.authenticated,
            )
            .await?;
        Ok(AgentToolResult {
            content: serde_json::to_value(CreatePendingOutput { pending })?,
        })
    }
}

/// List pending entries.
pub struct ListPendingActivities;

#[async_trait::async_trait]
impl AgentTool for ListPendingActivities {
    fn name(&self) -> &'static str {
        "list_pending_activities"
    }

    fn description(&self) -> &'static str {
        LIST_PENDING_DESCRIPTION
    }

    fn input_schema(&self) -> serde_json::Value {
        serde_json::json!({
            "type": "object",
            "properties": {
                "includeResolved": {
                    "type": "boolean",
                    "description": "Also return committed and rejected entries (default false)."
                }
            }
        })
    }

    fn required_scopes(&self) -> &'static [AgentScope] {
        &[AgentScope::ActivitiesRead]
    }

    fn access_level(&self) -> AgentToolAccess {
        AgentToolAccess::Read
    }

    async fn call(
        &self,
        env: Arc<dyn AgentEnvironment>,
        args: serde_json::Value,
    ) -> Result<AgentToolResult, AgentToolError> {
        let args: ListPendingArgs = serde_json::from_value(args)?;
        let service = env.pending_service().ok_or_else(|| {
            AgentToolError::ExecutionFailed(
                "pending entries are not available on this host".to_string(),
            )
        })?;
        let pending = service
            .list(args.include_resolved)
            .map_err(|error| AgentToolError::ExecutionFailed(error.to_string()))?;
        Ok(AgentToolResult {
            content: serde_json::to_value(ListPendingOutput { pending })?,
        })
    }
}

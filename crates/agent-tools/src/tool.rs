//! The object-safe `AgentTool` trait and its supporting types.

use std::sync::Arc;

use crate::context::ToolCallContext;
use crate::env::AgentEnvironment;
use crate::scope::AgentScope;

/// What a tool does to user data. Drives default enablement and UI badges.
#[derive(Debug, Clone, Copy, PartialEq, Eq, serde::Serialize)]
#[serde(rename_all = "snake_case")]
pub enum AgentToolAccess {
    /// Reads data only.
    Read,
    /// Prepares a draft that requires an explicit commit.
    Draft,
    /// Mutates data.
    Write,
    /// Returns proposals/suggestions without mutating data.
    Suggest,
}

/// Successful tool result: a JSON value the caller serializes for its
/// transport (rig tool output, MCP `CallToolResult`, ...).
#[derive(Debug, Clone)]
pub struct AgentToolResult {
    pub content: serde_json::Value,
}

/// Tool execution errors.
///
/// Display strings are part of the migration contract: they match the
/// error text the assistant produced before the agent-tools extraction
/// (`AiError::ToolExecutionFailed` formatted as "Tool execution failed: ..."
/// and serde failures as "JsonError: ..."), so model-visible and persisted
/// error messages are unchanged.
#[derive(Debug, thiserror::Error)]
pub enum AgentToolError {
    #[error("JsonError: {0}")]
    InvalidArgs(#[from] serde_json::Error),
    /// Invalid input or request. Displays as the bare message — same text
    /// as the former `AiError::InvalidInput`.
    #[error("{0}")]
    InvalidInput(String),
    #[error("Tool execution failed: {0}")]
    ExecutionFailed(String),
    #[error("Tool not found: {0}")]
    NotFound(String),
    #[error("Scope denied: tool '{tool}' requires {missing}")]
    ScopeDenied { tool: String, missing: String },
    /// A create matched an existing dedupe identity. Typed so transports can
    /// answer with `duplicate_activity` instead of parsing text.
    #[error("{message}")]
    DuplicateActivity {
        message: String,
        existing_activity_id: Option<String>,
    },
    /// Any other business-layer rejection, carrying its stable contract.
    #[error("{message}")]
    Business {
        code: String,
        message: String,
        retryable: bool,
        request_id: Option<String>,
        data: serde_json::Value,
    },
}

impl AgentToolError {
    /// Stable structured body for MCP `structuredContent` and CLI output.
    pub fn structured_value(&self) -> serde_json::Value {
        match self {
            AgentToolError::DuplicateActivity {
                message,
                existing_activity_id,
            } => serde_json::json!({
                "code": "duplicate_activity",
                "message": message,
                "retryable": false,
                "requestId": serde_json::Value::Null,
                "data": { "existingActivityId": existing_activity_id },
            }),
            AgentToolError::Business {
                code,
                message,
                retryable,
                request_id,
                data,
            } => serde_json::json!({
                "code": code,
                "message": message,
                "retryable": retryable,
                "requestId": request_id,
                "data": data,
            }),
            AgentToolError::ScopeDenied { tool, missing } => serde_json::json!({
                "code": "scope_denied",
                "message": self.to_string(),
                "retryable": false,
                "requestId": serde_json::Value::Null,
                "data": { "tool": tool, "missing": missing },
            }),
            AgentToolError::NotFound(name) => serde_json::json!({
                "code": "tool_not_found",
                "message": self.to_string(),
                "retryable": false,
                "requestId": serde_json::Value::Null,
                "data": { "tool": name },
            }),
            AgentToolError::InvalidArgs(error) => serde_json::json!({
                "code": "invalid_input",
                "message": error.to_string(),
                "retryable": false,
                "requestId": serde_json::Value::Null,
                "data": serde_json::Value::Null,
            }),
            AgentToolError::InvalidInput(message) => serde_json::json!({
                "code": "invalid_input",
                "message": message,
                "retryable": false,
                "requestId": serde_json::Value::Null,
                "data": serde_json::Value::Null,
            }),
            AgentToolError::ExecutionFailed(message) => serde_json::json!({
                "code": "execution_failed",
                "message": message,
                "retryable": false,
                "requestId": serde_json::Value::Null,
                "data": serde_json::Value::Null,
            }),
        }
    }
}

impl From<wealthfolio_business::OrdinaryActivityError> for AgentToolError {
    fn from(error: wealthfolio_business::OrdinaryActivityError) -> Self {
        match &error {
            wealthfolio_business::OrdinaryActivityError::DuplicateActivity {
                existing_activity_id,
            } => AgentToolError::DuplicateActivity {
                message: error.to_string(),
                existing_activity_id: existing_activity_id.clone(),
            },
            _ => AgentToolError::Business {
                code: error.code().to_string(),
                // Preserve the pre-existing MCP text convention: the stable
                // code prefixes the human message.
                message: format!("{}: {error}", error.code()),
                retryable: error.retryable(),
                request_id: None,
                data: error
                    .structured_value()
                    .get("data")
                    .cloned()
                    .unwrap_or(serde_json::Value::Null),
            },
        }
    }
}

/// An agent-callable tool. Object-safe: implementations are stateless unit
/// structs; the environment arrives per call so one catalog instance can
/// serve every session and runtime.
#[async_trait::async_trait]
pub trait AgentTool: Send + Sync {
    /// Stable tool identifier (e.g. `"get_accounts"`). Treated as a public
    /// contract: chat threads snapshot allowlists and MCP clients pin
    /// configs by this name. Renaming is a migration, not an edit.
    fn name(&self) -> &'static str;

    /// Human/model-facing description.
    fn description(&self) -> &'static str;

    /// JSON Schema for the tool arguments (same shape the LLM sees).
    fn input_schema(&self) -> serde_json::Value;

    /// Scopes a caller must hold for this tool to be visible and callable.
    fn required_scopes(&self) -> &'static [AgentScope];

    /// Access classification (read/draft/write/suggest).
    fn access_level(&self) -> AgentToolAccess;

    /// Redact `args` for audit logging. Default is identity; tools whose
    /// arguments can carry bulk user data (e.g. CSV content) override this.
    fn sanitize_args_for_audit(&self, args: &serde_json::Value) -> serde_json::Value {
        args.clone()
    }

    /// Execute the tool against the environment.
    async fn call(
        &self,
        env: Arc<dyn AgentEnvironment>,
        args: serde_json::Value,
    ) -> Result<AgentToolResult, AgentToolError>;

    /// Execute with the trusted caller context.
    ///
    /// Tools that record who called them (pending entries, business audit)
    /// override this and use `context`; stateless tools keep the default and
    /// simply delegate. The catalog always routes through this method, so a
    /// tool can never mistake client-supplied fields for identity.
    async fn call_with_context(
        &self,
        env: Arc<dyn AgentEnvironment>,
        _context: &ToolCallContext,
        args: serde_json::Value,
    ) -> Result<AgentToolResult, AgentToolError> {
        self.call(env, args).await
    }
}

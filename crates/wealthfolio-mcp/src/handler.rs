//! Manual `ServerHandler` implementation over the agent-tools catalog.
//!
//! Tools are registered at runtime from `AgentToolCatalog` (no proc
//! macros). Authorization is read per-request from the
//! [`crate::auth::McpAuthContext`] that host middleware injected into the
//! HTTP request extensions (rmcp forwards `http::request::Parts` into the
//! request context). Missing auth context fails closed.

use std::sync::Arc;

use rmcp::handler::server::ServerHandler;
use rmcp::model::{
    CallToolRequestParams, CallToolResult, ContentBlock, ErrorData as McpError, Implementation,
    ListToolsResult, PaginatedRequestParams, ServerCapabilities, ServerInfo, Tool,
};
use rmcp::service::{RequestContext, RoleServer};
use wealthfolio_agent_tools::{
    AgentEnvironment, AgentToolCatalog, AgentToolError, CallActorKind, McpHost, ToolCallContext,
};
use wealthfolio_business::audit::BusinessChannel;

use crate::audit::{AuditOutcome, AuditSink, McpAuditEntry};
use crate::auth::McpAuthContext;
use crate::runtime::McpCallRuntime;

/// MCP server handler backed by the shared agent-tool catalog.
///
/// Cheap to clone: the Streamable HTTP transport constructs one handler
/// per session via the service factory.
#[derive(Clone)]
pub struct WealthfolioMcpHandler {
    env: Option<Arc<dyn AgentEnvironment>>,
    catalog: Arc<AgentToolCatalog>,
    audit: Option<Arc<dyn AuditSink>>,
    instructions: Option<String>,
    host: McpHost,
    channel: BusinessChannel,
    ledger_id: Option<String>,
    /// Last epoch seen per MCP session id. A lock bumps the epoch and thus
    /// revokes every session registered under it.
    sessions: Arc<std::sync::Mutex<std::collections::HashMap<String, u64>>>,
}

impl WealthfolioMcpHandler {
    pub fn new(
        env: Arc<dyn AgentEnvironment>,
        catalog: Arc<AgentToolCatalog>,
        audit: Option<Arc<dyn AuditSink>>,
        instructions: Option<String>,
        host: McpHost,
        channel: BusinessChannel,
        ledger_id: Option<String>,
    ) -> Self {
        Self {
            env: Some(env),
            catalog,
            audit,
            instructions,
            host,
            channel,
            ledger_id,
            sessions: Arc::new(std::sync::Mutex::new(std::collections::HashMap::new())),
        }
    }

    /// A handler with no fixed environment: every call must arrive with a
    /// host-supplied [`McpCallRuntime`] lease.
    pub fn new_dynamic(
        catalog: Arc<AgentToolCatalog>,
        audit: Option<Arc<dyn AuditSink>>,
        instructions: Option<String>,
        host: McpHost,
        channel: BusinessChannel,
        ledger_id: Option<String>,
    ) -> Self {
        Self {
            env: None,
            catalog,
            audit,
            instructions,
            host,
            channel,
            ledger_id,
            sessions: Arc::new(std::sync::Mutex::new(std::collections::HashMap::new())),
        }
    }

    /// The ledger this listener is serving, when the host pins one.
    pub fn ledger_id(&self) -> Option<&str> {
        self.ledger_id.as_deref()
    }

    /// The runtime lease the host middleware attached to this request.
    fn call_runtime(ctx: &RequestContext<RoleServer>) -> Option<McpCallRuntime> {
        ctx.extensions
            .get::<http::request::Parts>()
            .and_then(|parts| parts.extensions.get::<McpCallRuntime>())
            .cloned()
    }

    fn profile_header(ctx: &RequestContext<RoleServer>) -> Option<String> {
        ctx.extensions
            .get::<http::request::Parts>()
            .and_then(|parts| parts.headers.get("x-wf-profile-id"))
            .and_then(|value| value.to_str().ok())
            .map(str::trim)
            .filter(|value| !value.is_empty())
            .map(str::to_string)
    }

    /// A present header must name this listener's ledger. Formal writes must
    /// always name it — the host decides which ledger the call mutates.
    fn check_profile(
        &self,
        ctx: &RequestContext<RoleServer>,
        requires: bool,
        ledger_id: Option<&str>,
    ) -> Result<(), McpError> {
        let Some(expected) = ledger_id else {
            return Ok(());
        };
        match Self::profile_header(ctx) {
            Some(profile) if profile == expected => Ok(()),
            Some(profile) => Err(McpError::invalid_request(
                "X-WF-Profile-Id does not match this listener's ledger",
                Some(serde_json::json!({
                    "code": "profile_mismatch",
                    "expectedLedgerId": expected,
                    "receivedLedgerId": profile,
                })),
            )),
            None if requires => Err(McpError::invalid_request(
                "X-WF-Profile-Id is required for formal writes",
                Some(serde_json::json!({
                    "code": "profile_required",
                    "expectedLedgerId": expected,
                })),
            )),
            None => Ok(()),
        }
    }

    /// Revoke sessions created under an earlier epoch. A newly initialized
    /// session has no entry and is admitted under the current epoch.
    fn check_session(
        &self,
        ctx: &RequestContext<RoleServer>,
        runtime: Option<&McpCallRuntime>,
    ) -> Result<(), McpError> {
        let Some(runtime) = runtime else {
            return Ok(());
        };
        let session_id = Self::session_id(ctx);
        if session_id == "stateless" {
            return Ok(());
        }
        let mut sessions = self
            .sessions
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner());
        match sessions.get(&session_id) {
            Some(epoch) if *epoch != runtime.session_epoch => Err(McpError::invalid_request(
                "the MCP session was revoked when the profile locked; initialize again",
                Some(serde_json::json!({ "code": "session_revoked" })),
            )),
            _ => {
                sessions.insert(session_id, runtime.session_epoch);
                Ok(())
            }
        }
    }

    /// Pull the auth context the host middleware injected; fail closed.
    fn auth_context(ctx: &RequestContext<RoleServer>) -> Result<McpAuthContext, McpError> {
        ctx.extensions
            .get::<http::request::Parts>()
            .and_then(|parts| parts.extensions.get::<McpAuthContext>())
            .cloned()
            .ok_or_else(|| {
                McpError::invalid_request(
                    "missing authentication context; the MCP endpoint must be served behind \
                     Wealthfolio's auth middleware",
                    None,
                )
            })
    }

    /// MCP session id for audit grouping (`Mcp-Session-Id` header).
    fn session_id(ctx: &RequestContext<RoleServer>) -> String {
        ctx.extensions
            .get::<http::request::Parts>()
            .and_then(|parts| parts.headers.get("mcp-session-id"))
            .and_then(|value| value.to_str().ok())
            .unwrap_or("stateless")
            .to_string()
    }

    /// Persist an audit entry, awaited before the tool result returns so the
    /// row is durable — a mutation must never commit without its audit trail
    /// (e.g. if the process exits right after the call).
    async fn audit(&self, runtime: Option<&McpCallRuntime>, entry: McpAuditEntry) {
        let sink = runtime
            .and_then(|runtime| runtime.audit.clone())
            .or_else(|| self.audit.clone());
        if let Some(sink) = sink {
            sink.record(entry).await;
        }
    }

    fn audit_entry(
        auth: &McpAuthContext,
        session_id: String,
        tool: &str,
        args_summary: serde_json::Value,
        outcome: AuditOutcome,
        error_message: Option<String>,
    ) -> McpAuditEntry {
        McpAuditEntry {
            session_id,
            actor_kind: auth.actor_kind,
            actor_fingerprint: auth.actor_fingerprint.clone(),
            tool: tool.to_string(),
            scopes: auth
                .granted_scopes
                .iter()
                .map(|s| s.as_str().to_string())
                .collect(),
            args_summary,
            outcome,
            error_message,
            business_request_id: None,
        }
    }

    fn to_mcp_tool(tool: &Arc<dyn wealthfolio_agent_tools::AgentTool>) -> Tool {
        let schema = match tool.input_schema() {
            serde_json::Value::Object(map) => map,
            _ => serde_json::Map::new(),
        };
        Tool::new(tool.name(), tool.description(), Arc::new(schema))
    }
}

impl ServerHandler for WealthfolioMcpHandler {
    fn get_info(&self) -> ServerInfo {
        let mut server_info = Implementation::default();
        server_info.name = "wealthlog".into();
        server_info.version = env!("CARGO_PKG_VERSION").into();

        let mut info = ServerInfo::default();
        info.capabilities = ServerCapabilities::builder().enable_tools().build();
        info.server_info = server_info;
        info.instructions = self.instructions.clone();
        info
    }

    async fn list_tools(
        &self,
        _request: Option<PaginatedRequestParams>,
        ctx: RequestContext<RoleServer>,
    ) -> Result<ListToolsResult, McpError> {
        let auth = Self::auth_context(&ctx)?;
        self.check_session(&ctx, Self::call_runtime(&ctx).as_ref())?;
        let tools = self
            .catalog
            .iter()
            .filter(|tool| {
                !wealthfolio_agent_tools::mcp_write_blocked_for(tool.as_ref(), self.host)
            })
            .filter(|tool| auth.granted_scopes.grants_all(tool.required_scopes()))
            .map(Self::to_mcp_tool)
            .collect();
        Ok(ListToolsResult {
            tools,
            ..ListToolsResult::default()
        })
    }

    fn get_tool(&self, name: &str) -> Option<Tool> {
        self.catalog
            .get(name)
            .filter(|tool| {
                !wealthfolio_agent_tools::mcp_write_blocked_for(tool.as_ref(), self.host)
            })
            .map(Self::to_mcp_tool)
    }

    async fn call_tool(
        &self,
        request: CallToolRequestParams,
        ctx: RequestContext<RoleServer>,
    ) -> Result<CallToolResult, McpError> {
        let auth = Self::auth_context(&ctx)?;
        let call_runtime = Self::call_runtime(&ctx);
        self.check_session(&ctx, call_runtime.as_ref())?;
        let session_id = Self::session_id(&ctx);
        let name = request.name.as_ref();

        let Some(tool) = self.catalog.get(name) else {
            // Audit name-probing too: unknown-tool calls must leave a trace.
            let message = format!("unknown tool: {name}");
            self.audit(
                call_runtime.as_ref(),
                Self::audit_entry(
                    &auth,
                    session_id,
                    name,
                    serde_json::Value::Null,
                    AuditOutcome::Error,
                    Some(message.clone()),
                ),
            )
            .await;
            return Err(McpError::invalid_params(
                message,
                Some(serde_json::json!({ "code": "tool_not_found" })),
            ));
        };

        // Execution-entry interception: even a handler constructed with an
        // unfiltered catalog must never run a formal-write or hidden tool.
        if wealthfolio_agent_tools::mcp_write_blocked_for(tool.as_ref(), self.host) {
            let message = format!(
                "tool '{name}' is unavailable: this host does not permit that formal write"
            );
            self.audit(
                call_runtime.as_ref(),
                Self::audit_entry(
                    &auth,
                    session_id,
                    name,
                    serde_json::Value::Null,
                    AuditOutcome::Denied,
                    Some(message.clone()),
                ),
            )
            .await;
            let mut result = CallToolResult::error(vec![ContentBlock::text(message.clone())]);
            result.structured_content = Some(serde_json::json!({
                "code": "write_blocked",
                "message": message,
                "retryable": false,
                "requestId": serde_json::Value::Null,
                "data": { "tool": name },
            }));
            return Ok(result);
        }

        let effective_ledger = call_runtime
            .as_ref()
            .map(|runtime| runtime.ledger_id.as_str())
            .or(self.ledger_id.as_deref());
        let requires_profile = matches!(
            tool.access_level(),
            wealthfolio_agent_tools::AgentToolAccess::Write
        );
        self.check_profile(&ctx, requires_profile, effective_ledger)?;

        let args = request
            .arguments
            .map(serde_json::Value::Object)
            .unwrap_or_else(|| serde_json::json!({}));
        let args_summary = tool.sanitize_args_for_audit(&args);

        // Trusted context: built only from middleware-injected auth facts and
        // rmcp's own request metadata, never from tool arguments.
        let context = ToolCallContext {
            actor: auth.actor_fingerprint.clone(),
            credential_id: auth.credential_id.clone(),
            actor_kind: CallActorKind::Pat,
            authenticated: true,
            session_id: Some(session_id.clone()),
            request_id: Some(ctx.id.to_string()),
            channel: self.channel,
            scopes: auth.granted_scopes.clone(),
        };

        let env = call_runtime
            .as_ref()
            .map(|runtime| runtime.env.clone())
            .or_else(|| self.env.clone());
        let Some(env) = env else {
            let message = "no ledger runtime is attached to this request".to_string();
            return Err(McpError::internal_error(
                message,
                Some(serde_json::json!({ "code": "profile_locked" })),
            ));
        };
        let result = self
            .catalog
            .execute_with_context(env, &context, name, args)
            .await;

        match result {
            Ok(output) => {
                let mut entry = Self::audit_entry(
                    &auth,
                    session_id,
                    name,
                    args_summary,
                    AuditOutcome::Success,
                    None,
                );
                entry.business_request_id = output
                    .content
                    .get("requestId")
                    .or_else(|| {
                        output
                            .content
                            .get("result")
                            .and_then(|result| result.get("requestId"))
                    })
                    .or_else(|| {
                        output
                            .content
                            .get("pending")
                            .and_then(|pending| pending.get("requestId"))
                    })
                    .and_then(serde_json::Value::as_str)
                    .map(str::to_owned);
                self.audit(call_runtime.as_ref(), entry).await;
                let text =
                    serde_json::to_string(&output.content).unwrap_or_else(|_| "{}".to_string());
                let mut result = CallToolResult::success(vec![ContentBlock::text(text)]);
                result.structured_content = Some(output.content);
                Ok(result)
            }
            Err(err @ AgentToolError::ScopeDenied { .. }) => {
                let message = err.to_string();
                self.audit(
                    call_runtime.as_ref(),
                    Self::audit_entry(
                        &auth,
                        session_id,
                        name,
                        args_summary,
                        AuditOutcome::Denied,
                        Some(message.clone()),
                    ),
                )
                .await;
                let mut result = CallToolResult::error(vec![ContentBlock::text(message)]);
                result.structured_content = Some(err.structured_value());
                Ok(result)
            }
            Err(err) => {
                let message = err.to_string();
                let mut entry = Self::audit_entry(
                    &auth,
                    session_id,
                    name,
                    args_summary,
                    AuditOutcome::Error,
                    Some(message.clone()),
                );
                if let AgentToolError::Business { request_id, .. } = &err {
                    entry.business_request_id = request_id.clone();
                }
                self.audit(call_runtime.as_ref(), entry).await;
                let mut result = CallToolResult::error(vec![ContentBlock::text(message)]);
                result.structured_content = Some(err.structured_value());
                Ok(result)
            }
        }
    }
}

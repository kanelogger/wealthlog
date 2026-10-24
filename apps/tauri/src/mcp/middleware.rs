//! HTTP middleware guarding the embedded MCP endpoint.
//!
//! Origin first, then the ledger gate, then bearer authentication. The gate is
//! consulted before the token on purpose: a locked ledger answers
//! `profile_locked` without revealing or checking any credential. On success a
//! request lease and the resolved runtime are attached; both are dropped with
//! the request, which releases profile shutdown.

use std::collections::HashMap;
use std::sync::{Arc, Mutex};

use axum::body::Body;
use axum::extract::{Request, State};
use axum::http::{header, StatusCode};
use axum::middleware::Next;
use axum::response::{IntoResponse, Response};
use axum::Json;
use wealthfolio_agent_tools::AgentScopeSet;
use wealthfolio_mcp::pat;
use wealthfolio_mcp::runtime::{McpAvailability, McpLease, McpRuntimeProvider};
use wealthfolio_mcp::{ActorKind, McpAuthContext};

#[derive(Clone)]
pub struct McpRouterState {
    pub provider: Arc<dyn McpRuntimeProvider>,
    /// Session id to the epoch it was created under. A lock bumps the epoch;
    /// sessions from an older epoch are revoked even after the listener stays up.
    pub sessions: Arc<Mutex<HashMap<String, u64>>>,
}

fn gate_error(availability: McpAvailability) -> Response {
    match availability {
        McpAvailability::Locked => (
            StatusCode::LOCKED,
            Json(serde_json::json!({
                "code": "profile_locked",
                "message": "The profile is locked. Unlock it in the app before calling the service.",
                "retryable": false,
                "requestId": serde_json::Value::Null,
                "authorization": "not_checked",
                "data": serde_json::Value::Null,
            })),
        )
            .into_response(),
        McpAvailability::Unavailable => (
            StatusCode::SERVICE_UNAVAILABLE,
            Json(serde_json::json!({
                "code": "service_unavailable",
                "message": "The local service is not available. Open the app and check whether the profile is unlocked and the service is running.",
                "retryable": true,
                "requestId": serde_json::Value::Null,
                "data": serde_json::Value::Null,
            })),
        )
            .into_response(),
    }
}

/// Resolves the current ledger, then authenticates the bearer token against
/// that ledger's PAT repository. The lease is registered before the token
/// lookup, so a shutdown cannot close the repository mid-check.
pub async fn require_runtime_pat(
    State(state): State<McpRouterState>,
    mut req: Request<Body>,
    next: Next,
) -> Response {
    let lease = match state.provider.acquire() {
        Ok(lease) => lease,
        Err(availability) => return gate_error(availability),
    };
    let runtime = lease.runtime().clone();

    let presented_session = req
        .headers()
        .get("mcp-session-id")
        .and_then(|value| value.to_str().ok())
        .map(str::to_string);
    let provided = req
        .headers()
        .get(header::AUTHORIZATION)
        .and_then(|value| value.to_str().ok())
        .and_then(|value| value.strip_prefix("Bearer "));

    let Some(token) = provided else {
        return StatusCode::UNAUTHORIZED.into_response();
    };

    if let Some(authenticated) = pat::authenticate(&runtime.pat_repository, token).await {
        if let Some(session_id) = &presented_session {
            let sessions = state
                .sessions
                .lock()
                .unwrap_or_else(|poisoned| poisoned.into_inner());
            match sessions.get(session_id) {
                Some(epoch) if *epoch != runtime.session_epoch => {
                    return (
                        StatusCode::LOCKED,
                        Json(serde_json::json!({
                            "code": "session_revoked",
                            "message": "The MCP session was revoked when the profile locked; initialize again.",
                            "retryable": false,
                            "requestId": serde_json::Value::Null,
                            "data": serde_json::Value::Null,
                        })),
                    )
                        .into_response();
                }
                Some(_) => {}
                None => {
                    return (
                        StatusCode::LOCKED,
                        Json(serde_json::json!({
                            "code": "session_revoked",
                            "message": "The MCP session is not known to this listener; initialize again.",
                            "retryable": false,
                            "requestId": serde_json::Value::Null,
                            "data": serde_json::Value::Null,
                        })),
                    )
                        .into_response();
                }
            }
        }
        req.extensions_mut().insert(McpAuthContext {
            actor_kind: ActorKind::Pat,
            actor_fingerprint: authenticated.fingerprint,
            granted_scopes: AgentScopeSet::from_strs(
                authenticated.scopes.iter().map(String::as_str),
            ),
            credential_id: Some(authenticated.id),
        });
        let epoch = runtime.session_epoch;
        req.extensions_mut().insert(runtime);
        req.extensions_mut()
            .insert(Arc::from(lease) as Arc<dyn McpLease>);
        let response = next.run(req).await;
        // A newly initialized session is published by the server on the
        // response; bind it to the epoch under which it was created.
        if presented_session.is_none() {
            if let Some(session_id) = response
                .headers()
                .get("mcp-session-id")
                .and_then(|value| value.to_str().ok())
            {
                let mut sessions = state
                    .sessions
                    .lock()
                    .unwrap_or_else(|poisoned| poisoned.into_inner());
                // Bound the map: sessions are also bounded by the rmcp
                // manager, but an authenticated caller can initialize many.
                if sessions.len() >= 4096 {
                    if let Some(oldest) = sessions.keys().next().cloned() {
                        sessions.remove(&oldest);
                    }
                }
                sessions.insert(session_id.to_string(), epoch);
            }
        }
        return response;
    }

    StatusCode::UNAUTHORIZED.into_response()
}

/// Allows requests with no `Origin` header or exactly `Origin: null`
/// (non-browser MCP clients); everything else is rejected. No configured
/// allowlist in v1.
pub async fn validate_origin(req: Request<Body>, next: Next) -> Response {
    match req.headers().get(header::ORIGIN) {
        None => next.run(req).await,
        Some(value) if value.as_bytes() == b"null" => next.run(req).await,
        Some(_) => StatusCode::FORBIDDEN.into_response(),
    }
}

//! Trusted call context for agent tools.
//!
//! Hosts produce this from their authenticated transport (PAT middleware,
//! desktop session), and the catalog passes it to tools that need to record
//! who called them. It is never built from client-supplied fields.

use wealthfolio_business::audit::BusinessChannel;

use crate::scope::AgentScopeSet;

/// How the caller was authenticated.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum CallActorKind {
    /// Personal Access Token (MCP).
    Pat,
    /// Anonymous REST/session caller: no verified identity.
    Anonymous,
    /// Local GUI session (desktop window or web user session).
    Session,
}

impl CallActorKind {
    pub fn as_str(self) -> &'static str {
        match self {
            CallActorKind::Pat => "pat",
            CallActorKind::Anonymous => "anonymous",
            CallActorKind::Session => "session",
        }
    }
}

/// The trusted facts about one tool call.
#[derive(Debug, Clone)]
pub struct ToolCallContext {
    /// Stable caller id: token row id/session id, never a raw credential.
    pub actor: String,
    /// The credential that created pending entries; for PATs this is the PAT
    /// database id, so rotating a token does not inherit entries.
    pub credential_id: Option<String>,
    pub actor_kind: CallActorKind,
    /// True only when middleware authenticated the actor.
    pub authenticated: bool,
    /// MCP session id, when the transport has one.
    pub session_id: Option<String>,
    /// MCP/HTTP request id, when the transport has one.
    pub request_id: Option<String>,
    /// Which host surface accepted the call. Set by the transport, never by
    /// tool arguments. It is recorded in the business audit and does not
    /// participate in authorization.
    pub channel: BusinessChannel,
    pub scopes: AgentScopeSet,
}

impl ToolCallContext {
    pub fn pat(
        actor: impl Into<String>,
        credential_id: Option<String>,
        scopes: AgentScopeSet,
    ) -> Self {
        Self {
            actor: actor.into(),
            credential_id,
            actor_kind: CallActorKind::Pat,
            authenticated: true,
            session_id: None,
            request_id: None,
            channel: BusinessChannel::Mcp,
            scopes,
        }
    }

    pub fn anonymous(scopes: AgentScopeSet) -> Self {
        Self {
            actor: "anonymous".to_string(),
            credential_id: None,
            actor_kind: CallActorKind::Anonymous,
            authenticated: false,
            session_id: None,
            request_id: None,
            channel: BusinessChannel::Rest,
            scopes,
        }
    }

    pub fn session(actor: impl Into<String>, scopes: AgentScopeSet) -> Self {
        Self {
            actor: actor.into(),
            credential_id: None,
            actor_kind: CallActorKind::Session,
            authenticated: true,
            session_id: None,
            request_id: None,
            channel: BusinessChannel::Gui,
            scopes,
        }
    }

    pub fn with_session(mut self, session_id: impl Into<String>) -> Self {
        self.session_id = Some(session_id.into());
        self
    }

    pub fn with_request(mut self, request_id: impl Into<String>) -> Self {
        self.request_id = Some(request_id.into());
        self
    }

    /// The host decides the surface: the same tools are served over HTTP as
    /// `mcp` and over the local socket as `cli`.
    pub fn with_channel(mut self, channel: BusinessChannel) -> Self {
        self.channel = channel;
        self
    }
}

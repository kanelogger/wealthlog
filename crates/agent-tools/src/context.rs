//! Trusted call context for agent tools.
//!
//! Hosts produce this from their authenticated transport (PAT middleware,
//! desktop session), and the catalog passes it to tools that need to record
//! who called them. It is never built from client-supplied fields.

use wealthfolio_business::audit::BusinessChannel;

use crate::scope::{AgentScope, AgentScopeSet};
use wealthfolio_business::investment_journal::{
    JournalActorKind, JournalCaller, JournalFactPermissions,
};

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
    /// Map authenticated host facts, never tool arguments. A program keeps its
    /// program identity even when it declares USER_PROVIDED contents or a GUI channel.
    pub fn journal_caller(&self) -> JournalCaller {
        let read = self.scopes.grants_all(&[
            AgentScope::InvestmentsRead,
            AgentScope::AccountsRead,
            AgentScope::HoldingsRead,
            AgentScope::ActivitiesRead,
        ]);
        let write = read && self.scopes.contains(AgentScope::InvestmentsWrite);
        let gui_session =
            self.actor_kind == CallActorKind::Session && self.channel == BusinessChannel::Gui;
        JournalCaller {
            channel: self.channel,
            actor_id: self.actor.clone(),
            credential_id: self.credential_id.clone(),
            actor_kind: if gui_session {
                JournalActorKind::GuiSession
            } else {
                JournalActorKind::Program
            },
            authenticated: self.authenticated,
            can_read: read,
            can_write: write,
            can_confirm: write && self.scopes.contains(AgentScope::InvestmentsConfirm),
            fact_permissions: JournalFactPermissions {
                holdings_write: self.scopes.contains(AgentScope::HoldingsWrite),
                transfers_write: self.scopes.contains(AgentScope::TransfersWrite),
                // External security writes retain the existing manual-only boundary.
                activities_write: gui_session && self.scopes.contains(AgentScope::ActivitiesWrite),
            },
        }
    }
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

#[cfg(test)]
mod journal_tests {
    use super::*;
    fn granted() -> AgentScopeSet {
        AgentScopeSet::from_strs([
            "accounts:read",
            "holdings:read",
            "activities:read",
            "investments:read",
            "investments:write",
        ])
    }
    #[test]
    fn ordinary_write_never_grants_confirmation_or_ledger_security_writes() {
        let context = ToolCallContext::pat("program", Some("credential".into()), granted())
            .with_channel(BusinessChannel::Cli);
        let caller = context.journal_caller();
        assert!(caller.require_write().is_ok());
        assert!(caller.require_confirm().is_err());
        assert!(matches!(caller.actor_kind, JournalActorKind::Program));
        assert_eq!(caller.actor_id, "program");
        assert_eq!(caller.credential_id.as_deref(), Some("credential"));
        let mut confirmed = context.clone();
        confirmed.scopes.insert(AgentScope::InvestmentsConfirm);
        assert!(confirmed.journal_caller().require_confirm().is_ok());
        confirmed.scopes.insert(AgentScope::ActivitiesWrite);
        confirmed.scopes.insert(AgentScope::ActivitiesDraft);
        confirmed.channel = BusinessChannel::Gui;
        let mapped = confirmed.journal_caller();
        assert!(!mapped.fact_permissions.activities_write);
        assert!(matches!(mapped.actor_kind, JournalActorKind::Program));
    }
    #[test]
    fn old_tokens_and_incomplete_dependencies_cannot_enter_the_journal() {
        let old = ToolCallContext::pat(
            "old",
            None,
            AgentScopeSet::from_strs([
                "accounts:read",
                "holdings:read",
                "activities:read",
                "holdings:write",
            ]),
        );
        assert!(old.journal_caller().require_read().is_err());
        let mut incomplete = ToolCallContext::pat(
            "incomplete",
            None,
            AgentScopeSet::from_strs([
                "investments:read",
                "investments:write",
                "investments:confirm",
            ]),
        );
        assert!(incomplete.journal_caller().require_confirm().is_err());
        incomplete.authenticated = false;
        incomplete.scopes = granted();
        assert!(incomplete.journal_caller().require_write().is_err());
        assert!(ToolCallContext::anonymous(granted())
            .journal_caller()
            .require_read()
            .is_err());
    }
    #[test]
    fn gui_session_identity_does_not_imply_independent_confirmation_permission() {
        let mut context = ToolCallContext::session("unlocked-session", granted());
        context.scopes.insert(AgentScope::ActivitiesWrite);
        let mapped = context.journal_caller();
        assert!(matches!(mapped.actor_kind, JournalActorKind::GuiSession));
        assert!(mapped.fact_permissions.activities_write);
        assert!(mapped.require_confirm().is_err());
        context.scopes.insert(AgentScope::InvestmentsConfirm);
        assert!(context.journal_caller().require_confirm().is_ok());
        context.channel = BusinessChannel::Rest;
        assert!(context.journal_caller().require_read().is_err());
    }
}

//! Business audit trail shared by every entry point.

use serde::{Deserialize, Serialize};

use crate::store::{new_id, now};

/// Which surface performed the write.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum BusinessChannel {
    Gui,
    Cli,
    Rest,
    Mcp,
}

impl BusinessChannel {
    pub fn as_str(self) -> &'static str {
        match self {
            BusinessChannel::Gui => "gui",
            BusinessChannel::Cli => "cli",
            BusinessChannel::Rest => "rest",
            BusinessChannel::Mcp => "mcp",
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum BusinessOutcome {
    Success,
    Error,
    Denied,
}

impl BusinessOutcome {
    pub fn as_str(self) -> &'static str {
        match self {
            BusinessOutcome::Success => "success",
            BusinessOutcome::Error => "error",
            BusinessOutcome::Denied => "denied",
        }
    }
}

/// Who performed the write and what happened.
///
/// `actor` is a token id, a session id or `anonymous`; it never carries a raw
/// credential and is never presented as a verified person.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct BusinessAuditEntry {
    pub ledger_id: String,
    pub channel: BusinessChannel,
    pub actor: String,
    /// Whether middleware authenticated the actor. Anonymous REST writes are
    /// recorded as unauthenticated calls, not as an individual.
    pub authenticated: bool,
    pub operation: String,
    pub entity_type: String,
    pub entity_id: Option<String>,
    pub request_id: Option<String>,
    pub outcome: BusinessOutcome,
    pub detail: Option<serde_json::Value>,
}

impl BusinessAuditEntry {
    pub fn new(
        ledger_id: impl Into<String>,
        channel: BusinessChannel,
        actor: impl Into<String>,
        authenticated: bool,
        operation: impl Into<String>,
        entity_type: impl Into<String>,
    ) -> Self {
        Self {
            ledger_id: ledger_id.into(),
            channel,
            actor: actor.into(),
            authenticated,
            operation: operation.into(),
            entity_type: entity_type.into(),
            entity_id: None,
            request_id: None,
            outcome: BusinessOutcome::Success,
            detail: None,
        }
    }

    pub fn entity(mut self, entity_id: impl Into<String>) -> Self {
        self.entity_id = Some(entity_id.into());
        self
    }

    pub fn request(mut self, request_id: impl Into<String>) -> Self {
        self.request_id = Some(request_id.into());
        self
    }

    pub fn outcome(mut self, outcome: BusinessOutcome) -> Self {
        self.outcome = outcome;
        self
    }

    pub fn detail(mut self, detail: serde_json::Value) -> Self {
        self.detail = Some(detail);
        self
    }

    pub fn to_row(&self) -> crate::store::NewAuditRow {
        crate::store::NewAuditRow {
            id: new_id("bizaudit"),
            ledger_id: self.ledger_id.clone(),
            channel: self.channel.as_str().to_string(),
            actor: self.actor.clone(),
            authenticated: i32::from(self.authenticated),
            operation: self.operation.clone(),
            entity_type: self.entity_type.clone(),
            entity_id: self.entity_id.clone(),
            request_id: self.request_id.clone(),
            outcome: self.outcome.as_str().to_string(),
            detail_json: self
                .detail
                .as_ref()
                .and_then(|value| serde_json::to_string(value).ok()),
            created_at: now(),
        }
    }
}

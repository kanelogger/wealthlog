//! Native-session entry to the same command implementation used by CLI/MCP.
use crate::profiles::ProfileAccess;
use serde::Serialize;
use serde_json::Value;
use wealthfolio_business::investment_journal::{
    InvestmentJournalCommand, JournalActorKind, JournalCaller, JournalError, JournalFactPermissions,
};
use wealthfolio_business::BusinessChannel;

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct JournalCommandError {
    code: String,
    message: String,
    retryable: bool,
}
impl From<JournalError> for JournalCommandError {
    fn from(error: JournalError) -> Self {
        Self {
            code: error.code().into(),
            message: error.to_string(),
            retryable: matches!(error, JournalError::Storage(_)),
        }
    }
}

#[tauri::command]
pub async fn execute_investment_journal(
    command: InvestmentJournalCommand,
    args: Value,
    state: ProfileAccess,
) -> Result<Value, JournalCommandError> {
    // ProfileAccess is extracted by Tauri from the admitted native window,
    // active unlocked session and runtime generation. None of these facts come
    // from command arguments or a contentOrigin declaration.
    let context = state.context().map_err(|message| JournalCommandError {
        code: "profile_unavailable".into(),
        message: message.to_string(),
        retryable: false,
    })?;
    let service = context
        .agent_environment
        .investment_journal_service()
        .ok_or(JournalError::ResourceUnavailable)?;
    let caller = JournalCaller {
        channel: BusinessChannel::Gui,
        actor_id: format!("desktop-session:{}", state.profile_id),
        credential_id: None,
        actor_kind: JournalActorKind::GuiSession,
        authenticated: true,
        can_read: true,
        can_write: true,
        can_confirm: true,
        fact_permissions: JournalFactPermissions {
            holdings_write: true,
            transfers_write: true,
            activities_write: true,
        },
    };
    command
        .execute(&service, args, &caller)
        .await
        .map_err(Into::into)
}

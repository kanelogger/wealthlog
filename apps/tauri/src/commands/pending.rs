//! Pending-entry commands (待确认收支).
//!
//! Confirming an entry creates exactly one posted activity and records the
//! confirming desktop session separately from the creating credential.

use crate::profiles::ProfileAccess;
use serde::Deserialize;
use wealthfolio_business::audit::BusinessChannel;
use wealthfolio_business::pending::{
    PendingActivity, PendingConfirmContext, PendingConfirmResult, PendingCreate, PendingUpdate,
};
use wealthfolio_core::health::HealthServiceTrait;

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ConfirmPendingArgs {
    pub id: String,
    pub expected_version: i32,
}

#[tauri::command]
pub async fn list_pending_activities(
    include_resolved: Option<bool>,
    state: ProfileAccess,
) -> Result<Vec<PendingActivity>, String> {
    let context = state.context()?;
    context
        .pending_service
        .list(include_resolved.unwrap_or(false))
        .map_err(|error| error.to_string())
}

#[tauri::command]
pub async fn create_pending_activity(
    pending: PendingCreate,
    state: ProfileAccess,
) -> Result<PendingActivity, String> {
    let context = state.context()?;
    context
        .pending_service
        .create(pending, "manual", BusinessChannel::Gui, true)
        .await
        .map_err(|error| error.to_string())
}

#[tauri::command]
pub async fn update_pending_activity(
    id: String,
    pending: PendingUpdate,
    state: ProfileAccess,
) -> Result<PendingActivity, String> {
    let context = state.context()?;
    context
        .pending_service
        .update(&id, pending, "desktop-session", BusinessChannel::Gui, true)
        .await
        .map_err(|error| error.to_string())
}

#[tauri::command]
pub async fn confirm_pending_activity(
    args: ConfirmPendingArgs,
    state: ProfileAccess,
) -> Result<PendingConfirmResult, String> {
    let context = state.context()?;
    let outcome = context
        .pending_service
        .confirm(
            &args.id,
            args.expected_version,
            PendingConfirmContext {
                channel: BusinessChannel::Gui,
                actor: "desktop-session",
                authenticated: true,
            },
        )
        .await
        .map_err(|error| error.to_string())?;
    context.health_service().clear_cache().await;
    Ok(outcome.into_value())
}

#[tauri::command]
pub async fn reject_pending_activity(
    args: ConfirmPendingArgs,
    state: ProfileAccess,
) -> Result<PendingActivity, String> {
    let context = state.context()?;
    context
        .pending_service
        .reject(
            &args.id,
            args.expected_version,
            "desktop-session",
            BusinessChannel::Gui,
            true,
        )
        .await
        .map_err(|error| error.to_string())
}

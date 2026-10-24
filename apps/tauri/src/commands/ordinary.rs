//! Unified ordinary income/expense commands.
//!
//! The desktop form branches here only for the ordinary income/expense
//! envelope; every other type keeps the existing activity command.

use crate::profiles::ProfileAccess;
use wealthfolio_business::audit::BusinessChannel;
use wealthfolio_business::service::{
    CreateOrdinaryActivity, OrdinaryActivityDetail, OrdinaryActivityPage, OrdinaryActivityQuery,
    OrdinaryActivityResult, UpdateOrdinaryActivity,
};
use wealthfolio_core::health::HealthServiceTrait;

#[tauri::command]
pub async fn query_ordinary_activities(
    query: OrdinaryActivityQuery,
    state: ProfileAccess,
) -> Result<OrdinaryActivityPage, String> {
    let context = state.context()?;
    context
        .business_service
        .query_ordinary(query)
        .await
        .map_err(|error| error.to_string())
}

#[tauri::command]
pub async fn get_ordinary_activity(
    activity_id: String,
    state: ProfileAccess,
) -> Result<OrdinaryActivityDetail, String> {
    let context = state.context()?;
    context
        .business_service
        .get_ordinary(&activity_id)
        .await
        .map_err(|error| error.to_string())
}

#[tauri::command]
pub async fn create_ordinary_activity(
    input: CreateOrdinaryActivity,
    state: ProfileAccess,
) -> Result<OrdinaryActivityResult, String> {
    let context = state.context()?;
    let outcome = context
        .business_service
        .create_ordinary(input, BusinessChannel::Gui, "desktop-session", true)
        .await
        .map_err(|error| error.legacy_entry_message())?;
    context.health_service().clear_cache().await;
    Ok(outcome.into_value())
}

#[tauri::command]
pub async fn update_ordinary_activity(
    input: UpdateOrdinaryActivity,
    state: ProfileAccess,
) -> Result<OrdinaryActivityResult, String> {
    let context = state.context()?;
    let outcome = context
        .business_service
        .update_ordinary(input, BusinessChannel::Gui, "desktop-session", true)
        .await
        .map_err(|error| error.legacy_entry_message())?;
    context.health_service().clear_cache().await;
    Ok(outcome.into_value())
}

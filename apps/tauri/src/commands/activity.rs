use crate::profiles::ProfileAccess;

use log::debug;
use wealthfolio_core::activities::{
    Activity, ActivityBulkMutationRequest, ActivityBulkMutationResult, ActivitySearchResponse,
    ActivityUpdate, InternalTransferPairRequest, InternalTransferPairResponse, NewActivity, Sort,
    TransferMatchCandidate, TransferMatchCandidateRequest,
};
use wealthfolio_core::health::HealthServiceTrait;
use wealthfolio_core::utils::time_utils::{
    local_date_range_utc_bounds, parse_user_timezone_or_default,
};

#[allow(clippy::too_many_arguments)]
#[tauri::command]
pub async fn search_activities(
    page: i64,                                 // Page number, 0-based
    page_size: i64,                            // Number of items per page
    account_id_filter: Option<Vec<String>>,    // Optional account_id filter
    activity_type_filter: Option<Vec<String>>, // Optional activity_type filter
    asset_id_keyword: Option<String>,          // Optional asset_id keyword for search
    sort: Option<Sort>,
    needs_review_filter: Option<bool>, // Optional needs_review filter for pending review
    date_from: Option<String>,         // Optional start date filter (YYYY-MM-DD, inclusive)
    date_to: Option<String>,           // Optional end date filter (YYYY-MM-DD, inclusive)
    instrument_type_filter: Option<Vec<String>>, // Optional instrument_type filter
    activity_id_filter: Option<Vec<String>>, // Optional exact activity-id filter
    state: ProfileAccess,
) -> Result<ActivitySearchResponse, String> {
    let context = state.context()?;
    debug!("Search activities... {}, {}", page, page_size);

    // Parse date strings to NaiveDate
    let date_from_parsed = date_from
        .map(|s| chrono::NaiveDate::parse_from_str(&s, "%Y-%m-%d"))
        .transpose()
        .map_err(|e| format!("Invalid date_from format: {}", e))?;
    let date_to_parsed = date_to
        .map(|s| chrono::NaiveDate::parse_from_str(&s, "%Y-%m-%d"))
        .transpose()
        .map_err(|e| format!("Invalid date_to format: {}", e))?;
    let timezone = context.get_timezone();
    let tz = parse_user_timezone_or_default(&timezone);
    let (date_from_utc, date_to_utc_exclusive) =
        local_date_range_utc_bounds(date_from_parsed, date_to_parsed, tz)
            .map_err(|e| e.to_string())?;

    Ok(context.activity_service().search_activities_in_utc_range(
        page,
        page_size,
        account_id_filter,
        activity_type_filter,
        asset_id_keyword,
        sort,
        needs_review_filter,
        date_from_utc,
        date_to_utc_exclusive,
        instrument_type_filter,
        activity_id_filter,
    )?)
}

#[tauri::command]
pub async fn create_activity(
    activity: NewActivity,
    state: ProfileAccess,
) -> Result<Activity, String> {
    let context = state.context()?;
    debug!("Creating activity...");
    // Domain events handle recalculation and asset enrichment automatically
    let created = context
        .activity_service()
        .create_activity(activity)
        .await
        .map_err(|e| e.to_string())?;
    context.health_service().clear_cache().await;
    Ok(created)
}

#[tauri::command]
pub async fn update_activity(
    activity: ActivityUpdate,
    state: ProfileAccess,
) -> Result<Activity, String> {
    let context = state.context()?;
    debug!("Updating activity...");
    // Domain events handle recalculation and asset enrichment automatically
    let updated = context
        .activity_service()
        .update_activity(activity)
        .await
        .map_err(|e| e.to_string())?;
    context.health_service().clear_cache().await;
    Ok(updated)
}

#[tauri::command]
pub async fn delete_activity(
    activity_id: String,
    state: ProfileAccess,
) -> Result<Activity, String> {
    let context = state.context()?;
    debug!("Deleting activity...");
    // Domain events handle recalculation automatically
    let deleted = context
        .activity_service()
        .delete_activity(activity_id)
        .await
        .map_err(|e| e.to_string())?;
    context.health_service().clear_cache().await;
    Ok(deleted)
}

#[tauri::command]
pub async fn get_transfer_pair_for_activity(
    activity_id: String,
    state: ProfileAccess,
) -> Result<Option<InternalTransferPairResponse>, String> {
    let context = state.context()?;
    debug!("Getting transfer pair...");
    context
        .activity_service()
        .get_transfer_pair_for_activity(activity_id)
        .map_err(|e| e.to_string())
}

#[tauri::command]
pub async fn find_transfer_match_candidates(
    request: TransferMatchCandidateRequest,
    state: ProfileAccess,
) -> Result<Vec<TransferMatchCandidate>, String> {
    let context = state.context()?;
    debug!("Finding transfer match candidates...");
    context
        .activity_service()
        .find_transfer_match_candidates(request)
        .map_err(|e| e.to_string())
}

#[tauri::command]
pub async fn save_internal_transfer_pair(
    request: InternalTransferPairRequest,
    state: ProfileAccess,
) -> Result<InternalTransferPairResponse, String> {
    let context = state.context()?;
    debug!("Saving internal transfer pair...");
    let pair = context
        .activity_service()
        .save_internal_transfer_pair(request)
        .await
        .map_err(|e| e.to_string())?;
    context.health_service().clear_cache().await;
    Ok(pair)
}

#[tauri::command]
pub async fn link_transfer_activities(
    activity_a_id: String,
    activity_b_id: String,
    state: ProfileAccess,
) -> Result<(Activity, Activity), String> {
    let context = state.context()?;
    debug!("Linking transfer activities...");
    // Domain events handle recalculation automatically
    let pair = context
        .activity_service()
        .link_transfer_activities(activity_a_id, activity_b_id)
        .await
        .map_err(|e| e.to_string())?;
    context.health_service().clear_cache().await;
    Ok(pair)
}

#[tauri::command]
pub async fn unlink_transfer_activities(
    activity_a_id: String,
    activity_b_id: String,
    state: ProfileAccess,
) -> Result<(Activity, Activity), String> {
    let context = state.context()?;
    debug!("Unlinking transfer activities...");
    // Domain events handle recalculation automatically
    let pair = context
        .activity_service()
        .unlink_transfer_activities(activity_a_id, activity_b_id)
        .await
        .map_err(|e| e.to_string())?;
    context.health_service().clear_cache().await;
    Ok(pair)
}

#[tauri::command]
pub async fn save_activities(
    request: ActivityBulkMutationRequest,
    state: ProfileAccess,
) -> Result<ActivityBulkMutationResult, String> {
    let context = state.context()?;
    let create_count = request.creates.len();
    let update_count = request.updates.len();
    let delete_count = request.delete_ids.len();
    debug!(
        "Bulk activity mutation request: {} creates, {} updates, {} deletes",
        create_count, update_count, delete_count
    );

    // Domain events handle recalculation and asset enrichment automatically
    let result = context
        .activity_service()
        .bulk_mutate_activities(request)
        .await
        .map_err(|e| e.to_string())?;
    context.health_service().clear_cache().await;
    Ok(result)
}

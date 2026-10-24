use std::sync::Arc;

use crate::{error::ApiResult, main_lib::AppState};
use axum::{
    extract::Path,
    routing::{delete, get, post},
    Json, Router,
};
use wealthfolio_core::activities::{
    Activity, ActivityBulkMutationRequest, ActivityBulkMutationResult, ActivitySearchResponse,
    ActivityUpdate, InternalTransferPairRequest, InternalTransferPairResponse, NewActivity,
    TransferMatchCandidate, TransferMatchCandidateRequest,
};
use wealthfolio_core::utils::time_utils::{
    local_date_range_utc_bounds, parse_user_timezone_or_default,
};

use super::shared::parse_date_optional;

#[derive(serde::Deserialize)]
#[serde(untagged)]
enum SortWrapper {
    One(wealthfolio_core::activities::Sort),
    Many(Vec<wealthfolio_core::activities::Sort>),
}

#[derive(serde::Deserialize)]
#[serde(untagged)]
enum StringOrVec {
    One(String),
    Many(Vec<String>),
}

#[derive(serde::Deserialize)]
struct ActivitySearchBody {
    page: i64,
    #[serde(rename = "pageSize")]
    page_size: i64,
    #[serde(rename = "accountIdFilter")]
    account_id_filter: Option<StringOrVec>,
    #[serde(rename = "activityTypeFilter")]
    activity_type_filter: Option<StringOrVec>,
    #[serde(rename = "assetIdKeyword")]
    asset_id_keyword: Option<String>,
    // Allow addons to pass either a single sort or an array (we pick the first)
    sort: Option<SortWrapper>,
    #[serde(rename = "needsReviewFilter")]
    needs_review_filter: Option<bool>,
    #[serde(rename = "dateFrom")]
    date_from: Option<String>, // YYYY-MM-DD format
    #[serde(rename = "dateTo")]
    date_to: Option<String>, // YYYY-MM-DD format
    #[serde(rename = "instrumentTypeFilter")]
    instrument_type_filter: Option<StringOrVec>,
    #[serde(rename = "activityIdFilter")]
    activity_id_filter: Option<StringOrVec>,
}

async fn search_activities(
    axum::Extension(state): axum::Extension<Arc<AppState>>,
    Json(body): Json<ActivitySearchBody>,
) -> ApiResult<Json<ActivitySearchResponse>> {
    // Normalize sort to a single value if provided
    let sort_normalized: Option<wealthfolio_core::activities::Sort> = match body.sort {
        Some(SortWrapper::One(s)) => Some(s),
        Some(SortWrapper::Many(v)) => v.into_iter().next(),
        None => None,
    };
    let account_ids: Option<Vec<String>> = match body.account_id_filter {
        Some(StringOrVec::One(s)) => Some(vec![s]),
        Some(StringOrVec::Many(v)) => Some(v),
        None => None,
    };
    let types: Option<Vec<String>> = match body.activity_type_filter {
        Some(StringOrVec::One(s)) => Some(vec![s]),
        Some(StringOrVec::Many(v)) => Some(v),
        None => None,
    };
    let instrument_types: Option<Vec<String>> = match body.instrument_type_filter {
        Some(StringOrVec::One(s)) => Some(vec![s]),
        Some(StringOrVec::Many(v)) => Some(v),
        None => None,
    };
    let activity_ids: Option<Vec<String>> = match body.activity_id_filter {
        Some(StringOrVec::One(s)) => Some(vec![s]),
        Some(StringOrVec::Many(v)) => Some(v),
        None => None,
    };
    // Parse date filters
    let date_from_parsed = parse_date_optional(body.date_from, "dateFrom")?;
    let date_to_parsed = parse_date_optional(body.date_to, "dateTo")?;
    let timezone = state
        .timezone
        .read()
        .unwrap_or_else(|poisoned| poisoned.into_inner())
        .clone();
    let tz = parse_user_timezone_or_default(&timezone);
    let (date_from_utc, date_to_utc_exclusive) =
        local_date_range_utc_bounds(date_from_parsed, date_to_parsed, tz)?;

    let resp = state.activity_service.search_activities_in_utc_range(
        body.page,
        body.page_size,
        account_ids,
        types,
        body.asset_id_keyword,
        sort_normalized,
        body.needs_review_filter,
        date_from_utc,
        date_to_utc_exclusive,
        instrument_types,
        activity_ids,
    )?;
    Ok(Json(resp))
}

async fn create_activity(
    axum::Extension(state): axum::Extension<Arc<AppState>>,
    Json(activity): Json<NewActivity>,
) -> ApiResult<Json<Activity>> {
    let created = state.activity_service.create_activity(activity).await?;
    state.health_service.clear_cache().await;

    // Domain events handle asset enrichment and portfolio recalculation
    Ok(Json(created))
}

async fn update_activity(
    axum::Extension(state): axum::Extension<Arc<AppState>>,
    Json(activity): Json<ActivityUpdate>,
) -> ApiResult<Json<Activity>> {
    let updated = state.activity_service.update_activity(activity).await?;
    state.health_service.clear_cache().await;
    // Domain events handle asset enrichment and portfolio recalculation
    Ok(Json(updated))
}

async fn save_activities(
    axum::Extension(state): axum::Extension<Arc<AppState>>,
    Json(request): Json<ActivityBulkMutationRequest>,
) -> ApiResult<Json<ActivityBulkMutationResult>> {
    let result = state
        .activity_service
        .bulk_mutate_activities(request)
        .await?;
    state.health_service.clear_cache().await;
    // Domain events handle asset enrichment and portfolio recalculation
    Ok(Json(result))
}

async fn delete_activity(
    Path(id): Path<String>,
    axum::Extension(state): axum::Extension<Arc<AppState>>,
) -> ApiResult<Json<Activity>> {
    let deleted = state.activity_service.delete_activity(id).await?;
    state.health_service.clear_cache().await;
    // Domain events handle portfolio recalculation
    Ok(Json(deleted))
}

async fn get_transfer_pair_for_activity(
    Path(id): Path<String>,
    axum::Extension(state): axum::Extension<Arc<AppState>>,
) -> ApiResult<Json<Option<InternalTransferPairResponse>>> {
    let pair = state.activity_service.get_transfer_pair_for_activity(id)?;
    Ok(Json(pair))
}

async fn find_transfer_match_candidates(
    axum::Extension(state): axum::Extension<Arc<AppState>>,
    Json(request): Json<TransferMatchCandidateRequest>,
) -> ApiResult<Json<Vec<TransferMatchCandidate>>> {
    let candidates = state
        .activity_service
        .find_transfer_match_candidates(request)?;
    Ok(Json(candidates))
}

async fn save_internal_transfer_pair(
    axum::Extension(state): axum::Extension<Arc<AppState>>,
    Json(request): Json<InternalTransferPairRequest>,
) -> ApiResult<Json<InternalTransferPairResponse>> {
    let pair = state
        .activity_service
        .save_internal_transfer_pair(request)
        .await?;
    state.health_service.clear_cache().await;
    Ok(Json(pair))
}

#[derive(serde::Deserialize)]
#[serde(rename_all = "camelCase")]
struct LinkTransferActivitiesBody {
    activity_a_id: String,
    activity_b_id: String,
}

async fn link_transfer_activities(
    axum::Extension(state): axum::Extension<Arc<AppState>>,
    Json(body): Json<LinkTransferActivitiesBody>,
) -> ApiResult<Json<(Activity, Activity)>> {
    let pair = state
        .activity_service
        .link_transfer_activities(body.activity_a_id, body.activity_b_id)
        .await?;
    state.health_service.clear_cache().await;
    // Domain events handle portfolio recalculation
    Ok(Json(pair))
}

async fn unlink_transfer_activities(
    axum::Extension(state): axum::Extension<Arc<AppState>>,
    Json(body): Json<LinkTransferActivitiesBody>,
) -> ApiResult<Json<(Activity, Activity)>> {
    let pair = state
        .activity_service
        .unlink_transfer_activities(body.activity_a_id, body.activity_b_id)
        .await?;
    state.health_service.clear_cache().await;
    // Domain events handle portfolio recalculation
    Ok(Json(pair))
}

pub fn router<S: Clone + Send + Sync + 'static>() -> Router<S> {
    Router::new()
        .route("/activities/search", post(search_activities))
        .route("/activities", post(create_activity).put(update_activity))
        .route("/activities/bulk", post(save_activities))
        .route("/activities/{id}", delete(delete_activity))
        .route(
            "/activities/{id}/transfer-pair",
            get(get_transfer_pair_for_activity),
        )
        .route(
            "/activities/transfer-pair",
            post(save_internal_transfer_pair),
        )
        .route(
            "/activities/transfer-match-candidates",
            post(find_transfer_match_candidates),
        )
        .route("/activities/link", post(link_transfer_activities))
        .route("/activities/unlink", post(unlink_transfer_activities))
}

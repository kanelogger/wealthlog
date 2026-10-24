//! Pending-entry REST endpoints.
//!
//! The GUI reviews and confirms pending entries here. Confirmation is the only
//! path that creates a posted activity from a pending entry; it records the
//! confirming session separately from the creating credential.

use std::sync::Arc;

use axum::{
    extract::Path,
    routing::{get, post},
    Json, Router,
};
use serde::Deserialize;

use crate::{
    auth::RequestAuthentication,
    error::{ApiError, ApiResult},
    main_lib::AppState,
};
use wealthfolio_business::audit::BusinessChannel;
use wealthfolio_business::pending::{
    PendingActivity, PendingConfirmContext, PendingConfirmResult, PendingCreate, PendingUpdate,
};
use wealthfolio_business::service::{
    CreateOrdinaryActivity, OrdinaryActivityDetail, OrdinaryActivityError, OrdinaryActivityPage,
    OrdinaryActivityQuery, UpdateOrdinaryActivity,
};

/// Surface the stable business error code before the human message.
fn business_error(error: OrdinaryActivityError) -> ApiError {
    // Existing REST clients keep the pre-Phase-2 body (including the
    // storage-wrapped duplicate text); typed structured errors are the
    // CLI/MCP contract.
    ApiError::BadRequest(error.legacy_rest_message())
}

#[derive(Debug, Deserialize, utoipa::ToSchema)]
#[serde(rename_all = "camelCase")]
pub struct ConfirmBody {
    expected_version: i32,
}

/// The confirming actor for a GUI session: the authenticated session, or an
/// anonymous REST caller. Never a person's name.
fn confirm_context<'a>(auth: &'a RequestAuthentication) -> PendingConfirmContext<'a> {
    PendingConfirmContext {
        channel: BusinessChannel::Rest,
        actor: &auth.actor,
        authenticated: auth.authenticated,
    }
}

#[utoipa::path(get, path = "/api/v1/pending-activities", responses((status = 200, body = Vec<PendingActivity>)))]
async fn list_pending(
    axum::Extension(state): axum::Extension<Arc<AppState>>,
    axum::extract::Query(params): axum::extract::Query<ListParams>,
) -> ApiResult<Json<Vec<PendingActivity>>> {
    let rows = state
        .pending_service
        .list(params.include_resolved)
        .map_err(business_error)?;
    Ok(Json(rows))
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
struct ListParams {
    #[serde(default)]
    include_resolved: bool,
}

#[utoipa::path(post, path = "/api/v1/pending-activities", request_body = PendingCreate, responses((status = 200, body = PendingActivity)))]
async fn create_pending(
    axum::Extension(state): axum::Extension<Arc<AppState>>,
    axum::Extension(auth): axum::Extension<RequestAuthentication>,
    Json(payload): Json<PendingCreate>,
) -> ApiResult<Json<PendingActivity>> {
    let created = state
        .pending_service
        .create(
            payload,
            &auth.actor,
            BusinessChannel::Rest,
            auth.authenticated,
        )
        .await
        .map_err(business_error)?;
    Ok(Json(created))
}

#[utoipa::path(put, path = "/api/v1/pending-activities/{id}", params(("id" = String, Path, description = "Pending entry id")), request_body = PendingUpdate, responses((status = 200, body = PendingActivity)))]
async fn update_pending(
    axum::Extension(state): axum::Extension<Arc<AppState>>,
    axum::Extension(auth): axum::Extension<RequestAuthentication>,
    Path(id): Path<String>,
    Json(payload): Json<PendingUpdate>,
) -> ApiResult<Json<PendingActivity>> {
    let updated = state
        .pending_service
        .update(
            &id,
            payload,
            &auth.actor,
            BusinessChannel::Rest,
            auth.authenticated,
        )
        .await
        .map_err(business_error)?;
    Ok(Json(updated))
}

#[utoipa::path(post, path = "/api/v1/pending-activities/{id}/confirm", params(("id" = String, Path, description = "Pending entry id")), request_body = ConfirmBody, responses((status = 200, body = PendingConfirmResult)))]
async fn confirm_pending(
    axum::Extension(state): axum::Extension<Arc<AppState>>,
    axum::Extension(auth): axum::Extension<RequestAuthentication>,
    Path(id): Path<String>,
    Json(payload): Json<ConfirmBody>,
) -> ApiResult<Json<PendingConfirmResult>> {
    let outcome = state
        .pending_service
        .confirm(&id, payload.expected_version, confirm_context(&auth))
        .await
        .map_err(business_error)?;
    Ok(Json(outcome.into_value()))
}

#[utoipa::path(post, path = "/api/v1/pending-activities/{id}/reject", params(("id" = String, Path, description = "Pending entry id")), request_body = ConfirmBody, responses((status = 200, body = PendingActivity)))]
async fn reject_pending(
    axum::Extension(state): axum::Extension<Arc<AppState>>,
    axum::Extension(auth): axum::Extension<RequestAuthentication>,
    Path(id): Path<String>,
    Json(payload): Json<ConfirmBody>,
) -> ApiResult<Json<PendingActivity>> {
    let rejected = state
        .pending_service
        .reject(
            &id,
            payload.expected_version,
            &auth.actor,
            BusinessChannel::Rest,
            auth.authenticated,
        )
        .await
        .map_err(business_error)?;
    Ok(Json(rejected))
}

#[utoipa::path(post, path = "/api/v1/ordinary-activities/query", request_body = OrdinaryActivityQuery, responses((status = 200, body = OrdinaryActivityPage)))]
async fn query_ordinary(
    axum::Extension(state): axum::Extension<Arc<AppState>>,
    Json(query): Json<OrdinaryActivityQuery>,
) -> ApiResult<Json<OrdinaryActivityPage>> {
    state
        .business_service
        .query_ordinary(query)
        .await
        .map(Json)
        .map_err(business_error)
}

#[utoipa::path(get, path = "/api/v1/ordinary-activities/{id}", params(("id" = String, Path, description = "Activity id")), responses((status = 200, body = OrdinaryActivityDetail)))]
async fn get_ordinary(
    axum::Extension(state): axum::Extension<Arc<AppState>>,
    Path(id): Path<String>,
) -> ApiResult<Json<OrdinaryActivityDetail>> {
    state
        .business_service
        .get_ordinary(&id)
        .await
        .map(Json)
        .map_err(business_error)
}

#[utoipa::path(post, path = "/api/v1/ordinary-activities", request_body = CreateOrdinaryActivity, responses((status = 200, body = OrdinaryActivityResult)))]
async fn create_ordinary(
    axum::Extension(state): axum::Extension<Arc<AppState>>,
    axum::Extension(auth): axum::Extension<RequestAuthentication>,
    Json(payload): Json<CreateOrdinaryActivity>,
) -> ApiResult<Json<wealthfolio_business::OrdinaryActivityResult>> {
    state
        .business_service
        .create_ordinary(
            payload,
            BusinessChannel::Rest,
            &auth.actor,
            auth.authenticated,
        )
        .await
        .map(|outcome| Json(outcome.into_value()))
        .map_err(business_error)
}

#[utoipa::path(put, path = "/api/v1/ordinary-activities", request_body = UpdateOrdinaryActivity, responses((status = 200, body = OrdinaryActivityResult)))]
async fn update_ordinary(
    axum::Extension(state): axum::Extension<Arc<AppState>>,
    axum::Extension(auth): axum::Extension<RequestAuthentication>,
    Json(payload): Json<UpdateOrdinaryActivity>,
) -> ApiResult<Json<wealthfolio_business::OrdinaryActivityResult>> {
    state
        .business_service
        .update_ordinary(
            payload,
            BusinessChannel::Rest,
            &auth.actor,
            auth.authenticated,
        )
        .await
        .map(|outcome| Json(outcome.into_value()))
        .map_err(business_error)
}

pub fn router<S: Clone + Send + Sync + 'static>() -> Router<S> {
    Router::new()
        .route("/ordinary-activities/query", post(query_ordinary))
        .route("/ordinary-activities", post(create_ordinary))
        .route("/ordinary-activities", axum::routing::put(update_ordinary))
        .route("/ordinary-activities/{id}", get(get_ordinary))
        .route("/pending-activities", get(list_pending))
        .route("/pending-activities", post(create_pending))
        .route(
            "/pending-activities/{id}",
            axum::routing::put(update_pending),
        )
        .route("/pending-activities/{id}/confirm", post(confirm_pending))
        .route("/pending-activities/{id}/reject", post(reject_pending))
}

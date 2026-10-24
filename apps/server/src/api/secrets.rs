use std::sync::Arc;

use crate::{
    error::{ApiError, ApiResult},
    main_lib::AppState,
};
use axum::{extract::Query, http::StatusCode, routing::post, Json, Router};
use wealthfolio_core::secrets::validate_unscoped_secret_service_id;

#[derive(serde::Deserialize)]
struct SecretSetBody {
    #[serde(rename = "secretKey")]
    secret_key: String,
    secret: String,
}

async fn set_secret(
    axum::Extension(state): axum::Extension<Arc<AppState>>,
    Json(body): Json<SecretSetBody>,
) -> ApiResult<StatusCode> {
    validate_unscoped_secret_service_id(&body.secret_key).map_err(ApiError::BadRequest)?;
    state
        .secret_store
        .set_secret(&body.secret_key, &body.secret)?;
    Ok(StatusCode::NO_CONTENT)
}

#[derive(serde::Deserialize)]
struct SecretQuery {
    #[serde(rename = "secretKey")]
    secret_key: String,
}

async fn get_secret(
    axum::Extension(state): axum::Extension<Arc<AppState>>,
    Query(q): Query<SecretQuery>,
) -> ApiResult<Json<Option<String>>> {
    validate_unscoped_secret_service_id(&q.secret_key).map_err(ApiError::BadRequest)?;
    let val = state.secret_store.get_secret(&q.secret_key)?;
    Ok(Json(val))
}

async fn delete_secret(
    axum::Extension(state): axum::Extension<Arc<AppState>>,
    Query(q): Query<SecretQuery>,
) -> ApiResult<StatusCode> {
    validate_unscoped_secret_service_id(&q.secret_key).map_err(ApiError::BadRequest)?;
    state.secret_store.delete_secret(&q.secret_key)?;
    Ok(StatusCode::NO_CONTENT)
}

pub fn router<S: Clone + Send + Sync + 'static>() -> Router<S> {
    Router::new().route(
        "/secrets",
        post(set_secret).get(get_secret).delete(delete_secret),
    )
}

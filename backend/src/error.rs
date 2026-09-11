use axum::http::StatusCode;
use axum::response::{IntoResponse, Response};
use axum::Json;
use serde_json::json;

use crate::auth::AuthError;
use crate::providers::ProviderError;

#[derive(Debug, thiserror::Error)]
pub enum ApiError {
    #[error("not found")]
    NotFound,
    #[error("bad request: {0}")]
    BadRequest(String),
    #[error("unauthorized: {0}")]
    Unauthorized(String),
    #[error("forbidden: {0}")]
    Forbidden(String),
    #[error("conflict: {0}")]
    Conflict(String),
    #[error(transparent)]
    Auth(#[from] AuthError),
    #[error(transparent)]
    Database(#[from] sqlx::Error),
    #[error(transparent)]
    Provider(#[from] ProviderError),
}

impl IntoResponse for ApiError {
    fn into_response(self) -> Response {
        let (status, message) = match &self {
            ApiError::NotFound => (StatusCode::NOT_FOUND, self.to_string()),
            ApiError::BadRequest(_) => (StatusCode::BAD_REQUEST, self.to_string()),
            ApiError::Unauthorized(m) => (StatusCode::UNAUTHORIZED, m.clone()),
            ApiError::Forbidden(m) => (StatusCode::FORBIDDEN, m.clone()),
            ApiError::Conflict(m) => (StatusCode::CONFLICT, m.clone()),
            ApiError::Auth(e) => match e {
                AuthError::JwksUnavailable | AuthError::NotConfigured => {
                    tracing::error!("auth backend error: {e:?}");
                    (
                        StatusCode::BAD_GATEWAY,
                        "authentication service unavailable".to_string(),
                    )
                }
                _ => (StatusCode::UNAUTHORIZED, e.to_string()),
            },
            ApiError::Database(e) => {
                if let Some(db) = e.as_database_error() {
                    if db.is_unique_violation() {
                        let msg = match db.constraint() {
                            Some("users_handle_key") => "handle already taken",
                            Some("users_hanko_user_id_key") => "profile already exists",
                            _ => "resource already exists",
                        };
                        return ApiError::Conflict(msg.to_string()).into_response();
                    }
                }
                tracing::error!("database error: {e:?}");
                (
                    StatusCode::INTERNAL_SERVER_ERROR,
                    "internal error".to_string(),
                )
            }
            ApiError::Provider(e) => match e {
                ProviderError::NotFound { .. } => (StatusCode::NOT_FOUND, e.to_string()),
                ProviderError::UnknownSource(_) => (StatusCode::BAD_REQUEST, e.to_string()),
                ProviderError::Http(_) | ProviderError::Status { .. } => {
                    tracing::error!("book provider error: {e:?}");
                    (
                        StatusCode::BAD_GATEWAY,
                        "book provider unavailable".to_string(),
                    )
                }
            },
        };
        (status, Json(json!({ "error": message }))).into_response()
    }
}

pub type ApiResult<T> = Result<T, ApiError>;

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
    #[error("unavailable: {0}")]
    Unavailable(String),
    #[error("upstream failure: {0}")]
    BadGateway(String),
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
            ApiError::Unavailable(m) => (StatusCode::SERVICE_UNAVAILABLE, m.clone()),
            ApiError::BadGateway(m) => (StatusCode::BAD_GATEWAY, m.clone()),
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
                        let msg = conflict_message_for_constraint(db.constraint());
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

/// Which unique constraint fired -> which user-facing message to give. Split
/// out of `IntoResponse` so it's testable without a real `sqlx::Error` (which
/// needs a live database error to construct — see the integration tests for
/// coverage of the real thing, e.g. onboarding twice).
fn conflict_message_for_constraint(constraint: Option<&str>) -> &'static str {
    match constraint {
        Some("users_handle_key") => "handle already taken",
        Some("users_hanko_user_id_key") => "profile already exists",
        _ => "resource already exists",
    }
}

pub type ApiResult<T> = Result<T, ApiError>;

#[cfg(test)]
mod tests {
    use super::*;
    use crate::auth::AuthError;

    fn status_of(err: ApiError) -> StatusCode {
        err.into_response().status()
    }

    #[test]
    fn conflict_message_matches_known_constraints() {
        assert_eq!(
            conflict_message_for_constraint(Some("users_handle_key")),
            "handle already taken"
        );
        assert_eq!(
            conflict_message_for_constraint(Some("users_hanko_user_id_key")),
            "profile already exists"
        );
        assert_eq!(
            conflict_message_for_constraint(Some("some_other_constraint")),
            "resource already exists"
        );
        assert_eq!(
            conflict_message_for_constraint(None),
            "resource already exists"
        );
    }

    #[test]
    fn variants_map_to_the_documented_status_codes() {
        assert_eq!(status_of(ApiError::NotFound), StatusCode::NOT_FOUND);
        assert_eq!(
            status_of(ApiError::BadRequest("x".into())),
            StatusCode::BAD_REQUEST
        );
        assert_eq!(
            status_of(ApiError::Unauthorized("x".into())),
            StatusCode::UNAUTHORIZED
        );
        assert_eq!(
            status_of(ApiError::Forbidden("x".into())),
            StatusCode::FORBIDDEN
        );
        assert_eq!(
            status_of(ApiError::Conflict("x".into())),
            StatusCode::CONFLICT
        );
    }

    #[test]
    fn auth_backend_errors_surface_as_bad_gateway_not_unauthorized() {
        // JWKS being unreachable is our fault (or Hanko's), not the caller's —
        // it must not look like a rejected token.
        assert_eq!(
            status_of(ApiError::Auth(AuthError::JwksUnavailable)),
            StatusCode::BAD_GATEWAY
        );
        assert_eq!(
            status_of(ApiError::Auth(AuthError::NotConfigured)),
            StatusCode::BAD_GATEWAY
        );
    }

    #[test]
    fn other_auth_errors_are_unauthorized() {
        assert_eq!(
            status_of(ApiError::Auth(AuthError::MissingToken)),
            StatusCode::UNAUTHORIZED
        );
        assert_eq!(
            status_of(ApiError::Auth(AuthError::UnknownKey)),
            StatusCode::UNAUTHORIZED
        );
        assert_eq!(
            status_of(ApiError::Auth(AuthError::InvalidToken("bad".into()))),
            StatusCode::UNAUTHORIZED
        );
    }

    #[test]
    fn database_errors_default_to_internal_server_error() {
        // A non-database-constraint sqlx error (e.g. a pool timeout) should
        // not leak internals to the caller — just a generic 500.
        assert_eq!(
            status_of(ApiError::Database(sqlx::Error::PoolTimedOut)),
            StatusCode::INTERNAL_SERVER_ERROR
        );
    }

    #[test]
    fn provider_not_found_is_404_and_unknown_source_is_400() {
        assert_eq!(
            status_of(ApiError::Provider(ProviderError::NotFound {
                provider: "open_library",
                source_id: "OLxW".into(),
            })),
            StatusCode::NOT_FOUND
        );
        assert_eq!(
            status_of(ApiError::Provider(ProviderError::UnknownSource(
                "carrier_pigeon".into()
            ))),
            StatusCode::BAD_REQUEST
        );
    }

    #[test]
    fn provider_upstream_failure_is_bad_gateway() {
        assert_eq!(
            status_of(ApiError::Provider(ProviderError::Status {
                status: StatusCode::INTERNAL_SERVER_ERROR,
                url: "https://openlibrary.org/search.json".into(),
            })),
            StatusCode::BAD_GATEWAY
        );
    }
}

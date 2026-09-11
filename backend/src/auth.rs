//! Authentication: validate Hanko Cloud JWTs and map them to `users` rows.
//!
//! The mobile client authenticates with Hanko, receives a JWT, and sends it as
//! `Authorization: Bearer <token>`. We verify the signature against Hanko's JWKS
//! (`<HANKO_API_URL>/.well-known/jwks.json`, cached in-process) and read the
//! `sub` (Hanko user id) + `email` claims.
//!
//! Two extractors:
//! - [`AuthClaims`] — a verified token, nothing more. Used by onboarding
//!   endpoints (`POST /users`, `GET /me`) where the profile row may not exist.
//! - [`CurrentUser`] — a verified token resolved to a `users` row via
//!   `hanko_user_id`. Used by every other route. No row → 403 (complete
//!   onboarding first).
//!
//! Local dev: set `AUTH_DISABLED=true` to skip JWT verification entirely. In
//! that mode [`CurrentUser`] loads the row named by an `X-Debug-User-Id: <uuid>`
//! header and [`AuthClaims`] is rejected (seed a `users` row directly, or point
//! at a real Hanko project, to exercise onboarding locally).

use std::collections::HashMap;
use std::sync::Arc;
use std::time::{Duration, Instant};

use axum::extract::{FromRef, FromRequestParts};
use axum::http::header::AUTHORIZATION;
use axum::http::request::Parts;
use jsonwebtoken::jwk::JwkSet;
use jsonwebtoken::{decode, decode_header, Algorithm, DecodingKey, Validation};
use serde::Deserialize;
use sqlx::PgPool;
use tokio::sync::RwLock;
use uuid::Uuid;

use crate::error::{ApiError, ApiResult};
use crate::models::User;

/// How long a fetched JWKS is trusted before a refresh is forced.
const JWKS_TTL: Duration = Duration::from_secs(3600);
/// Floor between JWKS fetches, so an unknown `kid` can't be used to hammer Hanko.
const JWKS_MIN_REFRESH: Duration = Duration::from_secs(60);

const DEBUG_USER_HEADER: &str = "x-debug-user-id";

#[derive(Debug, thiserror::Error)]
pub enum AuthError {
    #[error("missing bearer token")]
    MissingToken,
    #[error("invalid token: {0}")]
    InvalidToken(String),
    #[error("token signed with an unknown key")]
    UnknownKey,
    #[error("authentication backend unavailable")]
    JwksUnavailable,
    #[error("authentication is not configured")]
    NotConfigured,
}

/// Claims we care about from a Hanko JWT. Hanko has shipped `email` both as a
/// bare string and as an object (`{ address, .. }`); accept either.
#[derive(Debug, Clone, Deserialize)]
pub struct HankoClaims {
    pub sub: String,
    #[serde(default)]
    email: Option<EmailClaim>,
}

#[derive(Debug, Clone, Deserialize)]
#[serde(untagged)]
enum EmailClaim {
    Plain(String),
    Object { address: String },
}

impl HankoClaims {
    pub fn email(&self) -> Option<&str> {
        match &self.email {
            Some(EmailClaim::Plain(s)) => Some(s.as_str()),
            Some(EmailClaim::Object { address }) => Some(address.as_str()),
            None => None,
        }
    }
}

struct JwksCache {
    keys: HashMap<String, DecodingKey>,
    fetched_at: Option<Instant>,
}

/// Hanko JWT verifier with an in-process JWKS cache.
pub struct HankoAuth {
    client: reqwest::Client,
    jwks_url: Option<String>,
    audience: Option<String>,
    /// `AUTH_DISABLED=true` — bypass verification (local dev only).
    pub disabled: bool,
    cache: RwLock<JwksCache>,
}

impl HankoAuth {
    pub fn from_env() -> anyhow::Result<Self> {
        let disabled = env_flag("AUTH_DISABLED");

        let jwks_url = std::env::var("HANKO_JWKS_URL")
            .ok()
            .or_else(|| {
                std::env::var("HANKO_API_URL")
                    .ok()
                    .map(|u| format!("{}/.well-known/jwks.json", u.trim_end_matches('/')))
            })
            .map(|s| s.trim().to_string())
            .filter(|s| !s.is_empty());

        if jwks_url.is_none() && !disabled {
            anyhow::bail!(
                "no Hanko configuration: set HANKO_API_URL (or HANKO_JWKS_URL), \
                 or AUTH_DISABLED=true for local development"
            );
        }

        if disabled {
            tracing::warn!(
                "AUTH_DISABLED=true — JWT verification is bypassed; requests are \
                 identified by the X-Debug-User-Id header"
            );
        } else {
            tracing::info!("auth: Hanko JWKS at {}", jwks_url.as_deref().unwrap_or("?"));
        }

        let audience = std::env::var("HANKO_AUDIENCE")
            .ok()
            .map(|s| s.trim().to_string())
            .filter(|s| !s.is_empty());

        let client = reqwest::Client::builder()
            .user_agent("shelf-circle-backend/0.1")
            .build()?;

        Ok(Self {
            client,
            jwks_url,
            audience,
            disabled,
            cache: RwLock::new(JwksCache {
                keys: HashMap::new(),
                fetched_at: None,
            }),
        })
    }

    /// Best-effort JWKS prefetch at startup. Failure is logged, not fatal — the
    /// first authenticated request will retry.
    pub async fn warm(&self) {
        if self.disabled || self.jwks_url.is_none() {
            return;
        }
        if let Err(e) = self.refresh_jwks().await {
            tracing::warn!("could not prefetch Hanko JWKS: {e}");
        }
    }

    /// Verify a bearer token and return its claims.
    pub async fn verify(&self, token: &str) -> Result<HankoClaims, AuthError> {
        let header = decode_header(token).map_err(|e| AuthError::InvalidToken(e.to_string()))?;
        let kid = header
            .kid
            .ok_or_else(|| AuthError::InvalidToken("no key id in token header".into()))?;

        let mut key = self.cached_key(&kid).await;
        if key.is_none() {
            self.refresh_jwks().await?;
            key = self.cached_key(&kid).await;
        }
        let key = key.ok_or(AuthError::UnknownKey)?;

        let mut validation = Validation::new(Algorithm::RS256);
        match &self.audience {
            Some(aud) => validation.set_audience(std::slice::from_ref(aud)),
            None => validation.validate_aud = false,
        }

        let data = decode::<HankoClaims>(token, &key, &validation)
            .map_err(|e| AuthError::InvalidToken(e.to_string()))?;
        Ok(data.claims)
    }

    async fn cached_key(&self, kid: &str) -> Option<DecodingKey> {
        let cache = self.cache.read().await;
        let fresh = cache.fetched_at.is_some_and(|at| at.elapsed() < JWKS_TTL);
        if fresh {
            cache.keys.get(kid).cloned()
        } else {
            None
        }
    }

    async fn refresh_jwks(&self) -> Result<(), AuthError> {
        let url = self.jwks_url.as_deref().ok_or(AuthError::NotConfigured)?;

        {
            // Throttle: skip if we fetched very recently and have keys.
            let cache = self.cache.read().await;
            if let Some(at) = cache.fetched_at {
                if at.elapsed() < JWKS_MIN_REFRESH && !cache.keys.is_empty() {
                    return Ok(());
                }
            }
        }

        let set: JwkSet = self
            .client
            .get(url)
            .send()
            .await
            .and_then(|r| r.error_for_status())
            .map_err(|e| {
                tracing::error!("JWKS fetch from {url} failed: {e}");
                AuthError::JwksUnavailable
            })?
            .json()
            .await
            .map_err(|e| {
                tracing::error!("JWKS from {url} was not parseable: {e}");
                AuthError::JwksUnavailable
            })?;

        let mut keys = HashMap::new();
        for jwk in &set.keys {
            if let (Some(kid), Ok(dk)) = (jwk.common.key_id.clone(), DecodingKey::from_jwk(jwk)) {
                keys.insert(kid, dk);
            }
        }

        let mut cache = self.cache.write().await;
        cache.keys = keys;
        cache.fetched_at = Some(Instant::now());
        tracing::debug!("loaded {} Hanko signing key(s)", cache.keys.len());
        Ok(())
    }
}

fn env_flag(name: &str) -> bool {
    std::env::var(name)
        .map(|v| matches!(v.trim().to_ascii_lowercase().as_str(), "1" | "true" | "yes"))
        .unwrap_or(false)
}

fn bearer(parts: &Parts) -> Result<&str, AuthError> {
    let raw = parts
        .headers
        .get(AUTHORIZATION)
        .ok_or(AuthError::MissingToken)?
        .to_str()
        .map_err(|_| AuthError::InvalidToken("non-ASCII Authorization header".into()))?;

    raw.strip_prefix("Bearer ")
        .or_else(|| raw.strip_prefix("bearer "))
        .map(str::trim)
        .filter(|t| !t.is_empty())
        .ok_or(AuthError::MissingToken)
}

fn debug_user_id(parts: &Parts) -> ApiResult<Uuid> {
    let raw = parts
        .headers
        .get(DEBUG_USER_HEADER)
        .ok_or_else(|| {
            ApiError::Unauthorized("AUTH_DISABLED: set an X-Debug-User-Id header".into())
        })?
        .to_str()
        .map_err(|_| ApiError::Unauthorized("invalid X-Debug-User-Id header".into()))?;

    Uuid::parse_str(raw.trim())
        .map_err(|_| ApiError::Unauthorized("X-Debug-User-Id must be a user UUID".into()))
}

/// A verified Hanko token. The caller may not have a profile row yet.
pub struct AuthClaims(pub HankoClaims);

impl<S> FromRequestParts<S> for AuthClaims
where
    S: Send + Sync,
    Arc<HankoAuth>: FromRef<S>,
{
    type Rejection = ApiError;

    async fn from_request_parts(parts: &mut Parts, state: &S) -> Result<Self, Self::Rejection> {
        let auth = Arc::<HankoAuth>::from_ref(state);
        if auth.disabled {
            return Err(ApiError::Unauthorized(
                "AUTH_DISABLED: onboarding endpoints require a real Hanko token".into(),
            ));
        }
        let token = bearer(parts)?;
        Ok(AuthClaims(auth.verify(token).await?))
    }
}

/// A verified caller resolved to their `users` row.
pub struct CurrentUser(pub User);

impl<S> FromRequestParts<S> for CurrentUser
where
    S: Send + Sync,
    Arc<HankoAuth>: FromRef<S>,
    PgPool: FromRef<S>,
{
    type Rejection = ApiError;

    async fn from_request_parts(parts: &mut Parts, state: &S) -> Result<Self, Self::Rejection> {
        let auth = Arc::<HankoAuth>::from_ref(state);
        let pool = PgPool::from_ref(state);

        let user = if auth.disabled {
            let id = debug_user_id(parts)?;
            sqlx::query_as::<_, User>(
                "select id, handle, display_name, avatar_url, locale, created_at \
                 from users where id = $1",
            )
            .bind(id)
            .fetch_optional(&pool)
            .await?
        } else {
            let token = bearer(parts)?;
            let claims = auth.verify(token).await?;
            sqlx::query_as::<_, User>(
                "select id, handle, display_name, avatar_url, locale, created_at \
                 from users where hanko_user_id = $1",
            )
            .bind(&claims.sub)
            .fetch_optional(&pool)
            .await?
        };

        user.map(CurrentUser)
            .ok_or_else(|| ApiError::Forbidden("complete onboarding first (POST /users)".into()))
    }
}

/// Guard for `/users/{user_id}/…` routes: the path id must be the caller's own.
pub fn ensure_self(current: &User, path_id: Uuid) -> ApiResult<()> {
    if current.id == path_id {
        Ok(())
    } else {
        Err(ApiError::Forbidden(
            "you can only access your own resources".into(),
        ))
    }
}

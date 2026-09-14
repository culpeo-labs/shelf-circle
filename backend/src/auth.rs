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
    /// Build a verifier directly from already-resolved settings, bypassing the
    /// environment. `jwks_url` must be `Some` unless `disabled` is true.
    /// Split out of [`Self::from_env`] so it (and therefore [`Self::verify`])
    /// can be exercised in tests against a mock JWKS endpoint.
    pub fn new(
        jwks_url: Option<String>,
        audience: Option<String>,
        disabled: bool,
    ) -> anyhow::Result<Self> {
        if jwks_url.is_none() && !disabled {
            anyhow::bail!(
                "no Hanko configuration: set HANKO_API_URL (or HANKO_JWKS_URL), \
                 or AUTH_DISABLED=true for local development"
            );
        }

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

        Self::new(jwks_url, audience, disabled)
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

#[cfg(test)]
mod tests {
    use chrono::Utc;
    use jsonwebtoken::{encode, Algorithm, EncodingKey, Header};
    use serde_json::{json, Value};
    use uuid::Uuid;
    use wiremock::matchers::{method, path};
    use wiremock::{Mock, MockServer, ResponseTemplate};

    use super::*;

    // Throwaway RSA-2048 keypair generated only for these tests (see
    // tests/support/README.md, which carries the same key for the
    // integration tests). Its public half is hard-coded as N/E below.
    const PRIVATE_KEY_PEM: &str = include_str!("../tests/support/rsa_test_key.pem");
    const KID: &str = "test-key-1";
    const N: &str = "2cSkXaigrKoyCI9iESnXb8mhFXIt4echAPq56nlZtL0Hf92lFs7zBnfxi4QiREJGGM77x1bRHYfYC4GWhN6SXhTDpe-RE6m_ad3gdKObBpjoWSzPEO8BclY3188yyMxrHqGfXuMRfiiaKWiXk-7H5S5sILjUt8SjVhF4mHS6Zh3yB93Tv_LV0y0C9x6ZRnrV7rvn4qrDGeKQGBSDh75Bo5Rn8ZOj85slk81AfkWDaYPddPm7CTD4A29f9hyDjQAAvcSe6W23gRP_hexqPb5H4dHyM_JPgKnWTpMV1yA6mQZkqtyoOesxlEQwD5OIze-vXsaruD2NResDFfQYEEAWQw";
    const E: &str = "AQAB";

    fn jwks_json() -> Value {
        json!({
            "keys": [{
                "kty": "RSA",
                "use": "sig",
                "kid": KID,
                "alg": "RS256",
                "n": N,
                "e": E,
            }]
        })
    }

    async fn mock_jwks_server() -> MockServer {
        let server = MockServer::start().await;
        Mock::given(method("GET"))
            .and(path("/jwks.json"))
            .respond_with(ResponseTemplate::new(200).set_body_json(jwks_json()))
            .mount(&server)
            .await;
        server
    }

    fn sign(kid: &str, claims: Value) -> String {
        let mut header = Header::new(Algorithm::RS256);
        header.kid = Some(kid.to_string());
        let key =
            EncodingKey::from_rsa_pem(PRIVATE_KEY_PEM.as_bytes()).expect("parse test RSA key");
        encode(&header, &claims, &key).expect("sign token")
    }

    fn claims_with_exp() -> Value {
        json!({ "sub": "hanko|abc", "exp": Utc::now().timestamp() + 3600 })
    }

    // ---------- HankoClaims / email deserialization ----------
    // Hanko has shipped `email` as both a bare string and `{ address }`; the
    // untagged `EmailClaim` enum is exactly the kind of thing a serde bump
    // could silently change the behavior of.

    #[test]
    fn email_claim_accepts_plain_string() {
        let claims: HankoClaims =
            serde_json::from_value(json!({ "sub": "u1", "email": "a@example.com" })).unwrap();
        assert_eq!(claims.email(), Some("a@example.com"));
    }

    #[test]
    fn email_claim_accepts_object_shape() {
        let claims: HankoClaims = serde_json::from_value(json!({
            "sub": "u1",
            "email": { "address": "a@example.com", "is_verified": true }
        }))
        .unwrap();
        assert_eq!(claims.email(), Some("a@example.com"));
    }

    #[test]
    fn email_claim_defaults_to_none_when_absent() {
        let claims: HankoClaims = serde_json::from_value(json!({ "sub": "u1" })).unwrap();
        assert_eq!(claims.email(), None);
    }

    // ---------- ensure_self ----------

    fn user_with_id(id: Uuid) -> User {
        User {
            id,
            handle: "h".into(),
            display_name: "H".into(),
            avatar_url: None,
            locale: "en".into(),
            created_at: Utc::now(),
        }
    }

    #[test]
    fn ensure_self_allows_matching_id() {
        let id = Uuid::new_v4();
        assert!(ensure_self(&user_with_id(id), id).is_ok());
    }

    #[test]
    fn ensure_self_rejects_mismatched_id() {
        let user = user_with_id(Uuid::new_v4());
        let err = ensure_self(&user, Uuid::new_v4()).unwrap_err();
        assert!(matches!(err, ApiError::Forbidden(_)));
    }

    // ---------- env_flag ----------

    #[test]
    fn env_flag_parses_truthy_and_falsy_values() {
        for truthy in ["1", "true", "TRUE", "yes", " yes "] {
            std::env::set_var("SC_TEST_FLAG", truthy);
            assert!(env_flag("SC_TEST_FLAG"), "expected {truthy:?} to be truthy");
        }
        for falsy in ["0", "false", "no", "", "on"] {
            std::env::set_var("SC_TEST_FLAG", falsy);
            assert!(!env_flag("SC_TEST_FLAG"), "expected {falsy:?} to be falsy");
        }
        std::env::remove_var("SC_TEST_FLAG");
        assert!(!env_flag("SC_TEST_FLAG"), "unset var is falsy");
    }

    // ---------- HankoAuth::verify (against a mock JWKS endpoint) ----------

    #[tokio::test]
    async fn verify_accepts_a_correctly_signed_known_key_token() {
        let server = mock_jwks_server().await;
        let auth = HankoAuth::new(Some(format!("{}/jwks.json", server.uri())), None, false)
            .expect("build HankoAuth");

        let token = sign(KID, claims_with_exp());
        let claims = auth.verify(&token).await.expect("token should verify");
        assert_eq!(claims.sub, "hanko|abc");
    }

    #[tokio::test]
    async fn verify_fetches_jwks_lazily_on_first_use() {
        // No `warm()` call — the first `verify` should trigger the fetch itself.
        let server = mock_jwks_server().await;
        let auth = HankoAuth::new(Some(format!("{}/jwks.json", server.uri())), None, false)
            .expect("build HankoAuth");

        let token = sign(KID, claims_with_exp());
        assert!(auth.verify(&token).await.is_ok());
    }

    #[tokio::test]
    async fn verify_rejects_unknown_kid() {
        let server = mock_jwks_server().await;
        let auth = HankoAuth::new(Some(format!("{}/jwks.json", server.uri())), None, false)
            .expect("build HankoAuth");

        let token = sign("some-other-kid", claims_with_exp());
        let err = auth.verify(&token).await.unwrap_err();
        assert!(matches!(err, AuthError::UnknownKey));
    }

    #[tokio::test]
    async fn verify_rejects_expired_token() {
        let server = mock_jwks_server().await;
        let auth = HankoAuth::new(Some(format!("{}/jwks.json", server.uri())), None, false)
            .expect("build HankoAuth");

        // `jsonwebtoken`'s default `Validation` allows a 60s leeway, so this
        // needs to be well past expired, not just in the past.
        let token = sign(
            KID,
            json!({ "sub": "hanko|abc", "exp": Utc::now().timestamp() - 300 }),
        );
        let err = auth.verify(&token).await.unwrap_err();
        assert!(matches!(err, AuthError::InvalidToken(_)));
    }

    #[tokio::test]
    async fn verify_rejects_token_missing_exp_claim() {
        let server = mock_jwks_server().await;
        let auth = HankoAuth::new(Some(format!("{}/jwks.json", server.uri())), None, false)
            .expect("build HankoAuth");

        let token = sign(KID, json!({ "sub": "hanko|abc" }));
        let err = auth.verify(&token).await.unwrap_err();
        assert!(matches!(err, AuthError::InvalidToken(_)));
    }

    #[tokio::test]
    async fn verify_enforces_configured_audience() {
        let server = mock_jwks_server().await;
        let auth = HankoAuth::new(
            Some(format!("{}/jwks.json", server.uri())),
            Some("shelf-circle-app".to_string()),
            false,
        )
        .expect("build HankoAuth");

        let mut claims = claims_with_exp();
        claims["aud"] = json!("some-other-app");
        let token = sign(KID, claims);
        let err = auth.verify(&token).await.unwrap_err();
        assert!(matches!(err, AuthError::InvalidToken(_)));
    }

    #[tokio::test]
    async fn verify_fails_closed_when_jwks_endpoint_is_unreachable() {
        // A JWKS URL that answers 500 simulates Hanko being down: verification
        // must fail, not silently accept the token.
        let server = MockServer::start().await;
        Mock::given(method("GET"))
            .and(path("/jwks.json"))
            .respond_with(ResponseTemplate::new(500))
            .mount(&server)
            .await;
        let auth = HankoAuth::new(Some(format!("{}/jwks.json", server.uri())), None, false)
            .expect("build HankoAuth");

        let token = sign(KID, claims_with_exp());
        let err = auth.verify(&token).await.unwrap_err();
        assert!(matches!(err, AuthError::JwksUnavailable));
    }

    #[test]
    fn new_requires_jwks_url_unless_disabled() {
        assert!(HankoAuth::new(None, None, false).is_err());
        assert!(HankoAuth::new(None, None, true).is_ok());
        assert!(
            HankoAuth::new(Some("http://example.invalid/jwks.json".into()), None, true).is_ok()
        );
    }
}

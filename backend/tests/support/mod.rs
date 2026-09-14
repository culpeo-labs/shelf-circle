//! Shared harness for the black-box integration tests in this directory:
//! an ephemeral Postgres database per test (so tests can run concurrently
//! without stepping on each other), a mock Hanko JWKS endpoint, and a helper
//! to sign JWTs against the matching test key so tests can drive the real
//! `Authorization: Bearer` path end to end.
//!
//! `rsa_test_key.pem` is a throwaway keypair generated only for these tests
//! (see `README.md` next to it) — its public half is hard-coded below as
//! `TEST_N`/`TEST_E`.

use axum::body::Body;
use axum::http::{Request, StatusCode};
use axum::Router;
use http_body_util::BodyExt;
use jsonwebtoken::{encode, Algorithm, EncodingKey, Header};
use serde_json::{json, Value};
use sqlx::postgres::PgPoolOptions;
use sqlx::PgPool;
use tower::ServiceExt;
use uuid::Uuid;
use wiremock::matchers::{method, path};
use wiremock::{Mock, MockServer, ResponseTemplate};

use shelf_circle_backend::auth::HankoAuth;
use shelf_circle_backend::providers::BookProviders;
use shelf_circle_backend::state::AppState;

const TEST_PRIVATE_KEY_PEM: &str = include_str!("rsa_test_key.pem");
const TEST_KID: &str = "test-key-1";
const TEST_N: &str = "2cSkXaigrKoyCI9iESnXb8mhFXIt4echAPq56nlZtL0Hf92lFs7zBnfxi4QiREJGGM77x1bRHYfYC4GWhN6SXhTDpe-RE6m_ad3gdKObBpjoWSzPEO8BclY3188yyMxrHqGfXuMRfiiaKWiXk-7H5S5sILjUt8SjVhF4mHS6Zh3yB93Tv_LV0y0C9x6ZRnrV7rvn4qrDGeKQGBSDh75Bo5Rn8ZOj85slk81AfkWDaYPddPm7CTD4A29f9hyDjQAAvcSe6W23gRP_hexqPb5H4dHyM_JPgKnWTpMV1yA6mQZkqtyoOesxlEQwD5OIze-vXsaruD2NResDFfQYEEAWQw";
const TEST_E: &str = "AQAB";

/// JWKS document exposing the test public key, in the shape Hanko (and
/// `jsonwebtoken::jwk`) expect.
fn jwks_json() -> Value {
    json!({
        "keys": [{
            "kty": "RSA",
            "use": "sig",
            "kid": TEST_KID,
            "alg": "RS256",
            "n": TEST_N,
            "e": TEST_E,
        }]
    })
}

/// Starts a mock server that answers `GET /.well-known/jwks.json` with the
/// test key set, and returns its base URL (the `HANKO_JWKS_URL` shape).
pub async fn mock_jwks_server() -> MockServer {
    let server = MockServer::start().await;
    Mock::given(method("GET"))
        .and(path("/.well-known/jwks.json"))
        .respond_with(ResponseTemplate::new(200).set_body_json(jwks_json()))
        .mount(&server)
        .await;
    server
}

/// An `exp` an hour from now, in the Unix-timestamp shape JWT expects.
/// `jsonwebtoken`'s default `Validation` requires this claim to be present
/// (and in the future) even though nothing here calls `set_required_spec_claims`.
pub fn future_exp() -> i64 {
    chrono::Utc::now().timestamp() + 3600
}

/// Signs a JWT with the test private key, `kid` set so it matches
/// [`mock_jwks_server`]'s response. `claims` is merged as-is, so callers
/// control `sub`/`email`/`exp`/anything else Hanko might send — see
/// [`future_exp`] for a claim that passes default validation.
pub fn sign_token(claims: Value) -> String {
    let mut header = Header::new(Algorithm::RS256);
    header.kid = Some(TEST_KID.to_string());
    let key = EncodingKey::from_rsa_pem(TEST_PRIVATE_KEY_PEM.as_bytes())
        .expect("parse test RSA private key");
    encode(&header, &claims, &key).expect("sign test JWT")
}

fn with_database(admin_url: &str, db_name: &str) -> String {
    let (base, query) = match admin_url.split_once('?') {
        Some((b, q)) => (b, Some(q)),
        None => (admin_url, None),
    };
    let base = match base.rsplit_once('/') {
        Some((prefix, _old_db)) => format!("{prefix}/{db_name}"),
        None => format!("{base}/{db_name}"),
    };
    match query {
        Some(q) => format!("{base}?{q}"),
        None => base,
    }
}

/// An ephemeral, fully-migrated Postgres database. Each test gets its own, so
/// tests can run in parallel without sharing rows (handles, friendships, …).
/// Best-effort dropped when the test finishes (see `Drop` below); a leftover
/// `t_…` database after a crashed run is harmless and safe to delete by hand.
pub struct TestDb {
    pub pool: PgPool,
    admin_pool: PgPool,
    db_name: String,
}

impl TestDb {
    pub async fn new() -> Self {
        let admin_url = std::env::var("TEST_DATABASE_ADMIN_URL")
            .unwrap_or_else(|_| "postgres://postgres:postgres@localhost:5432/postgres".to_string());

        let admin_pool = PgPoolOptions::new()
            .max_connections(2)
            .connect(&admin_url)
            .await
            .expect(
                "connect to the admin Postgres database for tests \
                 (is Postgres running on localhost:5432? set TEST_DATABASE_ADMIN_URL to override)",
            );

        let db_name = format!("t_{}", Uuid::new_v4().simple());
        // `db_name` is our own `t_<uuid hex>`, never external input, so this
        // dynamic DDL is safe despite not being a `&'static str`.
        sqlx::query(sqlx::AssertSqlSafe(format!(
            r#"create database "{db_name}""#
        )))
        .execute(&admin_pool)
        .await
        .expect("create ephemeral test database");

        let db_url = with_database(&admin_url, &db_name);
        let pool = shelf_circle_backend::db::connect(&db_url)
            .await
            .expect("connect to and migrate ephemeral test database");

        Self {
            pool,
            admin_pool,
            db_name,
        }
    }
}

impl Drop for TestDb {
    fn drop(&mut self) {
        // Can't `.await` in `Drop`; fire-and-forget the cleanup on the
        // current Tokio runtime if there is one. Best-effort only — a test
        // process exiting right after this may not give it time to finish.
        if let Ok(handle) = tokio::runtime::Handle::try_current() {
            let admin_pool = self.admin_pool.clone();
            let db_name = self.db_name.clone();
            handle.spawn(async move {
                let _ = sqlx::query(
                    "select pg_terminate_backend(pid) from pg_stat_activity \
                     where datname = $1 and pid <> pg_backend_pid()",
                )
                .bind(&db_name)
                .execute(&admin_pool)
                .await;
                let _ = sqlx::query(sqlx::AssertSqlSafe(format!(
                    r#"drop database if exists "{db_name}""#
                )))
                .execute(&admin_pool)
                .await;
            });
        }
    }
}

/// A running app plus everything needed to talk to it as an authenticated
/// caller: an isolated database and a mock JWKS endpoint backing real JWT
/// verification (the production `Authorization: Bearer` path, not the
/// `AUTH_DISABLED` debug shortcut).
pub struct TestApp {
    pub router: Router,
    // Kept for callers that want direct DB access (e.g. to seed data an HTTP
    // call can't produce) and to keep the ephemeral database alive for the
    // app's lifetime — not read by every test.
    #[allow(dead_code)]
    pub db: TestDb,
    // Kept alive for the app's lifetime — dropping it would stop answering
    // JWKS requests out from under a still-running HankoAuth JWKS cache.
    _jwks_server: MockServer,
}

impl TestApp {
    pub async fn new() -> Self {
        let db = TestDb::new().await;
        let jwks_server = mock_jwks_server().await;

        let auth = HankoAuth::new(
            Some(format!("{}/.well-known/jwks.json", jwks_server.uri())),
            None,
            false,
        )
        .expect("build HankoAuth against the mock JWKS server");
        auth.warm().await;

        let state = AppState {
            pool: db.pool.clone(),
            providers: std::sync::Arc::new(BookProviders::from_env()),
            auth: std::sync::Arc::new(auth),
        };

        Self {
            router: shelf_circle_backend::app(state),
            db,
            _jwks_server: jwks_server,
        }
    }

    /// A bearer token for a fresh Hanko identity (`sub`); no `users` row
    /// exists for it yet, so it authenticates but needs `POST /users` first.
    pub fn token_for_new_hanko_user(&self, sub: &str, email: &str) -> String {
        sign_token(json!({ "sub": sub, "email": email, "exp": future_exp() }))
    }
}

/// Sends `req` through the app and returns the status plus the parsed JSON
/// body (`Value::Null` if the body is empty or not JSON).
pub async fn send(router: &Router, req: Request<Body>) -> (StatusCode, Value) {
    let response = router
        .clone()
        .oneshot(req)
        .await
        .expect("router is infallible");
    let status = response.status();
    let bytes = response
        .into_body()
        .collect()
        .await
        .expect("collect response body")
        .to_bytes();
    // Most routes return JSON; `/health` is plain text, so fall back to the
    // raw string rather than failing every non-JSON response.
    let body = if bytes.is_empty() {
        Value::Null
    } else {
        serde_json::from_slice(&bytes)
            .unwrap_or_else(|_| Value::String(String::from_utf8_lossy(&bytes).into_owned()))
    };
    (status, body)
}

pub fn json_request(method: &str, uri: &str, bearer: Option<&str>, body: Value) -> Request<Body> {
    let mut builder = Request::builder()
        .method(method)
        .uri(uri)
        .header("content-type", "application/json");
    if let Some(token) = bearer {
        builder = builder.header("authorization", format!("Bearer {token}"));
    }
    builder.body(Body::from(body.to_string())).unwrap()
}

pub fn get_request(uri: &str, bearer: Option<&str>) -> Request<Body> {
    let mut builder = Request::builder().method("GET").uri(uri);
    if let Some(token) = bearer {
        builder = builder.header("authorization", format!("Bearer {token}"));
    }
    builder.body(Body::empty()).unwrap()
}

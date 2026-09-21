//! Black-box HTTP tests: build the real router (`shelf_circle_backend::app`)
//! over an ephemeral, fully-migrated Postgres database and a mock Hanko JWKS
//! endpoint, then drive it exactly as a client would — real signed JWTs,
//! real SQL, real (de)serialization. This is the safety net for dependency
//! bumps (axum, sqlx, serde, jsonwebtoken, …): if a bump changes how routing,
//! extraction, query execution, or JSON shapes behave, these tests are where
//! it should show up.
//!
//! Every test builds its own [`support::TestApp`] (own database, own mock
//! JWKS server), so tests are independent and safe to run in parallel — the
//! default `cargo test` behavior.
//!
//! Requires a reachable Postgres superuser to create/drop the ephemeral test
//! databases; see `support::TestDb` for how to point that elsewhere.

mod support;

use axum::http::StatusCode;
use serde_json::{json, Value};
use uuid::Uuid;

use support::{get_request, json_request, send, TestApp};

#[tokio::test]
async fn health_check_does_not_require_auth() {
    let app = TestApp::new().await;
    let (status, _) = send(&app.router, get_request("/health", None)).await;
    assert_eq!(status, StatusCode::OK);
}

#[tokio::test]
async fn protected_routes_reject_missing_or_bad_tokens() {
    let app = TestApp::new().await;

    let (status, _) = send(&app.router, get_request("/me", None)).await;
    assert_eq!(status, StatusCode::UNAUTHORIZED, "no Authorization header");

    let mut req = get_request("/me", None);
    req.headers_mut()
        .insert("authorization", "Bearer not-a-jwt".parse().unwrap());
    let (status, _) = send(&app.router, req).await;
    assert_eq!(status, StatusCode::UNAUTHORIZED, "garbage token");
}

/// Onboard through `POST /users` with a real signed token, then confirm
/// `GET /me` resolves the same identity and a second onboarding attempt (same
/// Hanko `sub`, or a colliding handle) is rejected as a conflict.
#[tokio::test]
async fn onboarding_then_profile_lookup() {
    let app = TestApp::new().await;
    let token = app.token_for_new_hanko_user("hanko|user-1", "reader@example.com");

    let (status, body) = send(
        &app.router,
        json_request(
            "POST",
            "/users",
            Some(&token),
            json!({ "handle": "alice", "display_name": "Alice" }),
        ),
    )
    .await;
    assert_eq!(status, StatusCode::OK, "body: {body}");
    assert_eq!(body["handle"], "alice");
    assert_eq!(body["display_name"], "Alice");
    assert_eq!(body["locale"], "en", "locale defaults to en when omitted");
    let user_id = body["id"].as_str().expect("id is a string").to_string();

    let (status, body) = send(&app.router, get_request("/me", Some(&token))).await;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(body["id"], user_id);

    // Same token onboarding again -> duplicate hanko_user_id.
    let (status, _) = send(
        &app.router,
        json_request(
            "POST",
            "/users",
            Some(&token),
            json!({ "handle": "alice-2", "display_name": "Alice" }),
        ),
    )
    .await;
    assert_eq!(status, StatusCode::CONFLICT, "same identity onboards twice");

    // A different identity claiming the same handle -> duplicate handle.
    let other_token = app.token_for_new_hanko_user("hanko|user-2", "other@example.com");
    let (status, _) = send(
        &app.router,
        json_request(
            "POST",
            "/users",
            Some(&other_token),
            json!({ "handle": "alice", "display_name": "Someone Else" }),
        ),
    )
    .await;
    assert_eq!(status, StatusCode::CONFLICT, "handle already taken");

    // A token that has never onboarded gets 403 (not 500/404) from a
    // CurrentUser-gated route.
    let (status, _) = send(
        &app.router,
        get_request(&format!("/users/{user_id}/library"), Some(&other_token)),
    )
    .await;
    assert_eq!(status, StatusCode::FORBIDDEN);
}

async fn onboard(app: &TestApp, sub: &str, handle: &str) -> (String, String) {
    let token = app.token_for_new_hanko_user(sub, &format!("{handle}@example.com"));
    let (status, body) = send(
        &app.router,
        json_request(
            "POST",
            "/users",
            Some(&token),
            json!({ "handle": handle, "display_name": handle }),
        ),
    )
    .await;
    assert_eq!(status, StatusCode::OK, "onboarding {handle}: {body}");
    (token, body["id"].as_str().unwrap().to_string())
}

#[tokio::test]
async fn users_lookup_and_ensure_self_guard() {
    let app = TestApp::new().await;
    let (token_a, id_a) = onboard(&app, "hanko|a", "alice").await;
    let (_token_b, id_b) = onboard(&app, "hanko|b", "bob").await;

    let (status, body) = send(
        &app.router,
        get_request(&format!("/users/{id_b}"), Some(&token_a)),
    )
    .await;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(body["handle"], "bob");

    let (status, body) = send(
        &app.router,
        get_request("/users/by-handle/bob", Some(&token_a)),
    )
    .await;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(body["id"], id_b);

    let missing = Uuid::new_v4();
    let (status, _) = send(
        &app.router,
        get_request(&format!("/users/{missing}"), Some(&token_a)),
    )
    .await;
    assert_eq!(status, StatusCode::NOT_FOUND);

    // A cannot read B's own-resources routes even though both are onboarded.
    let (status, _) = send(
        &app.router,
        get_request(&format!("/users/{id_b}/library"), Some(&token_a)),
    )
    .await;
    assert_eq!(status, StatusCode::FORBIDDEN);
    let (status, _) = send(
        &app.router,
        get_request(&format!("/users/{id_a}/library"), Some(&token_a)),
    )
    .await;
    assert_eq!(status, StatusCode::OK, "reading your own library is fine");
}

#[tokio::test]
async fn friendships_are_canonicalized_and_validated() {
    let app = TestApp::new().await;
    let (token_a, id_a) = onboard(&app, "hanko|a", "alice").await;
    let (_token_b, id_b) = onboard(&app, "hanko|b", "bob").await;

    let (status, body) = send(
        &app.router,
        json_request(
            "POST",
            "/friendships",
            Some(&token_a),
            json!({ "user_handle": "bob" }),
        ),
    )
    .await;
    assert_eq!(status, StatusCode::OK, "body: {body}");
    let (lo, hi) = if id_a < id_b {
        (&id_a, &id_b)
    } else {
        (&id_b, &id_a)
    };
    assert_eq!(body["user_a_id"], *lo);
    assert_eq!(body["user_b_id"], *hi);

    // Idempotent: creating it again succeeds rather than erroring.
    let (status, _) = send(
        &app.router,
        json_request(
            "POST",
            "/friendships",
            Some(&token_a),
            json!({ "user_handle": "bob" }),
        ),
    )
    .await;
    assert_eq!(status, StatusCode::OK);

    let (status, _) = send(
        &app.router,
        json_request(
            "POST",
            "/friendships",
            Some(&token_a),
            json!({ "user_handle": "alice" }),
        ),
    )
    .await;
    assert_eq!(status, StatusCode::BAD_REQUEST, "cannot friend yourself");

    let (status, _) = send(
        &app.router,
        json_request(
            "POST",
            "/friendships",
            Some(&token_a),
            json!({ "user_handle": "nobody-such-handle" }),
        ),
    )
    .await;
    assert_eq!(status, StatusCode::BAD_REQUEST, "unknown handle");
}

fn normalized_book(source_id: &str, work_id: &str, edition_title: &str) -> Value {
    json!({
        "canonical_title": "Test Book",
        "primary_author": "Author One",
        "language": "en",
        "isbn_13": null,
        "isbn_10": null,
        "edition_title": edition_title,
        "publisher": "Test Publisher",
        "cover_image_url": null,
        "source": "open_library",
        "source_id": source_id,
        "open_library_work_id": work_id,
        "google_books_volume_id": null,
    })
}

/// `/books/resolve`'s upsert has two dedup paths tested here: an
/// already-seen (source, source_id) returns the same edition, and a second
/// edition of an already-seen work (same `open_library_work_id`, new
/// `source_id`) attaches to the same book instead of creating a new one.
#[tokio::test]
async fn books_resolve_dedup_and_get() {
    let app = TestApp::new().await;
    let (token, _id) = onboard(&app, "hanko|a", "alice").await;

    let (status, first) = send(
        &app.router,
        json_request(
            "POST",
            "/books/resolve",
            Some(&token),
            normalized_book("OL1W", "OL1W", "Test Book"),
        ),
    )
    .await;
    assert_eq!(status, StatusCode::OK, "body: {first}");
    let book_id = first["id"].as_str().unwrap().to_string();
    let edition_id = first["edition"]["id"].as_str().unwrap().to_string();

    // Re-resolving the exact same reference returns the same book+edition.
    let (status, again) = send(
        &app.router,
        json_request(
            "POST",
            "/books/resolve",
            Some(&token),
            normalized_book("OL1W", "OL1W", "Test Book"),
        ),
    )
    .await;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(again["id"], book_id);
    assert_eq!(again["edition"]["id"], edition_id);

    // A different edition of the same work attaches to the same book.
    let (status, translation) = send(
        &app.router,
        json_request(
            "POST",
            "/books/resolve",
            Some(&token),
            normalized_book("OL1W-es", "OL1W", "Libro de Prueba"),
        ),
    )
    .await;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(
        translation["id"], book_id,
        "same work id merges into the existing book"
    );
    assert_ne!(translation["edition"]["id"], edition_id);

    let (status, fetched) = send(
        &app.router,
        get_request(&format!("/books/{book_id}"), Some(&token)),
    )
    .await;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(fetched["canonical_title"], "Test Book");

    let missing = Uuid::new_v4();
    let (status, _) = send(
        &app.router,
        get_request(&format!("/books/{missing}"), Some(&token)),
    )
    .await;
    assert_eq!(status, StatusCode::NOT_FOUND);
}

async fn resolve_book(app: &TestApp, token: &str, source_id: &str) -> String {
    let (status, body) = send(
        &app.router,
        json_request(
            "POST",
            "/books/resolve",
            Some(token),
            normalized_book(source_id, source_id, "Test Book"),
        ),
    )
    .await;
    assert_eq!(status, StatusCode::OK, "body: {body}");
    body["id"].as_str().unwrap().to_string()
}

/// Rating is only accepted alongside `finished`/`did_not_finish`, must be
/// 1-5, and setting a status is an upsert (one row per user/book).
#[tokio::test]
async fn book_status_rating_rules_and_upsert() {
    let app = TestApp::new().await;
    let (token, user_id) = onboard(&app, "hanko|a", "alice").await;
    let book_id = resolve_book(&app, &token, "OL1W").await;

    let (status, _) = send(
        &app.router,
        json_request(
            "PUT",
            "/book-statuses",
            Some(&token),
            json!({ "book_id": book_id, "status": "want_to_read", "rating": 4 }),
        ),
    )
    .await;
    assert_eq!(
        status,
        StatusCode::BAD_REQUEST,
        "rating only allowed with finished / did_not_finish"
    );

    let (status, _) = send(
        &app.router,
        json_request(
            "PUT",
            "/book-statuses",
            Some(&token),
            json!({ "book_id": book_id, "status": "finished", "rating": 6 }),
        ),
    )
    .await;
    assert_eq!(status, StatusCode::BAD_REQUEST, "rating out of 1..=5 range");

    let (status, started) = send(
        &app.router,
        json_request(
            "PUT",
            "/book-statuses",
            Some(&token),
            json!({ "book_id": book_id, "status": "currently_reading", "progress_percent": 10 }),
        ),
    )
    .await;
    assert_eq!(status, StatusCode::OK, "body: {started}");
    assert_eq!(started["status"], "currently_reading");
    assert_eq!(started["rating"], Value::Null);

    let (status, finished) = send(
        &app.router,
        json_request(
            "PUT",
            "/book-statuses",
            Some(&token),
            json!({ "book_id": book_id, "status": "finished", "rating": 5 }),
        ),
    )
    .await;
    assert_eq!(status, StatusCode::OK, "body: {finished}");
    assert_eq!(finished["status"], "finished");
    assert_eq!(finished["rating"], 5);
    assert_eq!(
        finished["id"], started["id"],
        "same user+book upserts one row, not a second"
    );

    let (status, list) = send(
        &app.router,
        get_request(&format!("/users/{user_id}/book-statuses"), Some(&token)),
    )
    .await;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(list.as_array().unwrap().len(), 1);
}

#[tokio::test]
async fn library_shelves_filter_and_validate() {
    let app = TestApp::new().await;
    let (token, user_id) = onboard(&app, "hanko|a", "alice").await;
    let finished_book = resolve_book(&app, &token, "OL-finished").await;
    let unread_book = resolve_book(&app, &token, "OL-unread").await;

    send(
        &app.router,
        json_request(
            "PUT",
            "/book-statuses",
            Some(&token),
            json!({ "book_id": finished_book, "status": "finished", "rating": 3 }),
        ),
    )
    .await;
    send(
        &app.router,
        json_request(
            "PUT",
            "/book-statuses",
            Some(&token),
            json!({ "book_id": unread_book, "status": "want_to_read" }),
        ),
    )
    .await;

    let (status, all) = send(
        &app.router,
        get_request(&format!("/users/{user_id}/library"), Some(&token)),
    )
    .await;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(all.as_array().unwrap().len(), 2);

    let (status, read) = send(
        &app.router,
        get_request(
            &format!("/users/{user_id}/library?shelf=read"),
            Some(&token),
        ),
    )
    .await;
    assert_eq!(status, StatusCode::OK);
    let read = read.as_array().unwrap();
    assert_eq!(read.len(), 1);
    assert_eq!(read[0]["book"]["id"], finished_book);

    let (status, _) = send(
        &app.router,
        get_request(
            &format!("/users/{user_id}/library?shelf=not-a-real-shelf"),
            Some(&token),
        ),
    )
    .await;
    assert_eq!(status, StatusCode::BAD_REQUEST);
}

/// Friends' reading-status changes show up in each other's feed (via the
/// `activity_events` trigger), but a stranger's don't.
#[tokio::test]
async fn feed_shows_friends_activity_only() {
    let app = TestApp::new().await;
    let (token_a, id_a) = onboard(&app, "hanko|a", "alice").await;
    let (token_b, id_b) = onboard(&app, "hanko|b", "bob").await;
    let (token_c, _id_c) = onboard(&app, "hanko|c", "carol").await;

    send(
        &app.router,
        json_request(
            "POST",
            "/friendships",
            Some(&token_a),
            json!({ "user_handle": "bob" }),
        ),
    )
    .await;

    let book_b = resolve_book(&app, &token_b, "OL-b").await;
    send(
        &app.router,
        json_request(
            "PUT",
            "/book-statuses",
            Some(&token_b),
            json!({ "book_id": book_b, "status": "currently_reading" }),
        ),
    )
    .await;

    let book_c = resolve_book(&app, &token_c, "OL-c").await;
    send(
        &app.router,
        json_request(
            "PUT",
            "/book-statuses",
            Some(&token_c),
            json!({ "book_id": book_c, "status": "currently_reading" }),
        ),
    )
    .await;

    let (status, feed) = send(
        &app.router,
        get_request(&format!("/users/{id_a}/feed"), Some(&token_a)),
    )
    .await;
    assert_eq!(status, StatusCode::OK, "body: {feed}");
    let feed = feed.as_array().unwrap();
    assert_eq!(
        feed.len(),
        1,
        "only the friend's activity, not the stranger's"
    );
    assert_eq!(feed[0]["actor"]["id"], id_b);
    assert_eq!(feed[0]["verb"], "started reading");
}

#[tokio::test]
async fn recommendations_go_to_the_right_inbox() {
    let app = TestApp::new().await;
    let (token_a, _id_a) = onboard(&app, "hanko|a", "alice").await;
    let (token_b, id_b) = onboard(&app, "hanko|b", "bob").await;
    let book = resolve_book(&app, &token_a, "OL-rec").await;

    let (status, rec) = send(
        &app.router,
        json_request(
            "POST",
            "/recommendations",
            Some(&token_a),
            json!({ "to_user_id": id_b, "book_id": book, "note": "you'd love this" }),
        ),
    )
    .await;
    assert_eq!(status, StatusCode::OK, "body: {rec}");

    let (status, inbox_b) = send(
        &app.router,
        get_request(
            &format!("/users/{id_b}/recommendations/inbox"),
            Some(&token_b),
        ),
    )
    .await;
    assert_eq!(status, StatusCode::OK);
    let inbox_b = inbox_b.as_array().unwrap();
    assert_eq!(inbox_b.len(), 1);
    assert_eq!(inbox_b[0]["note"], "you'd love this");

    // Bob cannot read Alice's inbox.
    let (status, _) = send(
        &app.router,
        get_request(
            &format!("/users/{id_b}/recommendations/inbox"),
            Some(&token_a),
        ),
    )
    .await;
    assert_eq!(status, StatusCode::FORBIDDEN);
}

#[tokio::test]
async fn invites_full_flow() {
    let app = TestApp::new().await;
    let (token_a, id_a) = onboard(&app, "hanko|a", "alice").await;
    let (token_b, id_b) = onboard(&app, "hanko|b", "bob").await;

    let (status, invite) = send(
        &app.router,
        json_request("POST", "/invites", Some(&token_a), json!({})),
    )
    .await;
    assert_eq!(status, StatusCode::OK, "body: {invite}");
    let token = invite["token"].as_str().unwrap().to_string();
    assert!(!invite["expires_at"].is_null());

    // Public: no Authorization header needed to preview it.
    let (status, preview) =
        send(&app.router, get_request(&format!("/invites/{token}"), None)).await;
    assert_eq!(status, StatusCode::OK, "body: {preview}");
    assert_eq!(preview["display_name"], "alice");
    assert!(preview.get("handle").is_none(), "handle must not leak");

    // Unknown token previews as 404.
    let (status, _) = send(&app.router, get_request("/invites/does-not-exist", None)).await;
    assert_eq!(status, StatusCode::NOT_FOUND);

    // Alice cannot accept her own invite.
    let (status, _) = send(
        &app.router,
        json_request(
            "POST",
            &format!("/invites/{token}/accept"),
            Some(&token_a),
            json!({}),
        ),
    )
    .await;
    assert_eq!(
        status,
        StatusCode::BAD_REQUEST,
        "cannot accept your own invite"
    );

    // Bob accepts it: creates the canonicalized friendship.
    let (status, friendship) = send(
        &app.router,
        json_request(
            "POST",
            &format!("/invites/{token}/accept"),
            Some(&token_b),
            json!({}),
        ),
    )
    .await;
    assert_eq!(status, StatusCode::OK, "body: {friendship}");
    let (lo, hi) = if id_a < id_b {
        (&id_a, &id_b)
    } else {
        (&id_b, &id_a)
    };
    assert_eq!(friendship["user_a_id"], *lo);
    assert_eq!(friendship["user_b_id"], *hi);

    // Single-use: the same token can't be accepted again, by anyone.
    let (status, _) = send(
        &app.router,
        json_request(
            "POST",
            &format!("/invites/{token}/accept"),
            Some(&token_b),
            json!({}),
        ),
    )
    .await;
    assert_eq!(status, StatusCode::BAD_REQUEST, "already used");

    // ...and it stops previewing too, once used.
    let (status, _) = send(&app.router, get_request(&format!("/invites/{token}"), None)).await;
    assert_eq!(
        status,
        StatusCode::NOT_FOUND,
        "used invite no longer previews"
    );
}

/// The self-accept rejection happens *after* the token is atomically claimed
/// (see accept_invite's doc comment) — this proves the claim rolls back
/// rather than permanently burning the token on that rejected attempt.
#[tokio::test]
async fn invites_self_accept_does_not_burn_the_token() {
    let app = TestApp::new().await;
    let (token_a, _id_a) = onboard(&app, "hanko|a", "alice").await;
    let (token_b, _id_b) = onboard(&app, "hanko|b", "bob").await;

    let (status, invite) = send(
        &app.router,
        json_request("POST", "/invites", Some(&token_a), json!({})),
    )
    .await;
    assert_eq!(status, StatusCode::OK);
    let token = invite["token"].as_str().unwrap().to_string();

    let (status, _) = send(
        &app.router,
        json_request(
            "POST",
            &format!("/invites/{token}/accept"),
            Some(&token_a),
            json!({}),
        ),
    )
    .await;
    assert_eq!(status, StatusCode::BAD_REQUEST);

    let (status, _) = send(
        &app.router,
        json_request(
            "POST",
            &format!("/invites/{token}/accept"),
            Some(&token_b),
            json!({}),
        ),
    )
    .await;
    assert_eq!(
        status,
        StatusCode::OK,
        "Alice's rejected self-accept must not have burned the token"
    );
}

/// A single-use token accepted by two different people at nearly the same
/// moment must let exactly one of them win — the earlier check-then-act
/// version (separate select, insert, then an unconditionally-succeeding
/// update) let both through, since neither request's update was guarded by
/// `used_at is null`.
#[tokio::test]
async fn invites_accept_is_race_safe() {
    let app = TestApp::new().await;
    let (token_a, _id_a) = onboard(&app, "hanko|a", "alice").await;
    let (token_b, _id_b) = onboard(&app, "hanko|b", "bob").await;
    let (token_c, _id_c) = onboard(&app, "hanko|c", "carol").await;

    let (status, invite) = send(
        &app.router,
        json_request("POST", "/invites", Some(&token_a), json!({})),
    )
    .await;
    assert_eq!(status, StatusCode::OK);
    let token = invite["token"].as_str().unwrap().to_string();

    let accept_b = send(
        &app.router,
        json_request(
            "POST",
            &format!("/invites/{token}/accept"),
            Some(&token_b),
            json!({}),
        ),
    );
    let accept_c = send(
        &app.router,
        json_request(
            "POST",
            &format!("/invites/{token}/accept"),
            Some(&token_c),
            json!({}),
        ),
    );
    let ((status_b, body_b), (status_c, body_c)) = tokio::join!(accept_b, accept_c);

    let successes = [status_b, status_c]
        .iter()
        .filter(|s| **s == StatusCode::OK)
        .count();
    assert_eq!(
        successes, 1,
        "exactly one concurrent accept should win: bob={status_b} ({body_b}), carol={status_c} ({body_c})"
    );
}

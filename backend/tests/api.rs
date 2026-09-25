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
use wiremock::matchers::{method, path, query_param};
use wiremock::{Mock, ResponseTemplate};

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

/// `backdated: true` (logging a book read before the user had the app) must
/// suppress the feed event a status change normally generates; the book
/// still lands on the right shelf either way.
#[tokio::test]
async fn book_status_backdated_suppresses_feed_event_but_not_the_shelf() {
    let app = TestApp::new().await;
    let (token, user_id) = onboard(&app, "hanko|a", "alice").await;

    let backlog_book = resolve_book(&app, &token, "OL-backlog").await;
    let (status, backlog) = send(
        &app.router,
        json_request(
            "PUT",
            "/book-statuses",
            Some(&token),
            json!({ "book_id": backlog_book, "status": "finished", "rating": 4, "backdated": true }),
        ),
    )
    .await;
    assert_eq!(status, StatusCode::OK, "body: {backlog}");
    assert_eq!(backlog["backdated"], true);

    let (status, feed) = send(
        &app.router,
        get_request(&format!("/users/{user_id}/feed"), Some(&token)),
    )
    .await;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(
        feed.as_array().unwrap().len(),
        0,
        "a backdated status change must not appear in the feed"
    );

    let (status, library) = send(
        &app.router,
        get_request(
            &format!("/users/{user_id}/library?shelf=read"),
            Some(&token),
        ),
    )
    .await;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(
        library.as_array().unwrap().len(),
        1,
        "the book still lands on the Read shelf even though it's backdated"
    );

    // Regression check: an ordinary (non-backdated) status change on a
    // *different* book still produces a feed event as before.
    let normal_book = resolve_book(&app, &token, "OL-normal").await;
    let (status, _) = send(
        &app.router,
        json_request(
            "PUT",
            "/book-statuses",
            Some(&token),
            json!({ "book_id": normal_book, "status": "finished" }),
        ),
    )
    .await;
    assert_eq!(status, StatusCode::OK);

    let (status, feed) = send(
        &app.router,
        get_request(&format!("/users/{user_id}/feed"), Some(&token)),
    )
    .await;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(
        feed.as_array().unwrap().len(),
        1,
        "a normal status change should still appear in the feed"
    );
}

/// `backdated` is meant to badge "this row's current status came from the
/// backlog flow" — it must clear the moment the status genuinely changes
/// (a real reread), but re-submitting the *same* status (e.g. just editing
/// the rating) must leave it alone, since nothing about "when did this
/// status take effect" actually changed.
#[tokio::test]
async fn book_status_backdated_flag_clears_on_status_change_only() {
    let app = TestApp::new().await;
    let (token, user_id) = onboard(&app, "hanko|a", "alice").await;
    let book = resolve_book(&app, &token, "OL-reread").await;

    let (status, row) = send(
        &app.router,
        json_request(
            "PUT",
            "/book-statuses",
            Some(&token),
            json!({ "book_id": book, "status": "finished", "rating": 3, "backdated": true }),
        ),
    )
    .await;
    assert_eq!(status, StatusCode::OK, "body: {row}");
    assert_eq!(row["backdated"], true);

    // Re-submitting the *same* status (editing the rating, not backdated
    // this time) must not clear the flag — nothing about the status changed.
    let (status, row) = send(
        &app.router,
        json_request(
            "PUT",
            "/book-statuses",
            Some(&token),
            json!({ "book_id": book, "status": "finished", "rating": 5 }),
        ),
    )
    .await;
    assert_eq!(status, StatusCode::OK, "body: {row}");
    assert_eq!(
        row["backdated"], true,
        "same status (finished -> finished) must not clear backdated"
    );

    // A genuine status change (starting a real reread) must clear it.
    let (status, row) = send(
        &app.router,
        json_request(
            "PUT",
            "/book-statuses",
            Some(&token),
            json!({ "book_id": book, "status": "currently_reading", "progress_percent": 10 }),
        ),
    )
    .await;
    assert_eq!(status, StatusCode::OK, "body: {row}");
    assert_eq!(
        row["backdated"], false,
        "a real status change (finished -> currently_reading) must clear backdated"
    );

    // Finishing the reread for real produces its own feed event — the
    // earlier backdated finish contributed none, so this is the only one.
    let (status, _) = send(
        &app.router,
        json_request(
            "PUT",
            "/book-statuses",
            Some(&token),
            json!({ "book_id": book, "status": "finished", "rating": 4 }),
        ),
    )
    .await;
    assert_eq!(status, StatusCode::OK);

    let (status, feed) = send(
        &app.router,
        get_request(&format!("/users/{user_id}/feed"), Some(&token)),
    )
    .await;
    assert_eq!(status, StatusCode::OK);
    let feed = feed.as_array().unwrap();
    assert_eq!(
        feed.iter().filter(|e| e["status"] == "finished").count(),
        1,
        "only the real reread's finish should be in the feed, not the backdated one"
    );
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

/// Regression: after a QR/link invite is accepted, the *inviter* must see the
/// scanner in their friend list too, not only the other way round.
#[tokio::test]
async fn friend_list_is_symmetric_and_scoped_to_the_caller() {
    let app = TestApp::new().await;
    let (token_a, id_a) = onboard(&app, "hanko|a", "alice").await;
    let (token_b, id_b) = onboard(&app, "hanko|b", "bob").await;
    let (token_c, _id_c) = onboard(&app, "hanko|c", "carol").await;

    let (status, friends) = send(&app.router, get_request("/me/friends", Some(&token_a))).await;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(friends, json!([]), "no friends yet");

    let (_, invite) = send(
        &app.router,
        json_request("POST", "/invites", Some(&token_a), json!({})),
    )
    .await;
    let invite_token = invite["token"].as_str().unwrap();
    let (status, _) = send(
        &app.router,
        json_request(
            "POST",
            &format!("/invites/{invite_token}/accept"),
            Some(&token_b),
            json!({}),
        ),
    )
    .await;
    assert_eq!(status, StatusCode::OK);

    // Inviter (alice) sees the scanner (bob)...
    let (_, friends) = send(&app.router, get_request("/me/friends", Some(&token_a))).await;
    let friends = friends.as_array().unwrap();
    assert_eq!(friends.len(), 1);
    assert_eq!(friends[0]["id"], id_b);
    assert_eq!(friends[0]["handle"], "bob");

    // ...and the scanner sees the inviter.
    let (_, friends) = send(&app.router, get_request("/me/friends", Some(&token_b))).await;
    let friends = friends.as_array().unwrap();
    assert_eq!(friends.len(), 1);
    assert_eq!(friends[0]["id"], id_a);

    // A third user is not affected.
    let (_, friends) = send(&app.router, get_request("/me/friends", Some(&token_c))).await;
    assert_eq!(friends, json!([]));
}

/// Friends can read your library only after you opt in with
/// `PATCH /me { share_shelves: true }`; strangers never can; turning it off
/// closes it again. Off by default.
#[tokio::test]
async fn library_is_visible_to_friends_only_when_shared() {
    let app = TestApp::new().await;
    let (token_a, id_a) = onboard(&app, "hanko|a", "alice").await;
    let (token_b, _id_b) = onboard(&app, "hanko|b", "bob").await;
    let (token_c, _id_c) = onboard(&app, "hanko|c", "carol").await;

    let book_id = resolve_book(&app, &token_a, "OL1W").await;
    let (status, _) = send(
        &app.router,
        json_request(
            "PUT",
            "/book-statuses",
            Some(&token_a),
            json!({ "book_id": book_id, "status": "finished", "rating": 5 }),
        ),
    )
    .await;
    assert_eq!(status, StatusCode::OK);

    // alice <-> bob are friends; carol is a stranger.
    send(
        &app.router,
        json_request(
            "POST",
            "/friendships",
            Some(&token_b),
            json!({ "user_handle": "alice" }),
        ),
    )
    .await;

    let library = format!("/users/{id_a}/library");

    let (_, me) = send(&app.router, get_request("/me", Some(&token_a))).await;
    assert_eq!(me["share_shelves"], false, "private by default");

    let (status, _) = send(&app.router, get_request(&library, Some(&token_b))).await;
    assert_eq!(status, StatusCode::FORBIDDEN, "friend, not shared yet");

    let (status, me) = send(
        &app.router,
        json_request(
            "PATCH",
            "/me",
            Some(&token_a),
            json!({ "share_shelves": true }),
        ),
    )
    .await;
    assert_eq!(status, StatusCode::OK, "body: {me}");
    assert_eq!(me["share_shelves"], true);

    let (status, entries) = send(&app.router, get_request(&library, Some(&token_b))).await;
    assert_eq!(status, StatusCode::OK, "friend, shared");
    assert_eq!(entries.as_array().unwrap().len(), 1);
    assert_eq!(entries[0]["rating"], 5);

    let (status, _) = send(&app.router, get_request(&library, Some(&token_c))).await;
    assert_eq!(status, StatusCode::FORBIDDEN, "stranger, even when shared");

    // Sharing doesn't open the other self-only routes to friends.
    let (status, _) = send(
        &app.router,
        get_request(&format!("/users/{id_a}/book-statuses"), Some(&token_b)),
    )
    .await;
    assert_eq!(status, StatusCode::FORBIDDEN);

    // An empty PATCH changes nothing; turning it off closes the door again.
    let (_, me) = send(
        &app.router,
        json_request("PATCH", "/me", Some(&token_a), json!({})),
    )
    .await;
    assert_eq!(me["share_shelves"], true);
    send(
        &app.router,
        json_request(
            "PATCH",
            "/me",
            Some(&token_a),
            json!({ "share_shelves": false }),
        ),
    )
    .await;
    let (status, _) = send(&app.router, get_request(&library, Some(&token_b))).await;
    assert_eq!(status, StatusCode::FORBIDDEN);
}

/// Profile editing: display name validation + trimming, and the avatar flow —
/// mint an upload URL, then only *that* URL (own prefix) is accepted back.
#[tokio::test]
async fn profile_edit_display_name_and_avatar() {
    let app = TestApp::new().await;
    let (token_a, id_a) = onboard(&app, "hanko|a", "alice").await;
    let (token_b, _id_b) = onboard(&app, "hanko|b", "bob").await;

    let patch = |token: &str, body: Value| json_request("PATCH", "/me", Some(token), body);

    let (status, me) = send(
        &app.router,
        patch(&token_a, json!({ "display_name": "  Alice L.  " })),
    )
    .await;
    assert_eq!(status, StatusCode::OK, "body: {me}");
    assert_eq!(me["display_name"], "Alice L.", "trimmed");
    assert_eq!(me["handle"], "alice", "handle isn't editable");

    for bad in ["", "   ", &"x".repeat(51)] {
        let (status, _) = send(&app.router, patch(&token_a, json!({ "display_name": bad }))).await;
        assert_eq!(status, StatusCode::BAD_REQUEST, "display_name {bad:?}");
    }

    // Upload ticket: SAS URL under alice's own prefix.
    let (status, ticket) = send(
        &app.router,
        json_request("POST", "/me/avatar-upload", Some(&token_a), json!({})),
    )
    .await;
    assert_eq!(status, StatusCode::OK, "body: {ticket}");
    let avatar_url = ticket["avatar_url"].as_str().unwrap().to_string();
    let upload_url = ticket["upload_url"].as_str().unwrap();
    assert!(avatar_url.contains(&format!("/avatars/{id_a}/")));
    assert!(upload_url.starts_with(&avatar_url) && upload_url.contains("sig="));

    // Arbitrary / foreign / query-suffixed URLs are refused...
    for bad in [
        "https://evil.example/a.jpg".to_string(),
        upload_url.to_string(),
    ] {
        let (status, _) = send(&app.router, patch(&token_a, json!({ "avatar_url": bad }))).await;
        assert_eq!(status, StatusCode::BAD_REQUEST, "{bad}");
    }
    // ...including alice's blob URL submitted by bob.
    let (status, _) = send(
        &app.router,
        patch(&token_b, json!({ "avatar_url": avatar_url })),
    )
    .await;
    assert_eq!(status, StatusCode::BAD_REQUEST, "another user's prefix");

    // The minted one is accepted, survives an unrelated PATCH, and null clears it.
    let (status, me) = send(
        &app.router,
        patch(&token_a, json!({ "avatar_url": avatar_url })),
    )
    .await;
    assert_eq!(status, StatusCode::OK, "body: {me}");
    assert_eq!(me["avatar_url"], avatar_url);
    let (_, me) = send(
        &app.router,
        patch(&token_a, json!({ "share_shelves": true })),
    )
    .await;
    assert_eq!(me["avatar_url"], avatar_url, "absent field leaves it alone");
    let (_, me) = send(&app.router, patch(&token_a, json!({ "avatar_url": null }))).await;
    assert!(me["avatar_url"].is_null(), "null removes it");
}

/// Pick a library system, then get a per-book link: the catalog's record page
/// when it has the *work* (matched by title + author, whatever edition/ISBN we
/// happen to store), a title-search link when it doesn't or can't be reached
/// (never an error), and a 400 until a library is chosen.
#[tokio::test]
async fn library_link_matches_the_work_with_search_fallback() {
    let app = TestApp::new().await;
    let (token, _id) = onboard(&app, "hanko|a", "alice").await;

    let (status, systems) = send(&app.router, get_request("/library-systems", Some(&token))).await;
    assert_eq!(status, StatusCode::OK);
    let ids: Vec<_> = systems
        .as_array()
        .unwrap()
        .iter()
        .map(|s| s["id"].as_str().unwrap())
        .collect();
    assert_eq!(ids, ["seattle", "kcls"]);

    let resolve = |source_id: &str, title: &str, isbn: Value| {
        let mut body = normalized_book(source_id, source_id, "Ed");
        body["isbn_13"] = isbn;
        body["canonical_title"] = json!(title);
        body["primary_author"] = json!("Andy Weir");
        json_request("POST", "/books/resolve", Some(&token), body)
    };
    // Stored ISBN is NOT the one the library holds — the common case.
    let (_, known) = send(
        &app.router,
        resolve("OL1W", "Project Hail Mary", json!("9781529000000")),
    )
    .await;
    let (_, no_isbn) = send(
        &app.router,
        resolve("OL2W", "Project Hail Mary", json!(null)),
    )
    .await;
    // Retitled in the catalog: only the ISBN can find it.
    let (_, retitled) = send(
        &app.router,
        resolve("OL3W", "Hail Mary Project", json!("978-0-593-13520-4")),
    )
    .await;
    let (_, missing) = send(
        &app.router,
        resolve("OL4W", "Unfindable Tome", json!("9781234567897")),
    )
    .await;
    let link_of = |book: &Value| format!("/books/{}/library-link", book["id"].as_str().unwrap());

    // No library chosen yet.
    let (status, _) = send(&app.router, get_request(&link_of(&known), Some(&token))).await;
    assert_eq!(status, StatusCode::BAD_REQUEST);

    // Choosing: unknown ids rejected, valid ones stored and readable.
    let put = |body: Value| json_request("PUT", "/me/library-system", Some(&token), body);
    let (status, _) = send(&app.router, put(json!({ "library_system": "atlantis" }))).await;
    assert_eq!(status, StatusCode::BAD_REQUEST);
    let (status, mine) = send(&app.router, put(json!({ "library_system": "seattle" }))).await;
    assert_eq!(status, StatusCode::OK, "body: {mine}");
    assert_eq!(mine["library_system"]["name"], "Seattle Public Library");
    let (_, mine) = send(&app.router, get_request("/me/library-system", Some(&token))).await;
    assert_eq!(mine["library_system"]["id"], "seattle");

    let bib = |id: &str, format: &str, title: &str, authors: Value, isbn: &str, lang: &str| {
        json!({ "id": id, "briefInfo": {
            "format": format, "title": title, "authors": authors,
            "isbns": [isbn], "primaryLanguage": lang } })
    };
    let results = |bibs: Vec<Value>| {
        let map: serde_json::Map<String, Value> = bibs
            .into_iter()
            .map(|b| (b["id"].as_str().unwrap().to_string(), b))
            .collect();
        ResponseTemplate::new(200).set_body_json(json!({ "entities": { "bibs": map } }))
    };
    let gateway = "/v2/libraries/seattle/bibs/search";
    let on_query = |q: &str| {
        Mock::given(method("GET"))
            .and(path(gateway))
            .and(query_param("query", q))
    };
    // Title+author search: the right work in several formats/languages, plus noise.
    on_query("Project Hail Mary Andy Weir")
        .respond_with(results(vec![
            bib(
                "S30DVD",
                "DVD",
                "PROJECT HAIL MARY (DVD)",
                json!([]),
                "",
                "eng",
            ),
            bib(
                "S30EB",
                "EBOOK",
                "Project Hail Mary",
                json!(["Weir, Andy"]),
                "9780593135211",
                "eng",
            ),
            bib(
                "S30BK",
                "BK",
                "Project Hail Mary",
                json!(["Weir, Andy"]),
                "9780593135204",
                "eng",
            ),
            bib(
                "S30ES",
                "BK",
                "Proyecto Hail Mary",
                json!(["Weir, Andy"]),
                "9788466000000",
                "spa",
            ),
        ]))
        .mount(&app.catalog_server)
        .await;
    on_query("Hail Mary Project Andy Weir")
        .respond_with(results(vec![]))
        .mount(&app.catalog_server)
        .await;
    on_query("9780593135204")
        .respond_with(results(vec![bib(
            "S30RT",
            "BK",
            "Project Hail Mary",
            json!(["Weir, Andy"]),
            "9780593135204",
            "eng",
        )]))
        .mount(&app.catalog_server)
        .await;
    on_query("Unfindable Tome Andy Weir")
        .respond_with(results(vec![]))
        .mount(&app.catalog_server)
        .await;
    on_query("9781234567897")
        .respond_with(results(vec![]))
        .mount(&app.catalog_server)
        .await;

    // Found by title+author despite the ISBN mismatch → the plain English book.
    // No ISBN on file at all works too.
    for book in [&known, &no_isbn] {
        let (status, link) = send(&app.router, get_request(&link_of(book), Some(&token))).await;
        assert_eq!(status, StatusCode::OK, "body: {link}");
        assert_eq!(link["found"], true);
        assert_eq!(link["lookup_failed"], false);
        assert_eq!(
            link["url"],
            "https://seattle.bibliocommons.com/v2/record/S30BK"
        );
        assert_eq!(link["library"]["id"], "seattle");
    }

    // Retitled in the catalog: title search misses, the ISBN fallback finds it.
    let (_, link) = send(&app.router, get_request(&link_of(&retitled), Some(&token))).await;
    assert_eq!(link["found"], true, "body: {link}");
    assert_eq!(
        link["url"],
        "https://seattle.bibliocommons.com/v2/record/S30RT"
    );

    // Catalog doesn't have it → title search, not an error.
    let (status, link) = send(&app.router, get_request(&link_of(&missing), Some(&token))).await;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(link["found"], false);
    assert_eq!(link["lookup_failed"], false);
    assert_eq!(
        link["url"],
        "https://seattle.bibliocommons.com/v2/search?query=Unfindable%20Tome%20Andy%20Weir&searchType=smart"
    );

    // Catalog down → still 200 with a usable link, flagged as a failed lookup.
    let (status, _) = send(&app.router, put(json!({ "library_system": "kcls" }))).await;
    assert_eq!(status, StatusCode::OK);
    Mock::given(method("GET"))
        .and(path("/v2/libraries/kcls/bibs/search"))
        .respond_with(ResponseTemplate::new(503))
        .mount(&app.catalog_server)
        .await;
    let (status, link) = send(&app.router, get_request(&link_of(&known), Some(&token))).await;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(link["found"], false);
    assert_eq!(link["lookup_failed"], true);
    assert!(link["url"]
        .as_str()
        .unwrap()
        .starts_with("https://kcls.bibliocommons.com/v2/search?"));

    // Clearing the choice goes back to 400; unknown book is 404.
    let (_, mine) = send(&app.router, put(json!({ "library_system": null }))).await;
    assert!(mine["library_system"].is_null());
    let (status, _) = send(&app.router, get_request(&link_of(&known), Some(&token))).await;
    assert_eq!(status, StatusCode::BAD_REQUEST);
    send(&app.router, put(json!({ "library_system": "seattle" }))).await;
    let nonexistent = format!("/books/{}/library-link", Uuid::new_v4());
    let (status, _) = send(&app.router, get_request(&nonexistent, Some(&token))).await;
    assert_eq!(status, StatusCode::NOT_FOUND);
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

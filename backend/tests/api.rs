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

/// Make two onboarded users friends the only way that exists: one creates a
/// single-use invite and the other accepts it. Returns the friendship id (the
/// same id for both of them — friends are referenced by it, never by user id).
async fn befriend(app: &TestApp, inviter_token: &str, accepter_token: &str) -> String {
    let (status, invite) = send(
        &app.router,
        json_request("POST", "/invites", Some(inviter_token), json!({})),
    )
    .await;
    assert_eq!(status, StatusCode::OK, "invite: {invite}");
    let token = invite["token"].as_str().unwrap();
    let (status, body) = send(
        &app.router,
        json_request(
            "POST",
            &format!("/invites/{token}/accept"),
            Some(accepter_token),
            json!({}),
        ),
    )
    .await;
    assert_eq!(status, StatusCode::OK, "accept: {body}");
    assert_eq!(body["status"], "friends");
    body["friendship_id"].as_str().unwrap().to_string()
}

/// A friend's profile is reached through the friendship, never a user id: a
/// stranger has nothing to look up (and there is no lookup by user id or handle).
#[tokio::test]
async fn friend_profiles_are_reached_through_the_friendship() {
    let app = TestApp::new().await;
    let (token_a, id_a) = onboard(&app, "hanko|a", "alice").await;
    let (token_b, id_b) = onboard(&app, "hanko|b", "bob").await;
    let (token_c, _id_c) = onboard(&app, "hanko|c", "carol").await;

    // No lookup by user id or by handle, for anyone.
    for uri in [format!("/users/{id_b}"), "/users/by-handle/bob".to_string()] {
        let (status, _) = send(&app.router, get_request(&uri, Some(&token_a))).await;
        assert_eq!(status, StatusCode::NOT_FOUND, "{uri}");
    }
    let (status, _) = send(
        &app.router,
        get_request(&format!("/users/{id_a}"), Some(&token_a)),
    )
    .await;
    assert_eq!(
        status,
        StatusCode::NOT_FOUND,
        "not even your own — that's /me"
    );

    // Another user's library isn't addressable by user id either.
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

    let fid = befriend(&app, &token_a, &token_b).await;

    let (status, profile) = send(
        &app.router,
        get_request(&format!("/friends/{fid}"), Some(&token_a)),
    )
    .await;
    assert_eq!(status, StatusCode::OK, "body: {profile}");
    assert_eq!(profile["handle"], "bob");
    assert_eq!(profile["friendship_id"], fid);
    assert_eq!(profile["share_shelves"], false);
    assert!(profile.get("id").is_none() && profile.get("user_id").is_none());

    // Both sides address the same friendship, and each sees the *other* person.
    let (_, from_bob) = send(
        &app.router,
        get_request(&format!("/friends/{fid}"), Some(&token_b)),
    )
    .await;
    assert_eq!(from_bob["handle"], "alice");

    // A friendship isn't reachable by someone who isn't in it, or if it doesn't exist.
    let (status, _) = send(
        &app.router,
        get_request(&format!("/friends/{fid}"), Some(&token_c)),
    )
    .await;
    assert_eq!(status, StatusCode::NOT_FOUND);
    let (status, _) = send(
        &app.router,
        get_request(&format!("/friends/{}", Uuid::new_v4()), Some(&token_a)),
    )
    .await;
    assert_eq!(status, StatusCode::NOT_FOUND);
}

/// Friendships only form through an invite: the old add-by-handle endpoint is
/// gone, and rows are stored canonicalized (user_a_id < user_b_id).
#[tokio::test]
async fn friendships_only_form_through_invites() {
    let app = TestApp::new().await;
    let (token_a, _id_a) = onboard(&app, "hanko|a", "alice").await;
    let (token_b, _id_b) = onboard(&app, "hanko|b", "bob").await;

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
    assert_eq!(status, StatusCode::NOT_FOUND, "no add-by-handle");
    let (_, friends) = send(&app.router, get_request("/me/friends", Some(&token_b))).await;
    assert_eq!(friends, json!([]), "and it didn't create anything");

    befriend(&app, &token_a, &token_b).await;
    let rows = sqlx::query_as::<_, (Uuid, Uuid)>("select user_a_id, user_b_id from friendships")
        .fetch_all(&app.db.pool)
        .await
        .unwrap();
    assert_eq!(rows.len(), 1);
    assert!(rows[0].0 < rows[0].1, "canonicalized");
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
    let (token_b, _id_b) = onboard(&app, "hanko|b", "bob").await;
    let (token_c, _id_c) = onboard(&app, "hanko|c", "carol").await;

    let fid = befriend(&app, &token_a, &token_b).await;

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
    assert_eq!(
        feed[0]["actor"]["friendship_id"], fid,
        "referenced by friendship"
    );
    assert_eq!(feed[0]["actor"]["is_me"], false);
    assert!(feed[0]["actor"].get("id").is_none(), "never a user id");
    assert_eq!(feed[0]["verb"], "started reading");
}

#[tokio::test]
async fn recommendations_go_to_a_friend_by_friendship() {
    let app = TestApp::new().await;
    let (token_a, id_a) = onboard(&app, "hanko|a", "alice").await;
    let (token_b, id_b) = onboard(&app, "hanko|b", "bob").await;
    let (token_c, _id_c) = onboard(&app, "hanko|c", "carol").await;
    let book = resolve_book(&app, &token_a, "OL-rec").await;

    let fid = befriend(&app, &token_a, &token_b).await;
    let other_fid = befriend(&app, &token_b, &token_c).await; // bob <-> carol, not alice's

    let recommend = |friendship: &str| {
        json_request(
            "POST",
            "/recommendations",
            Some(&token_a),
            json!({ "to_friendship_id": friendship, "book_id": book, "note": "you'd love this" }),
        )
    };

    let (status, rec) = send(&app.router, recommend(&fid)).await;
    assert_eq!(status, StatusCode::OK, "body: {rec}");
    assert_eq!(rec["to_friendship_id"], fid);
    assert!(rec.get("to_user_id").is_none() && rec.get("from_user_id").is_none());

    // You can only recommend to your own friends: someone else's friendship, or
    // one that doesn't exist, is a 404 — and the recipient is never a raw user id.
    for bad in [other_fid, Uuid::new_v4().to_string()] {
        let (status, _) = send(&app.router, recommend(&bad)).await;
        assert_eq!(status, StatusCode::NOT_FOUND, "{bad}");
    }
    let (status, _) = send(
        &app.router,
        json_request(
            "POST",
            "/recommendations",
            Some(&token_a),
            json!({ "to_user_id": id_b, "book_id": book }),
        ),
    )
    .await;
    assert!(status.is_client_error(), "the old by-user-id shape is gone");

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
    assert_eq!(
        inbox_b.len(),
        1,
        "only the valid recommendation was delivered"
    );
    assert_eq!(inbox_b[0]["note"], "you'd love this");
    assert_eq!(inbox_b[0]["from"]["friendship_id"], fid);
    assert_eq!(inbox_b[0]["from"]["display_name"], "alice");
    assert!(
        !inbox_b[0].to_string().contains(&id_a),
        "the inbox must not contain the sender's user id"
    );

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
    assert_eq!(friendship["status"], "friends");
    assert!(friendship["friendship_id"].is_string());
    let (lo, hi) = if id_a < id_b {
        (&id_a, &id_b)
    } else {
        (&id_b, &id_a)
    };
    let row = sqlx::query_as::<_, (Uuid, Uuid)>("select user_a_id, user_b_id from friendships")
        .fetch_one(&app.db.pool)
        .await
        .unwrap();
    assert_eq!(
        (row.0.to_string(), row.1.to_string()),
        (lo.clone(), hi.clone())
    );
    assert!(
        friendship.get("user_a_id").is_none() && friendship.get("user_b_id").is_none(),
        "accepting must not hand back either user's id"
    );

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
    let (token_a, _id_a) = onboard(&app, "hanko|a", "alice").await;
    let (token_b, _id_b) = onboard(&app, "hanko|b", "bob").await;
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
    assert_eq!(friends[0]["handle"], "bob");
    assert!(friends[0].get("id").is_none(), "friends carry no user id");
    let alice_sees = friends[0]["friendship_id"].clone();

    // ...and the scanner sees the inviter.
    let (_, friends) = send(&app.router, get_request("/me/friends", Some(&token_b))).await;
    let friends = friends.as_array().unwrap();
    assert_eq!(friends.len(), 1);
    assert_eq!(friends[0]["handle"], "alice");
    assert_eq!(
        friends[0]["friendship_id"], alice_sees,
        "the same friendship from both sides"
    );

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

    // alice <-> bob are friends; carol is a stranger. Bob reaches Alice's shelves
    // through their friendship.
    let fid = befriend(&app, &token_a, &token_b).await;

    let library = format!("/friends/{fid}/library");

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
    assert_eq!(
        status,
        StatusCode::NOT_FOUND,
        "stranger: not their friendship, even when shared"
    );
    let (status, _) = send(
        &app.router,
        get_request(&format!("/users/{id_a}/library"), Some(&token_b)),
    )
    .await;
    assert_eq!(
        status,
        StatusCode::FORBIDDEN,
        "a friend can't use the user-id route"
    );

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
    let key = sqlx::query_scalar::<_, Uuid>("select avatar_key from users where id = $1::uuid")
        .bind(&id_a)
        .fetch_one(&app.db.pool)
        .await
        .unwrap();
    assert!(
        avatar_url.contains(&format!("/avatars/{key}/")),
        "stored under the avatar key"
    );
    assert!(
        !avatar_url.contains(&id_a),
        "the photo URL is shown to other people and must not contain the user id"
    );
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

    // Translated work: filed (and displayed) under its Spanish title, saved
    // edition is the English translation. Search under both titles and prefer
    // the record titled like the book the user sees.
    let mut translated = normalized_book("OL5W", "OL5W", "One Hundred Years of Solitude");
    translated["canonical_title"] = json!("Cien años de soledad");
    translated["primary_author"] = json!("Gabriel García Márquez");
    let (_, translated) = send(
        &app.router,
        json_request("POST", "/books/resolve", Some(&token), translated),
    )
    .await;
    on_query("Cien años de soledad Gabriel García Márquez")
        .respond_with(results(vec![bib(
            "S30SP",
            "BK",
            "Cien años de soledad",
            json!(["García Márquez, Gabriel"]),
            "9788400000000",
            "spa",
        )]))
        .mount(&app.catalog_server)
        .await;
    on_query("One Hundred Years of Solitude Gabriel García Márquez")
        .respond_with(results(vec![bib(
            "S30EN",
            "EBOOK",
            "One Hundred Years of Solitude",
            json!(["García Márquez, Gabriel"]),
            "9780060000000",
            "eng",
        )]))
        .mount(&app.catalog_server)
        .await;
    let (_, link) = send(
        &app.router,
        get_request(&link_of(&translated), Some(&token)),
    )
    .await;
    assert_eq!(link["found"], true, "body: {link}");
    assert_eq!(
        link["url"], "https://seattle.bibliocommons.com/v2/record/S30SP",
        "the Spanish record matches the title shown in the app"
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

/// Descriptions ride along on resolve, come back from `GET /books/{id}`, are
/// null when no source had one, and get filled in (never overwritten) when the
/// same book is resolved again later.
#[tokio::test]
async fn book_descriptions_are_stored_and_filled_in_later() {
    let app = TestApp::new().await;
    let (token, _id) = onboard(&app, "hanko|a", "alice").await;

    let resolve = |source_id: &str, description: Value| {
        let mut body = normalized_book(source_id, source_id, "Ed");
        body["description"] = description;
        json_request("POST", "/books/resolve", Some(&token), body)
    };
    let get_book = |id: &str| get_request(&format!("/books/{id}"), Some(&token));

    // With a description.
    let (status, with) = send(&app.router, resolve("OL1W", json!("A story about hunger."))).await;
    assert_eq!(status, StatusCode::OK, "body: {with}");
    assert_eq!(with["description"], "A story about hunger.");
    let (_, fetched) = send(&app.router, get_book(with["id"].as_str().unwrap())).await;
    assert_eq!(fetched["description"], "A story about hunger.");

    // Without one (and no provider ids that a backfill could ask about).
    let mut manual = normalized_book("manual-1", "manual-1", "Ed");
    manual["source"] = json!("manual");
    manual["open_library_work_id"] = json!(null);
    let (_, without) = send(
        &app.router,
        json_request("POST", "/books/resolve", Some(&token), manual.clone()),
    )
    .await;
    assert!(without["description"].is_null());
    let (_, fetched) = send(&app.router, get_book(without["id"].as_str().unwrap())).await;
    assert!(fetched["description"].is_null());

    // Re-resolving the same edition later, now with a description, fills it in...
    manual["description"] = json!("Added later.");
    let (_, again) = send(
        &app.router,
        json_request("POST", "/books/resolve", Some(&token), manual.clone()),
    )
    .await;
    assert_eq!(again["id"], without["id"], "same book");
    assert_eq!(again["description"], "Added later.");

    // ...but never overwrites an existing one.
    manual["description"] = json!("Something else entirely.");
    let (_, third) = send(
        &app.router,
        json_request("POST", "/books/resolve", Some(&token), manual),
    )
    .await;
    assert_eq!(third["description"], "Added later.");
}

/// Yearly counts come from `book_completions`: live finishes and rereads count
/// (each finish, even of the same book), backdated backlog reads don't (until
/// reread), an immediately-undone finish doesn't, and the year is read in the
/// caller's time zone. Also covers goal create/update/delete + validation.
#[tokio::test]
async fn reading_stats_count_completions_not_backlog() {
    use chrono::Datelike;
    let app = TestApp::new().await;
    let (token, user_id) = onboard(&app, "hanko|a", "alice").await;
    let this_year = chrono::Utc::now().year();
    let month_idx = (chrono::Utc::now()
        .format("%m")
        .to_string()
        .parse::<usize>()
        .unwrap())
        - 1;

    let stats = |query: &str| {
        let token = token.clone();
        let router = app.router.clone();
        let uri = format!("/me/reading-stats{query}");
        async move { send(&router, get_request(&uri, Some(&token))).await }
    };
    let set_status = |book: &str, status: &str, backdated: bool| {
        json_request(
            "PUT",
            "/book-statuses",
            Some(&token),
            json!({ "book_id": book, "status": status, "backdated": backdated }),
        )
    };
    let age_completions = |book: String| {
        let pool = app.db.pool.clone();
        async move {
            sqlx::query(
                "update book_completions set completed_at = now() - interval '2 days' \
                 where book_id = $1::uuid",
            )
            .bind(book)
            .execute(&pool)
            .await
            .unwrap();
        }
    };

    let (_, empty) = stats("").await;
    assert_eq!(empty["year"], this_year);
    assert_eq!(empty["completed"], 0);
    assert!(empty["goal"].is_null());

    let book_a = resolve_book(&app, &token, "OL1W").await;
    let book_b = resolve_book(&app, &token, "OL2W").await;
    let book_c = resolve_book(&app, &token, "OL3W").await;

    // A: a live finish counts.
    send(&app.router, set_status(&book_a, "finished", false)).await;
    let (_, s) = stats("").await;
    assert_eq!(s["completed"], 1);
    assert_eq!(s["by_month"][month_idx], 1);

    // B: a backdated (backlog) finish is recorded but not counted.
    send(&app.router, set_status(&book_b, "finished", true)).await;
    let (_, s) = stats("").await;
    assert_eq!(s["completed"], 1, "backlog read isn't counted");

    // Re-submitting the same status (say, a rating edit) doesn't add a finish.
    send(&app.router, set_status(&book_a, "finished", false)).await;
    let (_, s) = stats("").await;
    assert_eq!(s["completed"], 1);

    // Reread A (finished long ago, so leaving 'finished' isn't an undo): counts again.
    age_completions(book_a.clone()).await;
    send(&app.router, set_status(&book_a, "currently_reading", false)).await;
    send(&app.router, set_status(&book_a, "finished", false)).await;
    let (_, s) = stats("").await;
    assert_eq!(s["completed"], 2, "each finish of the same book counts");

    // Reread the backlog book B: the reread is a real, counted finish.
    age_completions(book_b.clone()).await;
    send(&app.router, set_status(&book_b, "currently_reading", false)).await;
    send(&app.router, set_status(&book_b, "finished", false)).await;
    let (_, s) = stats("").await;
    assert_eq!(s["completed"], 3);

    // C: marking finished by accident and undoing it straight away doesn't count.
    send(&app.router, set_status(&book_c, "finished", false)).await;
    send(&app.router, set_status(&book_c, "currently_reading", false)).await;
    let (_, s) = stats("").await;
    assert_eq!(s["completed"], 3, "immediate undo removes the completion");

    // Year boundaries depend on the time zone: this instant is Jan 1 02:00 UTC
    // in 2020 but Dec 31 18:00 in Los Angeles, i.e. still 2019.
    sqlx::query(
        "insert into book_completions (user_id, book_id, completed_at) \
         values ($1::uuid, $2::uuid, '2020-01-01T02:00:00Z')",
    )
    .bind(&user_id)
    .bind(&book_c)
    .execute(&app.db.pool)
    .await
    .unwrap();
    let (_, s) = stats("?year=2020&tz=UTC").await;
    assert_eq!(
        (s["completed"].clone(), s["by_month"][0].clone()),
        (json!(1), json!(1))
    );
    let (_, s) = stats("?year=2020&tz=America/Los_Angeles").await;
    assert_eq!(s["completed"], 0);
    let (_, s) = stats("?year=2019&tz=America/Los_Angeles").await;
    assert_eq!(
        (s["completed"].clone(), s["by_month"][11].clone()),
        (json!(1), json!(1))
    );
    let (_, s) = stats("?year=2019&tz=UTC").await;
    assert_eq!(s["completed"], 0);

    // Validation.
    for bad in ["?tz=Mars/Olympus_Mons", "?year=1800"] {
        let (status, _) = stats(bad).await;
        assert_eq!(status, StatusCode::BAD_REQUEST, "{bad}");
    }

    // Goals: set, change, shown in stats, delete.
    let goal = |year: i32, body: Value| {
        json_request(
            "PUT",
            &format!("/me/reading-goals/{year}"),
            Some(&token),
            body,
        )
    };
    let (status, g) = send(
        &app.router,
        goal(
            this_year,
            json!({ "target_count": 24, "time_zone": "America/Los_Angeles" }),
        ),
    )
    .await;
    assert_eq!(status, StatusCode::OK, "body: {g}");
    let (_, s) = stats("").await;
    assert_eq!(s["goal"]["target_count"], 24);
    assert_eq!(s["goal"]["year"], this_year);
    let (_, _) = send(&app.router, goal(this_year, json!({ "target_count": 30 }))).await;
    let (_, s) = stats("").await;
    assert_eq!(s["goal"]["target_count"], 30, "updated in place");
    let (_, other_year) = stats(&format!("?year={}", this_year - 1)).await;
    assert!(other_year["goal"].is_null(), "goals are per year");

    for bad in [
        json!({ "target_count": 0 }),
        json!({ "target_count": 5, "time_zone": "Nope/Nope" }),
    ] {
        let (status, _) = send(&app.router, goal(this_year, bad.clone())).await;
        assert_eq!(status, StatusCode::BAD_REQUEST, "{bad}");
    }

    let (status, _) = send(
        &app.router,
        json_request(
            "DELETE",
            &format!("/me/reading-goals/{this_year}"),
            Some(&token),
            json!(null),
        ),
    )
    .await;
    assert_eq!(status, StatusCode::NO_CONTENT);
    let (_, s) = stats("").await;
    assert!(s["goal"].is_null());
}

/// Single-use invites connect immediately; already being friends is a success
/// that doesn't use the invite up; `GET /invites` lists what's still usable.
#[tokio::test]
async fn single_use_invites_connect_instantly_and_dont_burn_on_repeat() {
    let app = TestApp::new().await;
    let (token_a, _) = onboard(&app, "hanko|a", "alice").await;
    let (token_b, _) = onboard(&app, "hanko|b", "bob").await;
    let (token_c, _) = onboard(&app, "hanko|c", "carol").await;

    let create = || json_request("POST", "/invites", Some(&token_a), json!({}));
    let accept = |token: &str, who: &str| {
        json_request(
            "POST",
            &format!("/invites/{token}/accept"),
            Some(who),
            json!({}),
        )
    };

    let (_, first) = send(&app.router, create()).await;
    assert_eq!(first["reusable"], false);
    let first_token = first["token"].as_str().unwrap();
    assert_eq!(first_token.len(), 16, "short token for a single-use QR");

    let (_, preview) = send(
        &app.router,
        get_request(&format!("/invites/{first_token}"), None),
    )
    .await;
    assert_eq!(preview["requires_approval"], false);

    let (status, joined) = send(&app.router, accept(first_token, &token_b)).await;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(joined["status"], "friends");

    // Bob is already a friend: a second invite from Alice, accepted by Bob again,
    // succeeds and leaves the invite unused for someone else.
    let (_, second) = send(&app.router, create()).await;
    let second_token = second["token"].as_str().unwrap();
    let (status, again) = send(&app.router, accept(second_token, &token_b)).await;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(again["status"], "friends");
    assert_eq!(again["friendship_id"], joined["friendship_id"]);

    let (_, mine) = send(&app.router, get_request("/invites", Some(&token_a))).await;
    let mine = mine.as_array().unwrap();
    assert_eq!(
        mine.len(),
        1,
        "the spent invite is gone, the untouched one remains"
    );
    assert_eq!(mine[0]["token"], second_token);
    assert_eq!(mine[0]["reusable"], false);
    assert_eq!(mine[0]["use_count"], 0);

    let (status, carol) = send(&app.router, accept(second_token, &token_c)).await;
    assert_eq!(status, StatusCode::OK, "still usable: {carol}");
    assert_eq!(carol["status"], "friends");
}

/// Reusable ("anyone with the link") invites: each accept is only a *request*
/// until the issuer approves; requests reveal only a name + photo (never an id
/// or handle); decline/approve are owner-only; revoking kills the link.
#[tokio::test]
async fn reusable_invites_need_the_issuers_approval() {
    let app = TestApp::new().await;
    let (token_a, _id_a) = onboard(&app, "hanko|a", "alice").await;
    let (token_b, id_b) = onboard(&app, "hanko|b", "bob").await;
    let (token_c, _id_c) = onboard(&app, "hanko|c", "carol").await;
    let (token_d, _id_d) = onboard(&app, "hanko|d", "dave").await;

    let (status, invite) = send(
        &app.router,
        json_request("POST", "/invites?reusable=true", Some(&token_a), json!({})),
    )
    .await;
    assert_eq!(status, StatusCode::OK, "body: {invite}");
    assert_eq!(invite["reusable"], true);
    let token = invite["token"].as_str().unwrap().to_string();
    assert_eq!(
        token.len(),
        32,
        "full-strength token for a link that gets passed around"
    );
    let expires: chrono::DateTime<chrono::Utc> =
        invite["expires_at"].as_str().unwrap().parse().unwrap();
    assert!(expires > chrono::Utc::now() + chrono::Duration::days(29));

    // Public preview: only name, photo, and that approval is needed.
    let (_, preview) = send(&app.router, get_request(&format!("/invites/{token}"), None)).await;
    assert_eq!(preview["requires_approval"], true);
    let mut keys: Vec<_> = preview.as_object().unwrap().keys().cloned().collect();
    keys.sort();
    assert_eq!(keys, ["avatar_url", "display_name", "requires_approval"]);

    let accept = |who: &str| {
        json_request(
            "POST",
            &format!("/invites/{token}/accept"),
            Some(who),
            json!({}),
        )
    };
    let friends_of = |who: &str| get_request("/me/friends", Some(who));

    // Bob asks (twice — idempotent) and Carol asks: nobody is a friend yet.
    let (status, pending) = send(&app.router, accept(&token_b)).await;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(pending["status"], "pending");
    assert!(pending["friendship_id"].is_null());
    let (_, again) = send(&app.router, accept(&token_b)).await;
    assert_eq!(again["status"], "pending");
    send(&app.router, accept(&token_c)).await;
    for who in [&token_a, &token_b, &token_c] {
        let (_, friends) = send(&app.router, friends_of(who)).await;
        assert_eq!(friends, json!([]), "asking isn't befriending");
    }

    // What Alice sees: two requests, each with only a name and photo.
    let (_, requests) = send(
        &app.router,
        get_request("/me/friend-requests", Some(&token_a)),
    )
    .await;
    let requests = requests.as_array().unwrap();
    assert_eq!(
        requests.len(),
        2,
        "the repeat ask didn't duplicate: {requests:?}"
    );
    let mut keys: Vec<_> = requests[0].as_object().unwrap().keys().cloned().collect();
    keys.sort();
    assert_eq!(keys, ["avatar_url", "created_at", "display_name", "id"]);
    let id_of = |name: &str| {
        requests.iter().find(|r| r["display_name"] == name).unwrap()["id"]
            .as_str()
            .unwrap()
            .to_string()
    };
    let (bob_req, carol_req) = (id_of("bob"), id_of("carol"));
    assert!(
        !requests.iter().any(|r| r.to_string().contains(&id_b)),
        "a request must not leak the requester's user id"
    );

    let (_, mine) = send(&app.router, get_request("/invites", Some(&token_a))).await;
    assert_eq!(mine[0]["reusable"], true);
    assert_eq!(mine[0]["pending_requests"], 2);
    assert_eq!(mine[0]["use_count"], 0);

    // Only the issuer can decide.
    let decide = |who: &str, verb: &str, id: &str| {
        json_request(
            "POST",
            &format!("/friend-requests/{id}/{verb}"),
            Some(who),
            json!({}),
        )
    };
    let (status, _) = send(&app.router, decide(&token_b, "approve", &bob_req)).await;
    assert_eq!(
        status,
        StatusCode::NOT_FOUND,
        "the requester can't approve themselves"
    );
    let (status, _) = send(&app.router, decide(&token_c, "approve", &bob_req)).await;
    assert_eq!(status, StatusCode::NOT_FOUND, "nor can a bystander");

    // Approve Bob: mutual friends, the invite counts one use, and it can't be repeated.
    let (status, approved) = send(&app.router, decide(&token_a, "approve", &bob_req)).await;
    assert_eq!(status, StatusCode::OK, "body: {approved}");
    assert_eq!(approved["status"], "friends");
    for who in [&token_a, &token_b] {
        let (_, friends) = send(&app.router, friends_of(who)).await;
        assert_eq!(friends.as_array().unwrap().len(), 1);
    }
    let (status, _) = send(&app.router, decide(&token_a, "approve", &bob_req)).await;
    assert_eq!(status, StatusCode::NOT_FOUND, "already decided");
    let (_, mine) = send(&app.router, get_request("/invites", Some(&token_a))).await;
    assert_eq!(mine[0]["use_count"], 1);
    assert_eq!(mine[0]["pending_requests"], 1);

    // Decline Carol: not a friend, and asking again looks the same as pending.
    let (status, _) = send(&app.router, decide(&token_a, "decline", &carol_req)).await;
    assert_eq!(status, StatusCode::NO_CONTENT);
    let (_, retry) = send(&app.router, accept(&token_c)).await;
    assert_eq!(retry["status"], "pending", "a decline isn't revealed");
    let (_, friends) = send(&app.router, friends_of(&token_c)).await;
    assert_eq!(friends, json!([]));
    let (_, requests) = send(
        &app.router,
        get_request("/me/friend-requests", Some(&token_a)),
    )
    .await;
    assert_eq!(requests, json!([]), "a declined request doesn't come back");

    // Reusable means it still works for Dave...
    let (_, dave) = send(&app.router, accept(&token_d)).await;
    assert_eq!(dave["status"], "pending");

    // ...until Alice revokes it. Only she can, and it's idempotent.
    let revoke = |who: &str| {
        json_request(
            "DELETE",
            &format!("/invites/{token}"),
            Some(who),
            json!(null),
        )
    };
    let (status, _) = send(&app.router, revoke(&token_b)).await;
    assert_eq!(status, StatusCode::NOT_FOUND, "not your invite");
    let (status, _) = send(&app.router, revoke(&token_a)).await;
    assert_eq!(status, StatusCode::NO_CONTENT);
    let (status, _) = send(&app.router, revoke(&token_a)).await;
    assert_eq!(status, StatusCode::NO_CONTENT);
    let (status, _) = send(&app.router, get_request(&format!("/invites/{token}"), None)).await;
    assert_eq!(
        status,
        StatusCode::NOT_FOUND,
        "revoked links stop previewing"
    );
    let (status, _) = send(&app.router, accept(&token_d)).await;
    assert_eq!(status, StatusCode::BAD_REQUEST, "and stop working");
    let (_, mine) = send(&app.router, get_request("/invites", Some(&token_a))).await;
    assert_eq!(mine, json!([]));
    // Dave's request from before the revoke can still be decided.
    let (_, requests) = send(
        &app.router,
        get_request("/me/friend-requests", Some(&token_a)),
    )
    .await;
    assert_eq!(requests.as_array().unwrap().len(), 1);
}

/// No API response shown to *another* user may contain a user's id. Alice does
/// everything visible (photo, finished book, shared shelves, a recommendation,
/// invites); then everything Bob (a friend), Carol (a stranger) and the public
/// can see about her is scanned for her id — and Alice's view of Carol's friend
/// request is scanned for Carol's.
#[tokio::test]
async fn no_response_to_another_user_contains_a_user_id() {
    let app = TestApp::new().await;
    let (token_a, id_a) = onboard(&app, "hanko|a", "alice").await;
    let (token_b, id_b) = onboard(&app, "hanko|b", "bob").await;
    let (token_c, id_c) = onboard(&app, "hanko|c", "carol").await;

    let fid = befriend(&app, &token_a, &token_b).await;

    // Alice: a profile photo, shared shelves, a finished book, a recommendation.
    let (_, ticket) = send(
        &app.router,
        json_request("POST", "/me/avatar-upload", Some(&token_a), json!({})),
    )
    .await;
    let avatar = ticket["avatar_url"].as_str().unwrap();
    send(
        &app.router,
        json_request(
            "PATCH",
            "/me",
            Some(&token_a),
            json!({ "avatar_url": avatar, "share_shelves": true }),
        ),
    )
    .await;
    let book = resolve_book(&app, &token_a, "OL-leak").await;
    send(
        &app.router,
        json_request(
            "PUT",
            "/book-statuses",
            Some(&token_a),
            json!({ "book_id": book, "status": "finished", "rating": 4 }),
        ),
    )
    .await;
    send(
        &app.router,
        json_request(
            "POST",
            "/recommendations",
            Some(&token_a),
            json!({ "to_friendship_id": fid, "book_id": book, "note": "read this" }),
        ),
    )
    .await;

    // A reusable invite Carol asks to join through.
    let (_, invite) = send(
        &app.router,
        json_request("POST", "/invites?reusable=true", Some(&token_a), json!({})),
    )
    .await;
    let token = invite["token"].as_str().unwrap().to_string();

    let mut seen_by_others: Vec<(&str, Value)> = Vec::new();
    let fetch = |label: &'static str, req| {
        let router = app.router.clone();
        async move { (label, send(&router, req).await) }
    };
    for (label, (status, body)) in [
        fetch("bob: friends", get_request("/me/friends", Some(&token_b))).await,
        fetch(
            "bob: profile",
            get_request(&format!("/friends/{fid}"), Some(&token_b)),
        )
        .await,
        fetch(
            "bob: shelves",
            get_request(&format!("/friends/{fid}/library"), Some(&token_b)),
        )
        .await,
        fetch(
            "bob: timeline",
            get_request(&format!("/users/{id_b}/feed"), Some(&token_b)),
        )
        .await,
        fetch(
            "bob: inbox",
            get_request(
                &format!("/users/{id_b}/recommendations/inbox"),
                Some(&token_b),
            ),
        )
        .await,
        fetch(
            "public: invite preview",
            get_request(&format!("/invites/{token}"), None),
        )
        .await,
        fetch(
            "carol: accepting",
            json_request(
                "POST",
                &format!("/invites/{token}/accept"),
                Some(&token_c),
                json!({}),
            ),
        )
        .await,
        fetch("carol: friends", get_request("/me/friends", Some(&token_c))).await,
    ] {
        assert_eq!(status, StatusCode::OK, "{label}: {body}");
        seen_by_others.push((label, body));
    }

    for (label, body) in &seen_by_others {
        assert!(
            !body.to_string().contains(&id_a),
            "{label} leaked Alice's user id: {body}"
        );
    }
    // Positive control: the same check *does* find her id where it belongs (her
    // own profile), so the scan above would catch a leak.
    let (_, mine) = send(&app.router, get_request("/me", Some(&token_a))).await;
    assert!(
        mine.to_string().contains(&id_a),
        "control: /me shows your own id to you"
    );
    assert!(
        seen_by_others
            .iter()
            .any(|(l, b)| *l == "bob: shelves" && !b.as_array().unwrap().is_empty()),
        "sanity: Bob really did see Alice's shelves"
    );

    // Alice, in turn, sees Carol only as a name + photo on a pending request.
    let (_, requests) = send(
        &app.router,
        get_request("/me/friend-requests", Some(&token_a)),
    )
    .await;
    assert_eq!(requests.as_array().unwrap().len(), 1);
    assert!(
        !requests.to_string().contains(&id_c),
        "a friend request must not contain the requester's user id: {requests}"
    );
    assert!(
        requests[0].get("handle").is_none() && requests[0].get("user_id").is_none(),
        "nor a handle or user_id field"
    );
}

/// Replacing or removing a profile photo deletes the old file (a DELETE to blob
/// storage with a delete-only SAS); setting a photo when there was none, or
/// re-saving the same one, deletes nothing; and a storage failure never fails
/// the profile update.
#[tokio::test]
async fn replacing_or_removing_a_photo_deletes_the_old_file() {
    let app = TestApp::new().await;
    let (token, _id) = onboard(&app, "hanko|a", "alice").await;

    let patch = |body: Value| json_request("PATCH", "/me", Some(&token), body);
    let mint = || async {
        let (_, ticket) = send(
            &app.router,
            json_request("POST", "/me/avatar-upload", Some(&token), json!({})),
        )
        .await;
        ticket["avatar_url"].as_str().unwrap().to_string()
    };
    // Paths of the blobs that were deleted, in order.
    let deleted = || async {
        app.storage_server
            .received_requests()
            .await
            .unwrap()
            .into_iter()
            .filter(|r| r.method.as_str() == "DELETE")
            .map(|r| {
                assert!(
                    r.url.query().unwrap_or("").contains("sp=d"),
                    "delete must use a delete-only SAS: {}",
                    r.url
                );
                r.url.path().to_string()
            })
            .collect::<Vec<_>>()
    };
    let path_of = |url: &str| reqwest::Url::parse(url).unwrap().path().to_string();

    // No photo yet -> the first one has nothing to delete.
    let first = mint().await;
    send(&app.router, patch(json!({ "avatar_url": first }))).await;
    assert!(deleted().await.is_empty());

    // Replacing it deletes the first file.
    let second = mint().await;
    let (status, me) = send(&app.router, patch(json!({ "avatar_url": second }))).await;
    assert_eq!(status, StatusCode::OK, "body: {me}");
    assert_eq!(me["avatar_url"], second);
    assert_eq!(deleted().await, [path_of(&first)]);

    // Re-saving the same photo, or editing something else, deletes nothing.
    send(&app.router, patch(json!({ "avatar_url": second }))).await;
    send(&app.router, patch(json!({ "display_name": "Alice L." }))).await;
    assert_eq!(deleted().await.len(), 1);

    // Removing the photo deletes the file too; removing again is a no-op.
    let (_, me) = send(&app.router, patch(json!({ "avatar_url": null }))).await;
    assert!(me["avatar_url"].is_null());
    assert_eq!(deleted().await, [path_of(&first), path_of(&second)]);
    send(&app.router, patch(json!({ "avatar_url": null }))).await;
    assert_eq!(deleted().await.len(), 2, "nothing left to delete");

    // A storage failure doesn't block the profile change.
    let third = mint().await;
    send(&app.router, patch(json!({ "avatar_url": third }))).await;
    Mock::given(method("DELETE"))
        .respond_with(ResponseTemplate::new(500))
        .with_priority(1)
        .mount(&app.storage_server)
        .await;
    let (status, me) = send(&app.router, patch(json!({ "avatar_url": null }))).await;
    assert_eq!(status, StatusCode::OK, "body: {me}");
    assert!(me["avatar_url"].is_null(), "the profile still updated");
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

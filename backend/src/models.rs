use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use sqlx::FromRow;
use uuid::Uuid;

#[derive(Debug, Serialize, Deserialize, FromRow)]
pub struct User {
    pub id: Uuid,
    pub handle: String,
    pub display_name: String,
    pub avatar_url: Option<String>,
    pub locale: String,
    /// Whether friends may read this user's library (`GET /users/{id}/library`).
    pub share_shelves: bool,
    pub created_at: DateTime<Utc>,
}

/// `PATCH /me` body: only the fields present are changed. `avatar_url` also
/// distinguishes absent (leave alone) from `null` (remove the avatar).
#[derive(Debug, Deserialize)]
pub struct UpdateMe {
    pub share_shelves: Option<bool>,
    pub display_name: Option<String>,
    #[serde(default, deserialize_with = "present_or_null")]
    pub avatar_url: Option<Option<String>>,
}

fn present_or_null<'de, D, T>(d: D) -> Result<Option<Option<T>>, D::Error>
where
    D: serde::Deserializer<'de>,
    T: Deserialize<'de>,
{
    Option::<T>::deserialize(d).map(Some)
}

/// `POST /me/avatar-upload` response: see `storage.rs` for the flow.
#[derive(Debug, Serialize)]
pub struct AvatarUploadTicket {
    pub upload_url: String,
    pub avatar_url: String,
    pub expires_at: DateTime<Utc>,
}

#[derive(Debug, Deserialize)]
pub struct CreateUser {
    pub handle: String,
    pub display_name: String,
    pub locale: Option<String>,
}

#[derive(Debug, Serialize, Deserialize, FromRow)]
pub struct Book {
    pub id: Uuid,
    pub canonical_title: String,
    pub primary_author: Option<String>,
    pub open_library_work_id: Option<String>,
    pub google_books_volume_id: Option<String>,
    pub cover_image_url: Option<String>,
    /// Plain-text blurb from Open Library / Google Books, when a source has one.
    pub description: Option<String>,
    pub created_at: DateTime<Utc>,
}

#[derive(Debug, Serialize, Deserialize, FromRow)]
pub struct BookEdition {
    pub id: Uuid,
    pub book_id: Uuid,
    pub language: String,
    pub isbn_13: Option<String>,
    pub isbn_10: Option<String>,
    pub title: String,
    pub publisher: Option<String>,
    pub cover_image_url: Option<String>,
    pub source: String,
    pub source_id: String,
    pub created_at: DateTime<Utc>,
}

/// Input for resolving/upserting a book from an external source (Open Library or Google Books).
/// The book data service is responsible for producing this after normalization.
#[derive(Debug, Deserialize)]
pub struct ResolvedBook {
    pub canonical_title: String,
    pub primary_author: Option<String>,
    pub language: String,
    pub isbn_13: Option<String>,
    pub isbn_10: Option<String>,
    pub edition_title: String,
    pub publisher: Option<String>,
    pub cover_image_url: Option<String>,
    pub source: String,
    pub source_id: String,
    pub open_library_work_id: Option<String>,
    pub google_books_volume_id: Option<String>,
    /// Optional so manual entries and older clients needn't send it.
    #[serde(default)]
    pub description: Option<String>,
}

#[derive(Debug, Clone, Copy, sqlx::Type, Serialize, Deserialize, PartialEq, Eq)]
#[sqlx(type_name = "reading_status", rename_all = "snake_case")]
#[serde(rename_all = "snake_case")]
pub enum ReadingStatus {
    WantToRead,
    CurrentlyReading,
    Finished,
    DidNotFinish,
}

#[derive(Debug, Serialize, Deserialize, FromRow)]
pub struct BookStatus {
    pub id: Uuid,
    pub user_id: Uuid,
    pub book_id: Uuid,
    pub status: ReadingStatus,
    pub progress_percent: Option<i16>,
    /// 1-5, only set once status is `finished` or `did_not_finish`.
    pub rating: Option<i16>,
    pub updated_at: DateTime<Utc>,
    pub created_at: DateTime<Utc>,
    /// True if this row's *current* status was set via the backlog
    /// (`backdated: true`) flow — lets the client show e.g. a "logged as
    /// backlog" badge. Cleared automatically the next time the status
    /// actually changes (see `set_status`'s upsert), so a real reread un-badges
    /// it; re-submitting the same status (editing the rating, say) does not.
    pub backdated: bool,
}

#[derive(Debug, Deserialize)]
pub struct SetBookStatus {
    /// The acting user is taken from the auth token, not the body.
    pub book_id: Uuid,
    pub status: ReadingStatus,
    pub progress_percent: Option<i16>,
    /// 1-5. Allowed only with `finished` / `did_not_finish`; cleared on any
    /// other status.
    pub rating: Option<i16>,
    /// True for logging a book read before the user had the app — suppresses
    /// the activity_events row / feed entry this status change would
    /// otherwise generate. Defaults to false (omitting it is the normal
    /// "I just did this" path).
    #[serde(default)]
    pub backdated: bool,
}

#[derive(Debug, Serialize, Deserialize, FromRow)]
pub struct Recommendation {
    pub id: Uuid,
    pub from_user_id: Uuid,
    pub to_user_id: Uuid,
    pub book_id: Uuid,
    pub note: Option<String>,
    pub created_at: DateTime<Utc>,
}

#[derive(Debug, Deserialize)]
pub struct CreateRecommendation {
    /// The sender is taken from the auth token, not the body.
    pub to_user_id: Uuid,
    pub book_id: Uuid,
    pub note: Option<String>,
}

#[derive(Debug, Serialize, Deserialize, FromRow)]
pub struct Friendship {
    pub id: Uuid,
    pub user_a_id: Uuid,
    pub user_b_id: Uuid,
    pub created_at: DateTime<Utc>,
}

/// Response for `POST /invites`. The client builds the actual shareable
/// link/QR payload from `token` (a `shelfcircle://invite/{token}` deep link
/// today) — the server doesn't need to know the app's URL scheme.
#[derive(Debug, Serialize)]
pub struct Invite {
    pub token: String,
    pub expires_at: DateTime<Utc>,
    /// True for an "anyone with the link" invite (several people may use it and
    /// each needs the issuer's approval); false for a single-use invite.
    pub reusable: bool,
}

/// Response for `GET /invites/{token}` (public, no auth): just enough for the
/// client to show "so-and-so wants to be your friend" before the recipient
/// has signed in — never the inviter's handle/id.
#[derive(Debug, Serialize, FromRow)]
pub struct InvitePreview {
    pub display_name: String,
    pub avatar_url: Option<String>,
    /// Accepting sends a request the inviter must approve (reusable invites)
    /// instead of connecting you immediately.
    pub requires_approval: bool,
}

/**
 * Mirrors `backend/src/models.rs` and the route handlers under
 * `backend/src/routes/`. Keep in sync with the backend, not with
 * `frontend.md` at the repo root — that spec predates the real Hanko-based
 * auth and several request shapes changed since (see `docs/backend-contract.md`).
 */

export type UUID = string;
export type Timestamp = string; // RFC 3339 UTC

export type ReadingStatus = 'want_to_read' | 'currently_reading' | 'finished' | 'did_not_finish';

export interface User {
  id: UUID;
  handle: string;
  display_name: string;
  avatar_url: string | null;
  locale: string;
  /** Whether friends can read this user's library (opt-in, off by default). */
  share_shelves: boolean;
  created_at: Timestamp;
}

/** GET /me/reading-stats — completions in a calendar year, read in `time_zone`. */
export interface ReadingStats {
  year: number;
  time_zone: string;
  /** Books finished that year; rereads count each time, backlog (backdated) entries don't. */
  completed: number;
  /** Per month, January first. */
  by_month: number[];
  goal: ReadingGoal | null;
}

export interface ReadingGoal {
  year: number;
  target_count: number;
  time_zone: string;
}

/** PATCH /me body — only the fields present change. */
export interface UpdateMeInput {
  share_shelves?: boolean;
  display_name?: string;
  /** A URL from `POST /me/avatar-upload`, or `null` to remove the photo. */
  avatar_url?: string | null;
}

export interface LibrarySystem {
  id: string;
  name: string;
}

/** GET/PUT /me/library-system */
export interface MyLibrarySystem {
  library_system: LibrarySystem | null;
}

/** GET /books/{id}/library-link — `url` is always usable. */
export interface BookLibraryLink {
  library: LibrarySystem;
  /** The catalog has this edition; `url` is its record page. Otherwise `url` is a catalog search. */
  found: boolean;
  /** The catalog couldn't be reached, so `found: false` means "unknown". */
  lookup_failed: boolean;
  url: string;
}

/** POST /me/avatar-upload — PUT the JPEG to `upload_url`, then PATCH `avatar_url`. */
export interface AvatarUploadTicket {
  upload_url: string;
  avatar_url: string;
  expires_at: Timestamp;
}

export interface CreateUserInput {
  handle: string;
  display_name: string;
  locale?: string;
}

export interface Book {
  id: UUID;
  canonical_title: string;
  primary_author: string | null;
  open_library_work_id: string | null;
  google_books_volume_id: string | null;
  cover_image_url: string | null;
  /** Plain-text blurb, only when a source (Open Library / Google Books) has one. */
  description: string | null;
  created_at: Timestamp;
}

export interface BookEdition {
  id: UUID;
  book_id: UUID;
  language: string;
  isbn_13: string | null;
  isbn_10: string | null;
  title: string;
  publisher: string | null;
  cover_image_url: string | null;
  source: string;
  source_id: string;
  created_at: Timestamp;
}

export type BookWithEdition = Book & { edition: BookEdition };

export interface BookSearchResult {
  source: 'open_library' | 'google_books';
  source_id: string;
  title: string;
  authors: string[];
  first_publish_year: number | null;
  cover_image_url: string | null;
  languages: string[];
  open_library_work_id: string | null;
  google_books_volume_id: string | null;
}

/** POST /books/resolve body when hand-entering a book not in any catalog. */
export interface ResolvedBookInput {
  canonical_title: string;
  primary_author: string | null;
  language: string;
  isbn_13: string | null;
  isbn_10: string | null;
  edition_title: string;
  publisher: string | null;
  cover_image_url: string | null;
  source: 'manual';
  source_id: string;
  open_library_work_id: null;
  google_books_volume_id: null;
}

export interface BookStatus {
  id: UUID;
  user_id: UUID;
  book_id: UUID;
  status: ReadingStatus;
  progress_percent: number | null;
  rating: number | null;
  updated_at: Timestamp;
  created_at: Timestamp;
  /** True while this row's *current* status is the one that was logged via
   * the backlog flow — cleared server-side the next time the status
   * actually changes (a real reread). */
  backdated: boolean;
}

/** PUT /book-statuses body — the acting user comes from the auth token. */
export interface SetBookStatusInput {
  book_id: UUID;
  status: ReadingStatus;
  progress_percent?: number | null;
  rating?: number | null;
  /** True for a book read before using the app — suppresses the feed entry
   * this status change would otherwise generate. Defaults to false. */
  backdated?: boolean;
}

export interface LibraryEntry {
  status: ReadingStatus;
  rating: number | null;
  progress_percent: number | null;
  updated_at: Timestamp;
  book: {
    id: UUID;
    title: string;
    author: string | null;
    cover_image_url: string | null;
  };
}

export type LibraryShelf = 'reading' | 'read' | 'want_to_read' | 'did_not_finish' | 'all';

/**
 * Friends are referenced by the *friendship* (`friendship_id`, shared only by the
 * two of you), never by a user id: the API never returns one user's id to
 * another. `handle` is visible to friends only.
 */
export interface Friend {
  friendship_id: UUID;
  handle: string;
  display_name: string;
  avatar_url: string | null;
}

/** GET /friends/{friendship_id}. */
export interface FriendProfile extends Friend {
  /** Whether they let friends see their bookshelves. */
  share_shelves: boolean;
}

/** An entry in your recommendations inbox. */
export interface RecommendationItem {
  id: UUID;
  book_id: UUID;
  note: string | null;
  created_at: Timestamp;
  from: {
    /** Null only if you're no longer friends. */
    friendship_id: UUID | null;
    handle: string;
    display_name: string;
    avatar_url: string | null;
  };
}

/** POST /recommendations body — the sender comes from the auth token, and the
 * recipient is one of your friends, named by friendship. */
export interface CreateRecommendationInput {
  to_friendship_id: UUID;
  book_id: UUID;
  note?: string | null;
}

/** POST /invites response — the client builds the shareable deep link/QR
 * payload from `token` itself (see `buildInviteUrl`); the server doesn't
 * know about the app's URL scheme. */
export interface Invite {
  token: string;
  expires_at: Timestamp;
  /** "Anyone with the link": several people may use it, each needing your approval. */
  reusable: boolean;
}

/** One of your own invites that can still be used (GET /invites). */
export interface MyInvite {
  token: string;
  reusable: boolean;
  expires_at: Timestamp;
  /** People who have become friends through it. */
  use_count: number;
  /** Requests waiting for your approval. */
  pending_requests: number;
}

/** POST /invites/{token}/accept and approve. `pending`: the inviter has to
 * approve. `friendship_id` is an opaque id for the friendship, not a user id. */
export interface AcceptResult {
  status: 'friends' | 'pending';
  friendship_id: UUID | null;
}

/** Someone asking to join through one of your reusable invites. Not a friend
 * yet, so only a name and photo — never a handle or user id. */
export interface FriendRequest {
  id: UUID;
  display_name: string;
  avatar_url: string | null;
  created_at: Timestamp;
}

/** GET /invites/{token} response (public — no auth). Never the inviter's
 * handle/id, just enough to show "so-and-so wants to be your friend". */
export interface InvitePreview {
  display_name: string;
  avatar_url: string | null;
  /** Accepting sends a request they must approve, instead of connecting you at once. */
  requires_approval: boolean;
}

export interface FeedItem {
  id: UUID;
  created_at: Timestamp;
  status: ReadingStatus;
  verb: string;
  actor: {
    /** The friendship with this person; null for your own events. Never a user id. */
    friendship_id: UUID | null;
    is_me: boolean;
    handle: string;
    display_name: string;
    avatar_url: string | null;
  };
  book: { id: UUID; title: string; author: string | null; cover_image_url: string | null };
}

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

/** PATCH /me body — only the fields present change. */
export interface UpdateMeInput {
  share_shelves?: boolean;
}

export interface CreateUserInput {
  handle: string;
  display_name: string;
  locale?: string;
}

export interface Friendship {
  id: UUID;
  user_a_id: UUID;
  user_b_id: UUID;
  created_at: Timestamp;
}

export interface Book {
  id: UUID;
  canonical_title: string;
  primary_author: string | null;
  open_library_work_id: string | null;
  google_books_volume_id: string | null;
  cover_image_url: string | null;
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

export interface Recommendation {
  id: UUID;
  from_user_id: UUID;
  to_user_id: UUID;
  book_id: UUID;
  note: string | null;
  created_at: Timestamp;
}

/** POST /recommendations body — the sender comes from the auth token. */
export interface CreateRecommendationInput {
  to_user_id: UUID;
  book_id: UUID;
  note?: string | null;
}

/** POST /invites response — the client builds the shareable deep link/QR
 * payload from `token` itself (see `buildInviteUrl`); the server doesn't
 * know about the app's URL scheme. */
export interface Invite {
  token: string;
  expires_at: Timestamp;
}

/** GET /invites/{token} response (public — no auth). Never the inviter's
 * handle/id, just enough to show "so-and-so wants to be your friend". */
export interface InvitePreview {
  display_name: string;
  avatar_url: string | null;
}

export interface FeedItem {
  id: UUID;
  created_at: Timestamp;
  status: ReadingStatus;
  verb: string;
  actor: { id: UUID; handle: string; display_name: string; avatar_url: string | null };
  book: { id: UUID; title: string; author: string | null; cover_image_url: string | null };
}

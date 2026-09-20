# Spec: Shelf Circle mobile app (React Native)

Implementation brief for the front-end agent. Describes every screen and
behaviour the v1 app must ship, and the exact backend contract it runs against.

Read `spec/architecture.md` for product context and `spec/friends.md` for the
(not-yet-built) invite flow this app should be structured to accept later.

> **Status note:** this brief was written before real auth landed. §3 and §5
> below describe the original no-auth design; the backend now requires a
> Hanko Cloud JWT on every route (except onboarding, which needs a valid
> token but no profile row yet). The corrections are called out inline where
> they matter — treat §3/§5's *shape* (what data flows where) as right and
> their *auth-free* framing as superseded. `frontend/src/auth/` implements
> the real flow.

---

## 1. Product in one paragraph

A friends-only social layer around reading. You track what you're reading /
have read, rate books when you finish (or abandon) them, recommend specific
books to specific friends, and watch a timeline of what your friends are
reading. It is deliberately **not** a public review network. Non-commercial:
no ads, no growth mechanics. Primary "get this book" action will be *find at
your library* (not built yet — see §9).

---

## 2. Stack & conventions

- **Expo + React Native**, TypeScript, React Navigation (native stack + bottom
  tabs).
- **TanStack Query (React Query)** for all server state: caching, retries,
  pagination, optimistic updates. Do not hand-roll fetch state.
- One typed API client module wrapping `fetch`; base URL from
  `EXPO_PUBLIC_API_BASE_URL` (e.g. `http://localhost:8080` in dev).
- `AsyncStorage` for the local identity (§5) and lightweight UI prefs.
  `expo-secure-store` is fine too; nothing sensitive is stored yet.
- i18n-ready copy (`i18next` / `expo-localization`), English only for v1. Book
  data is inherently multi-language (see language fields everywhere).
- No API keys or secrets in the app. The Google Books key lives only on the
  backend.

---

## 3. Backend contract

Base URL: `EXPO_PUBLIC_API_BASE_URL`. JSON in, JSON out. All timestamps are
RFC 3339 UTC strings.

**Superseded:** every route below now requires `Authorization: Bearer
<Hanko JWT>` (see §5) — the acting user comes from the token, not from an
explicit id in the request. Where a row below still shows a body containing
`user_id` / `user_handle_a` / `from_user_id` etc., drop that field; the
server infers it. `GET /users/{id}/book-statuses`, `/library`,
`/recommendations/inbox`, and `/feed` are also **self-only** now (a 403 if
`{id}` isn't the caller) — see §6.9 for what this breaks.

### Error format

Any non-2xx response body is `{ "error": string }`.

| Status | Meaning | App handling |
|---|---|---|
| 400 | Validation failure; `error` is user-safe | Show the message inline |
| 404 | Not found | Screen-level empty/error state |
| 502 | Book provider (Open Library / Google Books) unavailable | "Search is having trouble" + retry |
| 500 | `error` is always `"internal error"` | Generic "Something went wrong" + retry |

### Endpoints

| Method & path | Body | Returns | Notes |
|---|---|---|---|
| `GET /health` | — | `ok` (text) | connectivity check |
| `POST /users` | `{ handle, display_name, locale? }` | `User` | `locale` defaults `"en"`. Onboarding — the Hanko user id/email come from the token. **Duplicate `handle` (or a second call for the same token) is 409**, not 500. |
| `GET /users/{id}` | — | `User` \| 404 | |
| `GET /users/by-handle/{handle}` | — | `User` \| 404 | how you "look someone up" |
| `POST /friendships` | `{ user_handle }` | `Friendship` | Friends *you* (the token's user) to `user_handle`. Mutual **immediately**, no request/accept step. Idempotent. Rows are canonicalized (`user_a_id < user_b_id`) — don't assume a↔b matches what you sent. 400 if the handle doesn't exist or it's your own. |
| `GET /books/search?q=&limit=` | — | `BookSearchResult[]` | `limit` 1–40, default 20. Work-level, **not persisted**. Merges Open Library + Google Books. May be slow (1–4 s) or 502. |
| `POST /books/resolve` | provider ref **or** full `ResolvedBook` | `BookWithEdition` | Upserts the book. Send `{ source, source_id }` for a search result; send a full `ResolvedBook` with `source: "manual"` for a hand-entered book. Idempotent on `(source, source_id)`. |
| `GET /books/{id}` | — | `Book` \| 404 | |
| `PUT /book-statuses` | `{ book_id, status, progress_percent?, rating? }` | `BookStatus` | Upsert (one status per user+book), for the token's user. `rating` (1–5) **only** valid with `status` `finished` / `did_not_finish`; sending it otherwise is 400, and it is cleared when moving to another status. Re-sending the same `status` with a new `progress_percent` does **not** create a timeline event. |
| `GET /users/{id}/book-statuses` | — | `BookStatus[]` | **self-only** — `{id}` must be the caller (403 otherwise). Raw rows, no book detail, newest-updated first. |
| `GET /users/{id}/library?shelf=` | — | `LibraryEntry[]` | **self-only** — `{id}` must be the caller (403 otherwise). `shelf` ∈ `reading` \| `read` \| `want_to_read` \| `did_not_finish` \| `all`; omit = all. `read` = finished ∪ did_not_finish. Includes book detail + rating. |
| `POST /recommendations` | `{ to_user_id, book_id, note? }` | `Recommendation` | From the token's user. `book_id` must already exist — resolve first. Not deduped: sending twice makes two rows. |
| `GET /users/{id}/recommendations/inbox` | — | `Recommendation[]` | **self-only** — `{id}` must be the caller (403 otherwise). Recs sent **to** this user, newest first. No book/sender detail — hydrate client-side. |
| `GET /users/{id}/feed?limit=&before=` | — | `FeedItem[]` | **self-only** — `{id}` must be the caller (403 otherwise). The timeline. Friends **+ self**. `limit` 1–100, default 50. `before` = RFC 3339; pass the last item's `created_at` for the next page. Reading-status events only. |

### TypeScript types (authoritative — mirror the backend)

```ts
type UUID = string;
type Timestamp = string; // RFC 3339 UTC, e.g. "2026-09-06T21:27:36.123456Z"
type ReadingStatus =
  | 'want_to_read'
  | 'currently_reading'
  | 'finished'
  | 'did_not_finish';

interface User {
  id: UUID;
  handle: string;
  display_name: string;
  avatar_url: string | null;
  locale: string;           // BCP-47
  created_at: Timestamp;
}

interface Friendship {
  id: UUID;
  user_a_id: UUID;          // canonicalized: user_a_id < user_b_id
  user_b_id: UUID;
  created_at: Timestamp;
}

interface Book {
  id: UUID;
  canonical_title: string;
  primary_author: string | null;
  open_library_work_id: string | null;
  google_books_volume_id: string | null;
  cover_image_url: string | null;
  created_at: Timestamp;
}

interface BookEdition {
  id: UUID;
  book_id: UUID;
  language: string;         // BCP-47
  isbn_13: string | null;
  isbn_10: string | null;
  title: string;            // title as printed in this edition/language
  publisher: string | null;
  cover_image_url: string | null;
  source: string;           // 'open_library' | 'google_books' | 'manual'
  source_id: string;
  created_at: Timestamp;
}

// POST /books/resolve response
interface BookWithEdition extends Book {
  edition: BookEdition;
}

// GET /books/search item
interface BookSearchResult {
  source: 'open_library' | 'google_books';
  source_id: string;
  title: string;
  authors: string[];
  first_publish_year: number | null;
  cover_image_url: string | null;
  languages: string[];      // BCP-47, best effort
  open_library_work_id: string | null;
  google_books_volume_id: string | null;
}

// POST /books/resolve request — manual entry variant
interface ResolvedBook {
  canonical_title: string;
  primary_author: string | null;
  language: string;         // BCP-47
  isbn_13: string | null;
  isbn_10: string | null;
  edition_title: string;
  publisher: string | null;
  cover_image_url: string | null;
  source: string;           // use "manual"
  source_id: string;        // stable app-generated id, e.g. "manual:" + uuid
  open_library_work_id: string | null;
  google_books_volume_id: string | null;
}

interface BookStatus {
  id: UUID;
  user_id: UUID;
  book_id: UUID;
  status: ReadingStatus;
  progress_percent: number | null; // 0–100
  rating: number | null;           // 1–5, only with finished / did_not_finish
  updated_at: Timestamp;
  created_at: Timestamp;
}

interface LibraryEntry {
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

interface Recommendation {
  id: UUID;
  from_user_id: UUID;
  to_user_id: UUID;
  book_id: UUID;
  note: string | null;
  created_at: Timestamp;
}

interface FeedItem {
  id: UUID;
  created_at: Timestamp;
  status: ReadingStatus;
  verb: string;             // "started reading" | "finished" | "wants to read" | "did not finish"
  actor: { id: UUID; handle: string; display_name: string; avatar_url: string | null };
  book:  { id: UUID; title: string; author: string | null; cover_image_url: string | null };
}
```

---

## 4. Navigation map

```
Root (driven by AuthContext's `status`)
├─ AuthFlow          (signed-out: Hanko login/registration, §5)
├─ CreateIdentity    (onboarding: token valid, no profile row yet, §5)
└─ Main (bottom tabs)
    ├─ Timeline        → BookDetail
    ├─ My Books        → BookDetail, AddPastRead, BookSearch
    ├─ Recommendations → BookDetail
    ├─ Friends         → AddFriend, FriendProfile
    └─ Me              → EditProfile
Modal / pushed from multiple places:
    ├─ BookSearch      → BookDetail
    ├─ BookDetail      → RecommendToFriend
    ├─ AddPastRead     (BookSearch + manual-entry form) → BookDetail
    └─ RecommendToFriend
```

---

## 5. Identity & onboarding (superseded — real Hanko auth)

This section originally specced a local, auth-free identity. That's gone —
real auth landed first. What's actually built (`frontend/src/auth/`):

1. On launch, `AuthProvider` reads a stored Hanko session token
   (`expo-secure-store`) and calls `GET /me`. Three outcomes:
   - no token → **AuthFlow** (sign in or create an account — see point 2
     below).
   - token valid, `GET /me` 404 → **onboarding** (profile row doesn't exist
     yet for this token).
   - token valid, `GET /me` 200 → signed in, straight to Main.
2. **AuthFlow screen** drives Hanko's Flow API (`/login` or `/registration`,
   user-toggleable) generically — it renders whatever actions/inputs the
   current flow state returns rather than hardcoding step names, since the
   exact steps depend on the Hanko Cloud project's configured login methods.
   Success yields a session JWT (`X-Auth-Token` response header), stored via
   `completeWithToken`.
3. **Create identity** (onboarding, shown once per new account): fields
   `display_name` and `handle` (lowercase, `[a-z0-9_]`, client-validated).
   Calls `POST /users` using the Hanko token (the server reads the Hanko user
   id/email from the token, not the request body).
   - On success: `completeOnboarding` stores the returned `User`, enters Main.
   - On 409: "That handle may be taken — try another."
4. `useAuth()` exposes `{ status, user, ... }`; `status` drives top-level
   navigation (`RootNavigator`). There is no more "local identity swap" —
   signing in on a new device is just signing in again with Hanko.
5. **Me** tab "Sign out" clears the stored token and returns to AuthFlow
   (does not delete the server user or the Hanko identity).
6. **Known gap:** no session refresh. Hanko JWTs are short-lived; an expired
   token just sends the user back through AuthFlow rather than silently
   refreshing.

---

## 6. Screens

Each screen: **purpose · data in · actions · states**. "Invalidate X" means a
React Query cache invalidation after a mutation.

### 6.1 Timeline (home tab)

- **Purpose:** the core loop — a reverse-chronological feed of friends' (and
  your own) reading activity.
- **Data:** `GET /users/{me}/feed?limit=50`, infinite scroll via `before` =
  `created_at` of the last loaded item. Pull-to-refresh refetches page 1.
- **Row:** actor avatar (fallback to initials) · `"{actor.display_name} {verb}
  {book.title}"` · book thumbnail (`book.cover_image_url`, placeholder if null)
  · relative time (`created_at`). Tap → BookDetail for `book.id`.
- **States:**
  - Empty (no events yet): "Your timeline is quiet. Add friends or shelve a
    book." with buttons to Friends and BookSearch.
  - Loading: skeleton rows. Error: retry button.
- **Notes:** feed rows do **not** carry the rating even for `finished`/`did_not_finish`
  events (§9). Render status only. Duplicate-looking entries across pages are
  possible if two events share a timestamp — de-dupe by `id` when appending.

### 6.2 My Books (library tab)

- **Purpose:** manage your own shelves.
- **Layout:** segmented control **Reading · Read · Want to read**, each backed
  by `GET /users/{me}/library?shelf=reading|read|want_to_read`.
- **Reading tab:** each row shows cover, title, author, a progress bar
  (`progress_percent`). Row actions: **Update progress** (slider 0–100 →
  `PUT /book-statuses` same status + new `progress_percent`), **Mark finished**
  (→ opens the Finish sheet, §6.5).
- **Read tab:** cover, title, author, star rating (`rating`, may be null),
  and a small `did_not_finish` badge where applicable. Header button
  **"+ Add a book I've read"** → AddPastRead (§6.4). Tap row → BookDetail.
- **Want to read tab:** cover, title, author. Row action **Start reading**
  (→ `PUT /book-statuses` `currently_reading`). Header button **"+ Find a
  book"** → BookSearch.
- **States:** per-tab empty copy; loading skeletons; error retry.
- After any mutation: invalidate the affected `library` shelves, the book's
  `book-statuses`, and `feed` (a status change may create a timeline event).

### 6.3 Book search

- **Purpose:** find a book in external catalogs and pull it into the app.
- **Data:** debounced `GET /books/search?q=&limit=20`. Show a hint that results
  come from Open Library / Google Books.
- **Row:** cover, title, `authors.join(", ")`, `first_publish_year`, language
  badges (`languages`). Tap a result:
  1. `POST /books/resolve` with `{ source, source_id }` → `BookWithEdition`.
  2. Navigate to BookDetail for the returned `id`.
- **States:** idle (prompt), loading spinner, no results, 502 → "Search is
  having trouble, try again."
- Entry points: My Books → Want to read / Reading; BookDetail's "recommend"
  path when the book isn't in the app yet; AddPastRead.

### 6.4 Add a past read (AddPastRead)

- **Purpose:** log a book already finished off-platform, including obscure books
  not in any catalog.
- **Flow:**
  - Default sub-view = BookSearch (§6.3) but after `resolve`, instead of going
    to BookDetail, go straight to the Finish sheet (§6.5) pre-set to `finished`.
  - **"Can't find it? Add manually"** → form: title (req), author, language
    (BCP-47 picker, default from `currentUser.locale`), publisher, ISBN-13,
    ISBN-10, edition title (defaults to title). On submit:
    `POST /books/resolve` with a full `ResolvedBook`, `source: "manual"`,
    `source_id: "manual:" + uuidv4()`, other external ids `null`.
  - Then Finish sheet → `PUT /book-statuses` `finished` (+ optional rating).
- After: invalidate `library?shelf=read`, `feed`.

### 6.5 Finish sheet (shared component)

- Trigger: "Mark finished" / "Did not finish" from BookDetail, My Books, or
  AddPastRead.
- Fields: status toggle **Finished / Didn't finish**, star rating 1–5
  (optional, clearable).
- Submit: `PUT /book-statuses` `{ user_id: me, book_id, status, rating }`.
- Validation: rating only sent with a finish status (the backend enforces this
  too). Clearing the rating = send `rating: null`.

### 6.6 Book detail

- **Purpose:** everything about one book + your relationship to it.
- **Data:**
  - `GET /books/{id}` → `Book` (title, author, cover, external ids).
  - Your status: from `GET /users/{me}/book-statuses` (find the row for this
    `book_id`) — cache this list and read from it.
- **Sections:**
  - Header: cover, `canonical_title`, `primary_author`.
  - **Your shelf control:** segmented `Want to read · Reading · Finished ·
    Didn't finish` → `PUT /book-statuses`. When Reading: progress slider. When
    a finish state: star rating (opens/uses Finish sheet).
  - **Recommend to a friend** button → RecommendToFriend (§6.8).
  - **Editions / languages:** the resolve response carried one `edition`; show
    its language, publisher, ISBNs. (There is no "list all editions" endpoint
    yet — show what you have.)
  - **Placeholders (disabled, labelled "coming soon"):** "Find at your library",
    "Buy a copy". Do not build these — see §9.
- After mutations: invalidate `book-statuses`, relevant `library` shelves,
  `feed`. Use optimistic updates for the shelf control.

### 6.7 Recommendations (inbox tab)

- **Purpose:** "X recommended a book to you."
- **Data:** `GET /users/{me}/recommendations/inbox` → `Recommendation[]`.
  Each row needs hydration:
  - sender: `GET /users/{from_user_id}` (cache by id).
  - book: `GET /books/{book_id}` (cache by id).
  Batch/parallelize; show a skeleton until hydrated.
- **Row:** sender avatar + name, book cover + title, `note` (if any), time.
  Tap → BookDetail. Optional quick action **"Add to Want to read"**.
- **States:** empty ("No recommendations yet"), loading, error.
- Badge the tab with unseen count: track the max `created_at` the user has
  seen locally in `AsyncStorage`; count rows newer than that.

### 6.8 Recommend to a friend (RecommendToFriend)

- **Purpose:** send the current book to a friend.
- **Data in:** `book_id` (must exist — if the origin was a search result,
  resolve first). Friend list from §6.9.
- **UI:** pick one friend, optional `note` (short, ~280 chars). Submit
  `POST /recommendations { from_user_id: me, to_user_id, book_id, note }`.
- **States:** no friends → link to AddFriend. Success toast, pop.
- Note: sending twice creates duplicates — disable submit after first tap.

### 6.9 Friends (tab)

- **Purpose:** see and grow your friend circle.
- **Data:** there is no "list my friends" endpoint. Derive it:
  - Keep a local list of friend user-ids. Seed/refresh it by remembering every
    `Friendship` you create, and by collecting distinct `actor.id`s seen in the
    feed. Hydrate each via `GET /users/{id}`.
  - **This is a known backend gap (§9)** — a `GET /users/{id}/friends`
    endpoint should be added; until then the derived list is best-effort.
- **Row:** avatar, display name, `@handle`. Tap → FriendProfile.
- **Add friend** button → AddFriend.
- **Empty state:** prominent "Add a friend to get started."
- **Superseded:** FriendProfile can no longer show a friend's `read`/`reading`
  shelf — `GET /users/{id}/library` is self-only now (§3), so a friend's
  library is unreachable with no workaround short of a backend change. As
  built, FriendProfile shows name/avatar only.

### 6.10 Add friend (AddFriend)

- **Current (handle-based):** text field for the friend's `@handle`. On submit:
  1. Optional pre-check `GET /users/by-handle/{handle}` for a nice preview
     (name + avatar) and clear "no such handle" error.
  2. `POST /friendships { user_handle_a: currentUser.handle, user_handle_b:
     handle }`. Friendship is immediate and mutual; no pending state.
  3. On success: add to the local friend list, invalidate `feed`, pop.
- **Structure for the future:** `spec/friends.md` replaces this with QR-code /
  invite-link redemption (`POST /invites`, `GET /invites/{token}`,
  `POST /invites/{token}/accept`). Build this screen so the handle field is one
  "method" and a **Scan / paste invite** method can be added beside it without
  restructuring. Do **not** implement the invite endpoints now — they don't
  exist yet.

### 6.11 Me / Edit profile

- **Data:** `currentUser`. Re-fetch `GET /users/{me}` on focus.
- **Show:** avatar, display name, `@handle`, locale, member since.
- **Editable:** — **none via the API yet** (no `PATCH /users`). Show fields as
  read-only with a "Profile editing coming soon" note, except locale which you
  may store locally and use as the default language for manual book entry.
- **Actions:** "Sign out" (§5).

---

## 7. Key user flows (acceptance-level)

1. **Onboard:** open app → create identity → land on an empty Timeline with a
   prompt to add friends.
2. **Add a friend:** Friends → Add friend → enter handle → they appear in the
   list; their past activity shows in the Timeline after next refresh.
3. **Shelve a book to read:** My Books → Want to read → Find a book → search →
   tap result (resolves) → BookDetail → set "Want to read".
4. **Start & progress:** BookDetail or My Books → "Start reading" → later,
   update progress slider → no timeline spam for progress-only changes.
5. **Finish & rate:** "Mark finished" → Finish sheet → choose Finished, 4 stars
   → appears on the Read shelf with the rating; a "finished" event appears in
   friends' timelines.
6. **Abandon & rate:** Finish sheet → "Didn't finish", 2 stars → Read shelf
   shows it with a DNF badge.
7. **Log a past read:** My Books → Read → "+ Add a book I've read" → search or
   manual entry → Finish sheet → lands on Read shelf.
8. **Recommend:** BookDetail → Recommend to a friend → pick friend + note →
   friend sees it in their Recommendations inbox (with a tab badge).
9. **Read the timeline:** Timeline tab → scroll (paginates) → tap a row → that
   book's detail.

---

## 8. Cross-cutting requirements

- **Server state:** every list/detail is a React Query key. Mutations invalidate
  the specific keys named per screen above. Prefer optimistic updates for shelf
  changes and progress; roll back on error.
- **Pagination:** only the feed paginates (`before` cursor). All other list
  endpoints return the full set — fine at friends scale, but virtualize long
  lists (`FlashList`/`FlatList`).
- **Covers/images:** `cover_image_url` is often `null` and Open Library URLs can
  404 → always render a titled placeholder on missing/failed load. Cache images
  (`expo-image`).
- **Errors:** map by the table in §3. Never surface raw 500 text (`"internal
  error"`). 400 `error` strings are safe to show verbatim.
- **Offline:** read-only screens should render last-cached data with an offline
  banner; queue no writes offline for v1 (disable mutating controls when
  `NetInfo` reports offline).
- **Empty/loading/error** states are required for every screen (copy suggested
  above).
- **Time:** show relative times ("2h", "yesterday"); absolute on long-press.
- **Multi-language:** display `languages` / `edition.language` as readable badges
  (map BCP-47 → language name). Don't assume English. App UI copy goes through
  i18n even though only `en` ships.
- **Accessibility:** every interactive element labelled; rating control operable
  without fine gestures; respect dynamic type.
- **Analytics/tracking:** none (non-commercial, privacy-first).

---

## 9. Not available from the backend yet

Design around these — **don't** build screens that require them:

- **List my friends.** No `GET /users/{id}/friends`. Friend list is derived
  client-side (§6.9). Flag for backend follow-up.
- **A friend's shelf/library/feed.** `GET /users/{id}/library`,
  `/book-statuses`, `/feed`, and `/recommendations/inbox` are self-only
  (403 for anyone else's id) — see §6.9. FriendProfile is name/avatar only
  until there's a backend endpoint for this.
- **Edit profile.** No `PATCH /users` — no display name / avatar / locale
  update. Avatars are always `null` today (no upload endpoint).
- **Ratings in the timeline.** `FeedItem` has `status`/`verb` but no `rating`.
  "Alice rated Dune ★★★★" is not expressible yet.
- **Recommendation & friendship events in the feed.** Feed is reading-status
  changes only.
- **Reactions / comments.** Table exists server-side, no routes.
- **Library availability ("find at your library")** and **affiliate / buy
  links.** Not built. Show disabled "coming soon" affordances only.
- **Push notifications.** No backend support; the Recommendations tab badge is
  client-side polling only.
- **Book editions list.** Only the single `edition` from the last `resolve` is
  available per book; no endpoint to enumerate all editions/translations.
- **Delete / unfriend / remove a shelf entry.** No endpoints.

---

## 10. Assumptions & open questions

- **Assumed:** the app targets a single backend instance whose URL is
  build-time config; one device = one identity.
- **Assumed:** `handle` is unique and user-chosen; `@handle` is shown publicly
  to friends. `friends.md` says no handle *search/discovery* — so never build a
  user-search screen; adding a friend always requires knowing their exact
  handle (or, later, scanning their invite).
- **Open:** should the Timeline include the user's own events (backend returns
  them)? Default: **yes**, show them, subtly de-emphasised.
- **Open:** progress updates while `currently_reading` don't create feed events
  by design — confirm the app shouldn't offer a manual "share my progress"
  action (out of scope for v1).
- **Open:** rec inbox has no "dismiss/mark handled" — v1 just shows all,
  newest first, with a locally-tracked seen marker for the badge.

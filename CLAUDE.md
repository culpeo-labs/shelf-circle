# Shelf Circle backend — working notes

## What this repo is

**Monorepo note:** this file lives at `backend/CLAUDE.md`, i.e. the repo root is
one level up. `.github/` (workflows + `dependabot.yml`) lives at the repo root,
**not** under `backend/` — every path below is relative to this `backend/`
directory unless said otherwise. The workflows `cd`/scope into `backend/`
(`working-directory` / explicit path prefixes) and are path-filtered to only run
on `backend/**` changes.

Rust API backend for Shelf Circle, a friends-only book recommendation app.
Stack: **Axum 0.8** (HTTP) + **SQLx 0.8** (Postgres, compile-time-unchecked
`query_as`) + **Postgres**. Auth: **Hanko Cloud** JWTs (bearer, verified against
Hanko's JWKS). Migrations in `migrations/` run automatically on startup via
`sqlx::migrate!`. Deployed on Azure Container Apps + Azure DB for PostgreSQL via
Bicep in `infra/` and GitHub Actions in the repo-root `.github/workflows/`.

### Layout

- `src/main.rs` — entrypoint; loads `.env` (`dotenvy`, real env wins, missing OK),
  builds `AppState`, the router, connects the pool, warms the JWKS cache,
  `axum::serve`.
- `src/state.rs` — `AppState { pool, providers, auth }`. Handlers extract
  `State<PgPool>`, `State<Arc<BookProviders>>`, or `State<Arc<HankoAuth>>`; all
  resolve through `FromRef`, so route modules don't name the whole struct.
  Sub-routers are `Router<AppState>`.
- `src/auth.rs` — Hanko JWT verification + the `AuthClaims` / `CurrentUser`
  extractors + `ensure_self`. See **Auth** below.
- `src/catalogs/` — "get it at your library" plugins. `mod.rs` = the
  `SYSTEMS` registry (`LibrarySystem { id, name, kind }`: Seattle Public
  Library `seattle`, King County `kcls`), `Catalogs` (`find_book` by title/author with ISBN preference, 1h
  in-process cache) and `search_url`; `biblio_commons.rs` = the only `Kind`
  so far. See **Library catalogs**.
- `src/storage.rs` — `AvatarStorage`: Azure Blob avatar uploads. Mints a
  10-minute, write-only (`sp=cw`) SAS URL for `<user-uuid>/<random-uuid>.jpg`,
  HMAC-signed with the storage account key (no Azure SDK); `owns_avatar_url`
  is what `PATCH /me` uses to accept only URLs this API minted for the caller.
  Config: `AZURE_STORAGE_ACCOUNT` + `AZURE_STORAGE_KEY` (+ optional
  `AZURE_STORAGE_CONTAINER`, default `avatars`; `AZURE_STORAGE_BLOB_ENDPOINT`
  for Azurite). Unset → `POST /me/avatar-upload` is 503 and `AppState.storage`
  is `None`. Signing format was verified against Azurite (valid SAS → 201,
  tampered → 403); to re-check locally: `npx azurite-blob`, create an
  `avatars` container, and PUT to a minted URL with `x-ms-blob-type: BlockBlob`.
  Old avatar blobs are not deleted when replaced.
- `src/db.rs` — pool creation + migration runner.
- `src/error.rs` — `ApiError` / `ApiResult`; maps errors to JSON responses
  (`ProviderError` → 404 / 400 / 502).
- `src/models.rs` — all request/response/DB structs.
- `src/providers/` — book data providers. `mod.rs` = `BookProviders` service
  (`search`, `resolve`) + `BookSearchResult` + `normalize_language`;
  `open_library.rs` (always on, no key); `google_books.rs` (only when
  `GOOGLE_BOOKS_API_KEY` set — keyless = HTTP 429). Uses `reqwest`.
- `src/routes/` — one module per resource (`me`, `users`, `friendships`,
  `invites`, `books`, `statuses`, `recommendations`, `feed`, `library`), each
  exposing a `router()`; wired in `routes/mod.rs`. Each file self-contains its
  response structs. `friendships::upsert_friendship` (the canonicalized
  insert-or-noop) is `pub` and shared with `invites::accept_invite` — don't
  reimplement it a third time (it's the only place friendships are created).
  `GET /me/friends` (same file) lists friends from
  either side of the canonicalized row — the app's friend list reads it (an
  invite's creator has no other way to learn who accepted).
- `migrations/` — `0001_init.sql` (v1 schema), `0002_activity_events.sql`
  (timeline log + trigger), `0003_ratings.sql` (`book_statuses.rating`),
  `0004_auth.sql` (`users.hanko_user_id` + `users.email`), `0005_invites.sql`
  (`invite_tokens` — QR/link friend-adding, see friends.md), `0006_...sql`
  (drops a redundant index `0005` accidentally duplicated), `0007_...sql`
  (redefines the `book_statuses` activity trigger to skip inserting an
  `activity_events` row when the request set `SET LOCAL
  shelf_circle.suppress_activity = 'true'` — see **Timeline / feed** below),
  `0008_...sql` (adds `book_statuses.backdated boolean not null default
  false` — persists the same flag on the row itself, for a "logged as
  backlog" badge; see **Timeline / feed**), `0009_share_shelves.sql`
  (`users.share_shelves boolean not null default false`), `0010_library_system.sql`
  (`users.library_system text` — a `catalogs::SYSTEMS` id, validated in code, not a FK),
  `0011_book_description.sql` (`books.description` + `description_checked_at`; see
  **Book descriptions**), `0012_book_completions_and_goals.sql` (`book_completions` +
  `reading_goals`; see **Reading completions & goals**). UUID default is
  `gen_random_uuid()` (built into Postgres 13+, no extension needed) — not
  `uuid_generate_v4()`/`create extension "uuid-ossp"`: Azure DB for
  PostgreSQL Flexible Server doesn't allow-list that extension by default, so
  `CREATE EXTENSION "uuid-ossp"` fails on first boot (`extension "uuid-ossp"
  is not allow-listed for users`), migration 1 never completes, the app never
  binds to 8080, and Container Apps just shows "This Container App is stopped
  or does not exist." with the revision stuck at 0 replicas — the deploy
  workflow's own smoke test is what catches this, 5 minutes late. Check
  `az containerapp logs show -g <rg> -n <app> --type console` (or, once no
  replica is up, Log Analytics: `ContainerAppConsoleLogs_CL` on the app's
  managed environment workspace) before assuming an infra/ingress problem.
- `infra/` — Bicep: `registry.bicep` (ACR, deployed first), `main.bicep`
  (Log Analytics + Container Apps env + Postgres Flexible Server + Storage
  account with a public-blob-read `avatars` container + Container App
  with managed-identity ACR pull; the storage key is injected as a Container
  App secret via `listKeys()`), `main.parameters.json` (non-secret defaults).
  ACR pull identity is **user-assigned**, not system-assigned: a system-assigned
  identity's principalId only exists once the Container App resource is
  created, so the AcrPull role assignment would depend on the app — but the
  app's first image pull depends on that role already existing. That deadlock
  makes the initial revision retry until Container Apps times out
  (`ContainerAppOperationError: Operation expired`), failing the deployment
  before the role assignment is ever attempted (`az role assignment list`
  against the ACR scope comes back empty). Fix: create the identity as its own
  resource, grant it AcrPull first, then attach it to the Container App
  (`identity: apiIdentity.id` in `registries[]`, explicit `dependsOn: [acrPull]`
  on `api`).
- `../.github/workflows/` (repo root) — `ci.yml` (fmt/clippy/build with
  `working-directory: backend` + `az bicep build backend/infra/*.bicep`),
  `deploy.yml` (push to `main`: OIDC → `backend/infra/registry.bicep` →
  `az acr build ... backend` → `backend/infra/main.bicep` → `/health` smoke
  test). Both are path-filtered to `backend/**`. `../.github/dependabot.yml` —
  weekly `cargo` (directory `/backend`) + `github-actions` (directory `/`)
  updates (minor/patch grouped, majors separate).

### Data model

users (with `hanko_user_id` unique + `email`, both from the JWT), mutual
friendships (canonicalized `user_a_id < user_b_id`), canonical `books` with
multi-language `book_editions`, per-user `book_statuses` (want_to_read /
currently_reading / finished / did_not_finish, plus optional `rating` 1-5),
`recommendations` (the "X recommended a book to you" inbox), `activity_events`
(append-only timeline log), `invite_tokens` (QR-code/deep-link friend adding,
single-use or reusable — see **Friends & invites**), and `friend_requests`
(pending approvals for reusable invites). `reactions` table exists
but has no routes yet.

### Friends & invites

- **A friendship only forms with both people involved.** It's mutual and opens
  up your timeline (and shelves, if shared), so there is deliberately **no
  add-by-handle** (`POST /friendships` and `GET /users/by-handle/{handle}` were
  removed) and **no profile lookup by id for strangers**: `GET /users/{id}` is
  your own or a friend's profile, else 404. Being signed in isn't enough to
  learn who someone is.
- **Two invite modes** (`POST /invites[?reusable=true]`, `invite_tokens.max_uses`
  / `use_count` / `requires_approval` / `revoked_at`, migration `0013`):
  - *single-use* (default): 16-hex token, 7 days, the first accept connects you
    immediately — the issuer chose whom to hand it to.
  - *reusable* ("anyone with the link"): 32-hex (full-strength) token, 30 days,
    revocable (`DELETE /invites/{token}`); each accept only creates a
    **`friend_requests` row** (`pending`) that the issuer approves/declines
    (`GET /me/friend-requests`, `POST /friend-requests/{id}/approve|decline`).
    A declined requester who retries still sees `pending` (declines aren't
    revealed). Requests show only display name + photo — no handle, no id.
  - `GET /invites` lists your usable invites (`use_count`, `pending_requests`);
    `GET /invites/{token}` is the public preview: display name, avatar,
    `requires_approval` — never a handle or id.
  - Accepting locks the invite row (`for update`) so the capacity check and the
    `use_count` bump are atomic (concurrent accepts of a single-use token: one
    wins). Accepting your own invite is a 400 that rolls back (doesn't burn the
    token); already being friends returns `friends` without using the invite up.
    Accept/approve return `{status: friends|pending, friendship_id}` — an opaque
    friendship id, **not a user id**.
- **Not done yet (see the user-id follow-up):** other users' `id`/`handle` are
  still returned to *friends* (friends list, feed actors, recommendations).
  The intended end state is that no user id is ever returned to anyone else —
  friends referenced by friendship id — with the handle visible to friends only.

### Auth

- Hanko Cloud issues RS256 JWTs; the client sends `Authorization: Bearer <jwt>`.
  `HankoAuth` (in `AppState`) verifies the signature against Hanko's JWKS
  (`<HANKO_API_URL>/.well-known/jwks.json`), cached in-process with a 1h TTL and a
  60s min-refresh throttle; an unknown `kid` triggers one refetch.
- Config: `HANKO_API_URL` (or `HANKO_JWKS_URL`), optional `HANKO_AUDIENCE`
  (enables the `aud` check; otherwise `validate_aud = false`). `from_env()` bails
  at startup if neither Hanko var nor `AUTH_DISABLED` is set.
- Two extractors: `AuthClaims` (verified token only — onboarding, where no row
  exists yet: `POST /users`, `GET /me`) and `CurrentUser` (token resolved to a
  `users` row by `hanko_user_id`; no row → **403**). Both `FromRequestParts`
  (axum 0.8, no `#[async_trait]`).
- The acting user comes from the token, never the body: `SetBookStatus` /
  `CreateRecommendation` dropped `user_id` / `from_user_id`. `/users/{user_id}/…` routes call `ensure_self(&me, user_id)`
  (feed, library, inbox, book-statuses list) → 403 on mismatch. **Exception:**
  `/users/{id}/library` also allows a *friend* of the owner when
  `users.share_shelves` is true (`library::ensure_can_view_library`; the owner
  toggles it via `PATCH /me { share_shelves }`). Non-friends and unknown ids
  get the same 403. Only the library opens up — feed/inbox/book-statuses stay
  self-only.
- `PATCH /me` edits `display_name` (trimmed, 1–50 chars), `share_shelves`, and
  `avatar_url` (absent = unchanged, `null` = remove, else must satisfy
  `AvatarStorage::owns_avatar_url`). Handle isn't editable. Avatar flow:
  `POST /me/avatar-upload` → app PUTs a resized JPEG to `upload_url` → `PATCH`
  with the returned `avatar_url`. The container is public-blob-read
  (unguessable names, no listing) so avatars load as plain image URLs.
- Unique-violation mapping in `error.rs`: `users_handle_key` /
  `users_hanko_user_id_key` → **409** (handle taken / profile exists).
- **`AUTH_DISABLED=true`** (local dev): skips JWT verification. `CurrentUser`
  loads the row named by an `X-Debug-User-Id: <uuid>` header; `AuthClaims` (and so
  onboarding) is rejected — seed a `users` row directly to exercise the
  authenticated routes.

### Timeline / feed

- `activity_events` is append-only: `(actor_user_id, book_id, status, created_at)`.
- Populated by a **Postgres trigger** (`record_reading_activity` on
  `book_statuses` insert/update) — fires only when `status` actually changes, so
  progress-only updates don't spam. The app write path (`statuses.rs`) does not
  touch it. `0002` backfills from existing `book_statuses` using `updated_at`.
- **Backlog reads (logged from before the user had the app) skip this.**
  `PUT /book-statuses` with `backdated: true` wraps the upsert in a
  transaction and runs `set local shelf_circle.suppress_activity = 'true'`
  first (see `0007`); the trigger checks that setting and no-ops instead of
  inserting. `SET LOCAL` resets at commit/rollback, so it can't leak onto a
  later request that reuses the same pooled connection. The book still lands
  on the right shelf either way — only the feed entry is suppressed.
  `book_statuses.backdated` (`0008`) separately persists the same flag on
  the row for a "logged as backlog" badge — distinct from the transient
  `SET LOCAL` above (which only controls the trigger for one write).
  `set_status`'s upsert clears it automatically the next time the status
  actually changes (a real reread), via a `case` comparing the incoming
  status against the row's *current* one, but leaves it alone when a write
  re-submits the same status (e.g. only the rating changed) — nothing about
  "when did this status take effect" changed in that case.
- `GET /users/{id}/feed` — friends **+ self**, newest first, keyset page via
  `?before=<rfc3339>&limit=`. Filters on `created_at` only (no id tiebreak yet).
- `feed.rs` / `library.rs` fetch a flat `*Row` struct (aliased columns) then map
  to nested response structs; they don't use `#[sqlx(flatten)]`.

### Library catalogs

- User picks a library system (`GET /library-systems`, `GET|PUT
  /me/library-system`); `GET /books/{id}/library-link` then returns
  `{library, found, lookup_failed, url}`: `url` is the catalog **record page**
  when the catalog has the *work*, else a **title+author catalog search**.
  Always 200 with a usable `url` — a slow/down catalog only sets
  `lookup_failed` (400 only if no library is chosen). Named `library_systems`
  in code to avoid confusion with `routes/library.rs` (a user's bookshelves).
- **Match by work (title + author), not by ISBN.** We store one representative
  edition per book (Open Library's first English one, often a UK/odd printing)
  and libraries hold other printings: ISBN-first matched **1 of 18** popular
  titles in Seattle's catalog; title+author matched 18/18. So a lookup is: search
  `main title + author`, accept records that are the same work
  (`catalogs/matching.rs`: normalized title — subtitle-tolerant but two main
  titles never fuzzy-match each other, so "Dune" ≠ "Dune Messiah" — plus author
  surname and compatible language), preferring the exact edition (one of our
  ISBNs), then a record in the book's language (a preference, not a filter),
  then plain book > large print > ebook > other. Ranking, in order: record
  titled like the book **as the app shows it** (`books.canonical_title`) →
  exact edition (our ISBN) → the saved edition's language → format. Title-first
  because the saved edition's language is arbitrary (Open Library's first
  English one) while the displayed title is what the user actually picked; an
  English-first rule linked the English translation of "Cien años de soledad"
  even though Seattle holds the Spanish edition the user was looking at. Only
  if nothing matches are up to 2 ISBNs tried on their own.
- **Translations:** Open Library files every translation under one work, so a
  book can be titled "Cien años de soledad" while its saved edition is the
  English "One Hundred Years of Solitude" (and the catalog lists each under its
  own title). The lookup therefore searches under every distinct title we have
  (`book_editions.title`, max 3 searches, pooled), and a record matches under
  any of them. Only the primary title's search failing is fatal
  (`lookup_failed`); an alternate's failure just means fewer candidates.
  Limits: authorless/ISBN-less junk works (e.g. Open Library's bare "100 años de
  soledad") can't be verified and fall back to the search link.
- **Adding a library** on an existing kind = one `SYSTEMS` entry (id is stored
  on users — never rename). **New kind of catalog** (Libby/OverDrive, Sierra…)
  = a module in `catalogs/`, a `Kind` variant, and an arm in
  `Catalogs::find_book` / `search_url`; each kind only answers "record URL
  for this book" (reusing `matching.rs` for the same-work test).
- BiblioCommons plugin: `GET {gateway}/v2/libraries/{slug}/bibs/search?query=<title author or isbn>
  &searchType=smart` (unauthenticated, **unofficial** — the endpoint the
  libraries' own sites use; verified live for `seattle` and `kcls`), record link
  `https://{host}/v2/record/{bib id}`. Note `slug` is the library's BiblioCommons
  id, not necessarily the obvious one (`spl` is a Canadian library; Seattle is
  `seattle`). Tests point it at a wiremock via
  `Catalogs::with_biblio_commons_gateway`.

### Reading completions & goals

- **`book_completions`** (`0012`): one row per *finish* — `(user_id, book_id,
  completed_at, backdated)`. Written by a trigger on `book_statuses` whenever a
  row moves **to** `finished` (insert or status change), carrying that row's
  `backdated` flag. This is the source of truth for counts; do **not** count
  `book_statuses` (one row per book: rereads overwrite it, `updated_at` moves on
  any write) or `activity_events` (the friends' feed: backdated reads are
  suppressed from it entirely, and its semantics should stay free to change).
- **Counting rules:** `not backdated` only; every finish counts, so a reread is
  a second completion (also same-year). A backlog book read before the app is
  recorded as `backdated = true` and excluded — until it's reread, since leaving
  and re-entering `finished` is a status change whose write isn't backdated.
  Re-submitting the same status (rating edit) adds nothing. **Undo window:**
  moving *away* from `finished` within 1 hour of that finish deletes it (a
  misclick can't inflate a count); later than that, the completion stays.
  Backfilled from `activity_events` finishes (dates approximate for rows that
  came from 0002's own backfill) plus backdated `book_statuses` rows.
- **Time zones:** timestamps are UTC instants; a year is
  `[make_timestamptz(y,1,1,…,tz), make_timestamptz(y+1,1,1,…,tz))`, so the app
  passes the device's IANA zone (`tz`), validated against `pg_timezone_names`.
- **`reading_goals`**: `(user_id, starts_on, ends_on inclusive, time_zone,
  target_count)`, unique per period. Period-based on purpose. API: `GET
  /me/reading-stats?year=&tz=` → `{year, time_zone, completed, by_month[12],
  goal}`; `PUT|DELETE /me/reading-goals/{year}` (calendar-year goals only for now).
- **Where challenges / lists would slot in (not built):** every target is "a
  user's non-backdated completions in a period, in a zone, optionally
  restricted to a set of books". A *friends' challenge* = a shared goal
  definition + a participants table (each participant's progress is that same
  query for their own user); a *"read N from this list"* challenge = a `lists` /
  `list_books` pair and a `book_id in (list)` filter on the same count. Neither
  needs to change `book_completions`; add `challenges`/`challenge_participants`
  (referencing or generalising `reading_goals`) when they're real. Friends
  seeing each other's counts should follow the `share_shelves` opt-in (or its own
  setting) — stats are self-only today.

### Ratings

- `book_statuses.rating smallint`, 1-5, DB CHECK: null unless status is
  `finished` / `did_not_finish`. `set_status` validates and force-clears rating
  on any other status; the upsert always writes `rating = excluded.rating`, so
  omitting it on a re-set clears it.
- `GET /users/{id}/library?shelf=reading|read|want_to_read|did_not_finish|all` —
  the user's own shelves with book details. `shelf` maps to a **static** SQL
  predicate (no user input in query text). Off-platform books: `/books/resolve`
  (manual body, `source:"manual"`) then `PUT /book-statuses` finished + rating.

### Book descriptions

- `books.description` (plain text, shown on the book page only when present).
  Captured at resolve: Open Library work `description` (a string or `{type,
  value}`) / Google Books `volumeInfo.description` (HTML), both run through
  `providers/text.rs` (`\r\n`, markdown emphasis/links, `----------` source
  footers, HTML tags/entities, 4000-char cap). `ResolvedBook.description` is
  optional so manual entries/older clients needn't send it. Re-resolving an
  existing book fills a missing description but never overwrites one.
- **Lazy backfill** for books saved before this existed: `GET /books/{id}` with
  no description and a provider id fetches one (4s timeout, best-effort — a
  failing provider never breaks the page). `description_checked_at` is set when
  a lookup *succeeds* (even with no description found) so a book with none at
  the source isn't re-fetched for 30 days; a failed/timed-out lookup isn't
  recorded and retries next view. Manual books (no provider id) never fetch.

### Book search / resolve

### Book search / resolve

- `GET /books/search?q=&limit=` → work-level `BookSearchResult`s, merged across
  providers, **not persisted**.
- `POST /books/resolve` body is an untagged enum: full `ResolvedBook` (tried
  first) **or** `{source, source_id}` reference, which `BookProviders::resolve`
  fetches + normalizes. Then the existing upsert runs (match on
  `(source, source_id)`, then work id, else insert).
- Open Library work-level resolve picks a representative edition: prefers an
  English one, then any with ISBN-13, then first. `source_id` stored on the
  edition is the **work id** for these (edition-level resolve would use a real
  edition id and merge into the same book via `open_library_work_id`).

### Not yet built

Provider-response cache. Edition-level search granularity. Feed keyset id
tiebreak; feed covers reading-status events only (no recommendation/friendship
events). Live library availability (holdings/wait times — the gateway's bib `availability` has available/total/holds counts, but we only link to the record today; see **Library catalogs**), affiliate links, reactions/comments routes. Auth
hardening (JWT `iss` check, friend-graph checks such as "recommend to friends
only", rate limiting). VNet integration for Postgres (currently the "allow all
Azure services" firewall rule).

## Running

Needs a Postgres reachable at `DATABASE_URL` (default
`postgres://shelfcircle:shelfcircle@localhost:5432/shelfcircle`) **and** an auth
config: either `AUTH_DISABLED=true` (local bypass, see **Auth**) or
`HANKO_API_URL` — startup fails fast without one. `cargo run`, then
`curl localhost:8080/health` → `ok`. Config comes from the environment or a
gitignored `.env` in the working dir; `GOOGLE_BOOKS_API_KEY` there enables the
Google Books provider.

## Conventions

- Dependencies track current majors; no exact `=x.y.z` pins.
- Axum 0.8 path params use `{param}` syntax, not `:param`.
- thiserror 2: a struct field literally named `source` is treated as the error
  source (must impl `StdError`). Name it something else (`provider`) if it's
  just data.
- Keep `README.md` (user-facing) and this file (agent-facing) in sync when
  endpoints or the data model change.

## Persisting knowledge

**This file is the memory for work on this repo.** When you learn something
non-obvious — a gotcha, a design decision, a constraint, a workflow step — add
it here so the next session has it. Remove notes that become false.

## Reasoning style

Think tersely. Caveman talk in reasoning: short words, no filler, drop articles.
"Check schema. Enum snake_case. Bind uuid. Build." Not "Let me now take a look
at the schema to understand how the enum is represented." Final answers to the
user stay normal, clear prose.

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
  reimplement it a third time.
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
  (`users.share_shelves boolean not null default false`). UUID default is
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
  (Log Analytics + Container Apps env + Postgres Flexible Server + Container App
  with managed-identity ACR pull), `main.parameters.json` (non-secret defaults).
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
(append-only timeline log), and `invite_tokens` (single-use, 7-day-lived
tokens backing QR-code/deep-link friend adding — `created_by_user_id`,
`used_at`/`used_by_user_id` nullable until redeemed). `reactions` table exists
but has no routes yet.

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
  `CreateRecommendation` / `CreateFriendship` dropped `user_id` / `from_user_id` /
  the second handle. `/users/{user_id}/…` routes call `ensure_self(&me, user_id)`
  (feed, library, inbox, book-statuses list) → 403 on mismatch. **Exception:**
  `/users/{id}/library` also allows a *friend* of the owner when
  `users.share_shelves` is true (`library::ensure_can_view_library`; the owner
  toggles it via `PATCH /me { share_shelves }`). Non-friends and unknown ids
  get the same 403. Only the library opens up — feed/inbox/book-statuses stay
  self-only.
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

### Ratings

- `book_statuses.rating smallint`, 1-5, DB CHECK: null unless status is
  `finished` / `did_not_finish`. `set_status` validates and force-clears rating
  on any other status; the upsert always writes `rating = excluded.rating`, so
  omitting it on a re-set clears it.
- `GET /users/{id}/library?shelf=reading|read|want_to_read|did_not_finish|all` —
  the user's own shelves with book details. `shelf` maps to a **static** SQL
  predicate (no user input in query text). Off-platform books: `/books/resolve`
  (manual body, `source:"manual"`) then `PUT /book-statuses` finished + rating.

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
events). Library availability, affiliate links, reactions/comments routes. Auth
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

<p align="center">
  <img src="assets/logo.svg" alt="Shelf Circle" width="240" />
</p>

<h1 align="center">Shelf Circle</h1>

<p align="center">
  A small, friends-first social layer around reading — what you're reading, what you've finished,
  and direct recommendations to specific friends. Not a public review feed. Non-commercial.
</p>

---

## Layout

- **`backend/`** — Rust (Axum + SQLx + Postgres) API. See `backend/CLAUDE.md` for architecture,
  auth, and data model notes.
- **`frontend/`** — Expo / React Native app. See `frontend/README.md` for setup, and `frontend.md`
  for the full screen-by-screen spec.
- **`assets/`** — shared brand assets. `logo.svg` is the source of truth for the logo; the app
  icons under `frontend/assets/` and this repo's social preview image are generated from it via
  `frontend/scripts/generate-app-icons.mjs` — don't hand-edit those.
- **`architecture.md`**, **`friends.md`** — product/architecture spec docs.

## Status

Deployed and in active use at friends-scale. Backend runs on Azure Container Apps; the app ships
through EAS. See `frontend/README.md`'s "Builds (EAS)" section and `backend/CLAUDE.md`'s
**Layout** section for the deploy pipelines.

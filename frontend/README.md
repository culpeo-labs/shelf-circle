# Shelf Circle — frontend

Expo (SDK 57) / React Native / TypeScript client for Shelf Circle, a
friends-only reading-tracker and recommendation app. See `../architecture.md`
for product context and `../frontend.md` for the full screen-by-screen spec
(note its status callout — a few sections were superseded once real auth
landed; this README reflects what's actually built).

## Setup

1. `npm install`
2. `cp .env.example .env` and fill in both values:
   - `EXPO_PUBLIC_API_BASE_URL` — the backend to hit. The deployed prod API
     works out of the box (see `.env.example` for the URL) and needs no local
     backend/DB; use `http://localhost:8080` (or your LAN IP, for a physical
     device) to run against a local backend instead.
   - `EXPO_PUBLIC_HANKO_API_URL` — the Hanko Cloud project's API URL. The prod
     value is the `HANKO_API_URL` GitHub Actions variable
     (`gh variable list --env prod`).
3. `npx expo start`, then open in Expo Go (device or simulator) or a dev
   build. **Expo web is not supported** — see Known gaps.

## Auth

Real Hanko Cloud auth (email passcode and password, whatever the connected
project has enabled), driven generically through Hanko's Flow API — the app
renders whatever the current flow state asks for rather than hardcoding step
names, since the exact steps are configured in the Hanko dashboard, not in
this code. See `src/auth/hankoFlowClient.ts` and
`src/screens/auth/AuthFlowScreen.tsx`.

First-time users: the sign-in screen has a "New here? Create an account"
link that switches to Hanko's registration flow. After verifying, a one-time
"Set up your profile" screen creates the Shelf Circle profile
(`POST /users`) tied to that Hanko identity.

## Brand assets

`../assets/logo.svg` (repo root) is the source of truth for the logo — it's
shared across whatever else in the repo eventually needs it, not just this
app. This app's icons (`assets/icon.png`, `splash-icon.png`, `favicon.png`,
the `android-icon-*.png` adaptive-icon layers) **and** the repo root's
`assets/social-preview.png` (GitHub's social-preview image — re-upload it
manually at Settings → General → Social preview after regenerating; there's
no API for that part) are generated from it; don't hand-edit any of them.
After changing the SVG, regenerate with:

```
npm run generate:icons
```

See `scripts/generate-app-icons.mjs` for the sizes/modes it produces and its
`--source` / `--out` / `--background` overrides.

## Builds (EAS)

Project: [@culpeo-labs/shelf-circle](https://expo.dev/accounts/culpeo-labs/projects/shelf-circle).
`eas.json` has three profiles:

- **development** / **preview** — installable Android APK + iOS _simulator_
  build (no Apple Developer account needed). `preview` is built **on demand**:
  run `.github/workflows/frontend-build.yml` from the Actions tab (or
  `gh workflow run frontend-build.yml`), which runs
  `eas build --profile preview --platform all --no-wait` — fire-and-forget;
  check the EAS dashboard for build status, it isn't wired back into the
  GitHub Actions run. It doesn't run on every merge: EAS's Free plan has a small
  monthly build quota, and every merge already spends a build on the Play
  internal-testing release (below).
- **production** — real Play Store AAB, and (once there's an Apple
  Developer account to sign with) an App Store IPA. Android is automated
  into **Play internal testing**: every push to `main` that touches the app
  (non-markdown files under `frontend/`) runs
  `.github/workflows/frontend-internal-testing.yml`, which builds this
  profile and submits it to the `internal` track (`eas.json`
  `submit.internal`, `releaseStatus: completed`). It also has a manual
  `workflow_dispatch`. To go to production, **promote the baked release in
  Play Console** (internal testing → production) rather than rebuilding; the
  manual `frontend-store-submit.yml` is the alternative that builds fresh and
  uploads a _draft_ to the production track. Not wired for iOS yet.

`EXPO_PUBLIC_API_BASE_URL` / `EXPO_PUBLIC_HANKO_API_URL` are set as EAS
environment variables (`eas env:list`), not read from `.env` — EAS Build
runs in a clean cloud checkout that never sees the (gitignored) local
`.env` file. Update them with `eas env:set` if the backend or Hanko
project ever changes, not by editing `.env` and hoping.

CI auth is an `EXPO_TOKEN` repo secret (an EAS access token, from
https://expo.dev/settings/access-tokens) — set once, unrelated to any
individual's `eas login`.

## Known gaps

- **No session refresh.** Hanko JWTs are short-lived; an expired token sends
  the user back through sign-in rather than refreshing silently.
- **Expo web isn't usable against this backend.** The backend has no CORS
  layer, and the Hanko project's allowed-origins list doesn't include a
  local dev origin either — both would need configuring before
  `npx expo start --web` (or the `web` npm script) works. Untested and not a
  supported target right now; use Expo Go or a dev build.
- **Friends see your shelves only if you opt in.** `PATCH /me
{ share_shelves }` (off by default); `/users/{id}/library` is readable by
  the owner, or by a friend when that's on. `/book-statuses`, `/feed` and
  `/recommendations/inbox` remain self-only.
- **Profile editing needs a fresh native build.** Avatar upload adds
  `expo-image-picker` + `expo-image-manipulator` (native modules), so it won't
  work in an already-installed build until you cut a new one with EAS.
- **Shelf-control mutations aren't optimistic.** The spec asked for
  optimistic updates on shelf/progress changes; currently these wait for the
  server round-trip.
- **No offline banner / `NetInfo` handling.** Screens don't currently detect
  or surface being offline.
- **Invite links use a custom URL scheme (`shelfcircle://`), not a universal
  link.** Deliberate for now — a real universal link needs a domain you
  control serving Apple's `apple-app-site-association` and Android's
  `assetlinks.json`, which doesn't exist yet. Consequence: opening an invite
  link only works if the recipient already has the app installed (fine for
  scanning a QR code in person; a shared link has no smart App Store
  fallback for someone who doesn't have the app yet).
- **Opening an invite link while signed out drops the token.**
  `AcceptInvite` only exists in the signed-in navigation stack (see
  `App.tsx`'s `linking` config comment) — there's no pending-invite state
  carried through sign-in/onboarding yet. Scanning a QR code in-app always
  works since that requires already being signed in to reach the scanner.
- **AddFriendScreen mints a fresh invite token every time it's opened**,
  rather than reusing a still-valid unused one. The backend explicitly
  allows several outstanding invites per user (see `INVITE_TTL`'s doc
  comment in `backend/src/routes/invites.rs`), so this isn't incorrect,
  just wasteful — reopening the screen a few times before actually sharing
  leaves that many extra live tokens until they expire in 7 days.

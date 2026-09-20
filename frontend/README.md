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

## Known gaps

- **No session refresh.** Hanko JWTs are short-lived; an expired token sends
  the user back through sign-in rather than refreshing silently.
- **Expo web isn't usable against this backend.** The backend has no CORS
  layer, and the Hanko project's allowed-origins list doesn't include a
  local dev origin either — both would need configuring before
  `npx expo start --web` (or the `web` npm script) works. Untested and not a
  supported target right now; use Expo Go or a dev build.
- **FriendProfile is name/avatar only.** The backend's
  `GET /users/{id}/library` (and `/book-statuses`, `/feed`,
  `/recommendations/inbox`) are self-only — a friend's shelf isn't fetchable
  at all. No workaround short of a backend change.
- **No "list my friends" endpoint.** The friend list is derived client-side
  (`FriendsContext`, persisted to `AsyncStorage`) from friendships you've
  created and actors seen in the feed — best-effort, not authoritative.
- **Shelf-control mutations aren't optimistic.** The spec asked for
  optimistic updates on shelf/progress changes; currently these wait for the
  server round-trip.
- **No offline banner / `NetInfo` handling.** Screens don't currently detect
  or surface being offline.

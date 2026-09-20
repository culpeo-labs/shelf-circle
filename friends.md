# Spec: Invite-based friend adding (QR + link)

## Goal
Replace handle-based `/friendships` lookup with an invite-token flow. No
email, no username search/discovery — you can only become friends with
someone by receiving a token from them directly (QR scan or shared link).
This keeps the friend graph closed and avoids storing any contact info.

## Schema addition

```sql
create table invite_tokens (
    id uuid primary key default uuid_generate_v4(),
    token text not null unique,           -- short random string, e.g. 10-12 url-safe chars
    created_by_user_id uuid not null references users(id) on delete cascade,
    expires_at timestamptz not null,      -- suggest 7 days from creation
    used_at timestamptz,                  -- null until redeemed
    used_by_user_id uuid references users(id) on delete set null,
    created_at timestamptz not null default now()
);
create index invite_tokens_token_idx on invite_tokens(token);
create index invite_tokens_created_by_idx on invite_tokens(created_by_user_id);
```

Notes:
- `token` should be generated server-side with a CSPRNG (not a UUID directly
  — keep it short enough to look reasonable in a URL, e.g. base62 or similar,
  ~10-12 chars gives plenty of entropy for this scale).
- Not single-use is fine to consider, but default to single-use
  (`used_at` set on first redemption, further redemption attempts rejected)
  since a friends-only app doesn't need one QR code to onboard a crowd.
- Tokens expire — no permanent standing invite links floating around old chat
  threads.

## Endpoints

### `POST /invites`
Auth: current user (from session/auth header — see auth TODO below).
Body: none.
Creates a new token for the requesting user, invalidating no previous ones
(a user can have multiple outstanding invites — e.g. one they generate fresh
per friend they're adding).
Returns: `{ token, expires_at, invite_url }` where `invite_url` is a
deep-link-able URL, e.g. `https://shelfcircle.app/invite/{token}` (falls back
to an app-store landing page if the recipient doesn't have the app yet;
if they do, the app should intercept this URL via universal links / app
links and route straight to the accept flow).

### `GET /invites/:token`
Public (no auth) — lets the client show "so-and-so wants to be your friend on
Shelf Circle" before the recipient has even logged in/signed up, by returning
the inviting user's public display name + avatar (not their handle/id).
Returns 404 if token is invalid, expired, or already used.

### `POST /invites/:token/accept`
Auth: current user (the person accepting — must already have an account).
Validates: token exists, not expired, not already used, and the accepting
user isn't the same as the creator.
On success: creates the friendship (reuse the existing canonicalized
friendship insert logic from `friendships.rs`), marks the token used
(`used_at`, `used_by_user_id`), returns the new `Friendship`.

## Client flow (for whichever mobile stack ends up in use)

1. User taps "Add a friend" → client calls `POST /invites` → gets back a
   token + `invite_url`.
2. Client renders that `invite_url` as a QR code (any QR-gen library takes a
   string and returns a scannable image — no backend involvement needed for
   the QR rendering itself, just the URL it encodes).
3. Client also shows a "share link" button (share sheet) for the remote case
   — same `invite_url`, just sent via text/DM/whatever, not through the app.
4. Recipient scans the code (camera → deep link) or taps the shared link →
   opens the app to an accept screen showing the inviter's name/avatar (via
   `GET /invites/:token`) → confirms → app calls
   `POST /invites/:token/accept`.
5. If the recipient doesn't have the app yet, the link should fall back to
   an app store page; once installed, ideally the token carries through
   (standard deferred deep linking — e.g. via Branch.io or a simple
   "paste your invite code" fallback if you want to avoid a third-party SDK
   for v1).

## Out of scope for this pass (note, don't build)
- Username/handle search or any public directory of users — deliberately
  not part of the design.
- Rate limiting on invite creation — worth adding before any real deploy,
  not needed for a handful of friends testing this.
- Auth itself isn't specified here — this assumes whatever auth layer gets
  built (see backend README's "not yet implemented" list) provides a current
  user id to these handlers.
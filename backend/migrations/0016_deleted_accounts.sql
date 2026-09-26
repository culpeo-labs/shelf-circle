-- Short-lived tombstones for deleted accounts.
--
-- Deleting an account removes the user at Hanko, but a session token that was
-- already issued stays cryptographically valid until it expires. Without this,
-- a device that's still signed in would send that token to POST /users
-- (onboarding) and quietly re-create a profile — with the email from the token —
-- for an account that no longer exists. Onboarding refuses a Hanko id listed here.
--
-- Only the opaque Hanko id is kept (no email, no name), and only for a few days,
-- far longer than a session token lives; the maintenance sweep purges older rows.
create table deleted_accounts (
    hanko_user_id text primary key,
    deleted_at timestamptz not null default now()
);

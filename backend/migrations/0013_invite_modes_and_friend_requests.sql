-- Invite modes + approval for reusable invites.
--
--  * single-use (default): first person to accept becomes a friend immediately;
--    the issuer chose to hand out that link/QR, which is their consent.
--  * reusable ("anyone with the link"): can be accepted by several people, so
--    each accept creates a *pending request* the issuer approves or declines
--    (a link that spreads shouldn't let strangers in automatically). Valid for
--    30 days unless revoked.
--
-- `max_uses` null = unlimited. `use_count` counts people who actually became
-- friends through the invite.
alter table invite_tokens
    add column max_uses integer,
    add column use_count integer not null default 0,
    add column requires_approval boolean not null default false,
    add column revoked_at timestamptz;

-- Every invite that exists so far is single-use.
update invite_tokens
set max_uses = 1,
    use_count = case when used_at is not null then 1 else 0 end;

create table friend_requests (
    id uuid primary key default gen_random_uuid(),
    invite_id uuid not null references invite_tokens(id) on delete cascade,
    requester_user_id uuid not null references users(id) on delete cascade,
    -- the invite's owner, denormalized so "my pending requests" is one index scan
    inviter_user_id uuid not null references users(id) on delete cascade,
    status text not null default 'pending' check (status in ('pending', 'approved', 'declined')),
    created_at timestamptz not null default now(),
    decided_at timestamptz,
    unique (invite_id, requester_user_id)
);
create index friend_requests_pending_idx on friend_requests(inviter_user_id) where status = 'pending';

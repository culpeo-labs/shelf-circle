-- Invite-token based friend adding (QR code / shared link) — see friends.md.
-- No email, no handle search/discovery: you can only become friends with
-- someone by receiving a token from them directly.

create table invite_tokens (
    id uuid primary key default gen_random_uuid(),
    token text not null unique,
    created_by_user_id uuid not null references users(id) on delete cascade,
    expires_at timestamptz not null,
    used_at timestamptz,
    used_by_user_id uuid references users(id) on delete set null,
    created_at timestamptz not null default now()
);
create index invite_tokens_token_idx on invite_tokens(token);
create index invite_tokens_created_by_idx on invite_tokens(created_by_user_id);

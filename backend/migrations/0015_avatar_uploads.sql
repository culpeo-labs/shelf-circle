-- Tracks profile-photo uploads so an *unsaved* one can expire.
--
-- `POST /me/avatar-upload` mints an upload URL and records the file here; when
-- the photo is saved to the profile (`PATCH /me { avatar_url }`) the row is
-- marked claimed. A photo that's uploaded but never saved (the user backed out)
-- stays unclaimed, and a periodic sweep deletes the file and the row once it's
-- older than the TTL (see maintenance.rs). Rows for photos that get replaced or
-- removed are deleted along with their file, so the table only ever holds
-- pending uploads plus each user's current photo.
--
-- It also means the backend knows every photo file a user has, which account
-- deletion can use to remove them.
create table avatar_uploads (
    id uuid primary key default gen_random_uuid(),
    user_id uuid not null references users(id) on delete cascade,
    -- '<avatar_key>/<uuid>.jpg', relative to the avatars container
    blob_path text not null unique,
    created_at timestamptz not null default now(),
    claimed_at timestamptz
);
-- The sweep only looks at unclaimed rows, oldest first.
create index avatar_uploads_unclaimed_idx on avatar_uploads(created_at) where claimed_at is null;

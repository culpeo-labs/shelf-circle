-- Profile photos are stored under an unguessable per-user key rather than the
-- user's id. The photo URL is shown to friends, in the public invite preview and
-- in friend requests, so putting `users.id` in the path leaked every user's id
-- to anyone who could see their photo. `avatar_key` is random, unrelated to the
-- id, and used only to build/validate that path (see storage.rs).
--
-- Photos uploaded before this migration still live under the old `<user id>/`
-- path; their URL keeps working (and keeps exposing the id) until the user next
-- changes their photo.
alter table users add column avatar_key uuid not null default gen_random_uuid();
create unique index users_avatar_key_idx on users(avatar_key);

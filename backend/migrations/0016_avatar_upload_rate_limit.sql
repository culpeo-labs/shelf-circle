-- Abuse guard for POST /me/avatar-upload: at most N uploads per user per rolling
-- 24h window (N in routes/me.rs). A plain counter on the user rather than a count
-- of avatar_uploads rows, because those rows are deleted when a photo is replaced
-- or swept, so they can't answer "how many did this user mint today?".
alter table users
    add column avatar_upload_window_start timestamptz,
    add column avatar_upload_count integer not null default 0;

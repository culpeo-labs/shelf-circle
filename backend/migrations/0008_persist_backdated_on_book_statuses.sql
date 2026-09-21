-- Persists whether a book_statuses row's *current* status was set via the
-- backlog (backdated) flow, so the client can show e.g. a "logged as
-- backlog" badge. This is separate from (and doesn't affect) the
-- activity_events suppression added in 0007, which only needs the flag
-- transiently per-write — this one needs to survive on the row so it can be
-- read back later, and cleared the next time the status actually changes
-- (a real reread), which set_status's upsert handles.
alter table book_statuses add column backdated boolean not null default false;

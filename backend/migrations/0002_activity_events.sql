-- Activity events: an append-only log that powers the friend timeline
-- ("what your friends are reading", Twitter-feed style).
--
-- v1 scope: one event per reading-status change. Unlike book_statuses (an
-- upsert with one row per user/book), this keeps the full history, so
-- "Alice started reading Dune" stays in the feed even after she finishes it.

create table activity_events (
    id uuid primary key default gen_random_uuid(),
    actor_user_id uuid not null references users(id) on delete cascade,
    book_id uuid not null references books(id) on delete cascade,
    -- the status the actor moved the book to
    status reading_status not null,
    created_at timestamptz not null default now()
);

-- Feed queries page by time across a set of actors (friends + self).
create index activity_events_created_idx on activity_events(created_at desc, id desc);
create index activity_events_actor_idx on activity_events(actor_user_id, created_at desc);

-- Emit an event whenever a book_status is created or its status value changes.
-- Re-setting the same status (e.g. a progress-only update) does not produce one.
create function record_reading_activity() returns trigger as $$
begin
    if tg_op = 'INSERT' or new.status is distinct from old.status then
        insert into activity_events (actor_user_id, book_id, status)
        values (new.user_id, new.book_id, new.status);
    end if;
    return new;
end;
$$ language plpgsql;

create trigger book_statuses_activity
    after insert or update on book_statuses
    for each row execute function record_reading_activity();

-- Backfill from statuses that already exist so the feed isn't empty on upgrade.
insert into activity_events (actor_user_id, book_id, status, created_at)
select user_id, book_id, status, updated_at
from book_statuses;

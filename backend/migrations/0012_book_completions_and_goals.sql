-- Reading completions: one row per *finish*, so yearly counts (and later
-- challenges) can be answered without touching the feed.
--
-- Why not count activity_events / book_statuses:
--  * book_statuses is one row per (user, book): a reread overwrites it, and
--    updated_at changes on any write (e.g. editing a rating).
--  * activity_events is the friends' timeline: backdated reads are suppressed
--    from it entirely (so they have no record at all), and its semantics
--    should stay free to change (hiding events, pruning) without moving counts.
--
-- A completion is written by a trigger whenever a book_statuses row moves *to*
-- 'finished' (insert or status change), carrying the row's `backdated` flag.
-- Backdated finishes (books read before using the app) are recorded but flagged,
-- and are simply left out of counts (`where not backdated`). A reread is
-- finished -> reading -> finished, which is a status change, so it adds a new
-- completion — and since the reread write isn't backdated, it counts.
create table book_completions (
    id uuid primary key default gen_random_uuid(),
    user_id uuid not null references users(id) on delete cascade,
    book_id uuid not null references books(id) on delete cascade,
    completed_at timestamptz not null default now(),
    backdated boolean not null default false
);
create index book_completions_user_time_idx on book_completions(user_id, completed_at desc);

create function record_book_completion() returns trigger as $$
begin
    if new.status = 'finished'
       and (tg_op = 'INSERT' or old.status is distinct from new.status) then
        insert into book_completions (user_id, book_id, backdated)
        values (new.user_id, new.book_id, new.backdated);

    -- Undoing an accidental "finished" shortly afterwards takes that completion
    -- back, so a misclick can't inflate a count. Anything later than an hour is
    -- treated as a real finish (e.g. about to reread), and stays.
    elsif tg_op = 'UPDATE' and old.status = 'finished' and new.status <> 'finished' then
        delete from book_completions
        where id = (
            select id from book_completions
            where user_id = new.user_id and book_id = new.book_id
            order by completed_at desc
            limit 1
        )
        and completed_at > now() - interval '1 hour';
    end if;
    return new;
end;
$$ language plpgsql;

create trigger book_statuses_completion
    after insert or update on book_statuses
    for each row execute function record_book_completion();

-- Backfill. Live finishes: every 'finished' event in the feed log (backdated
-- ones were never logged there). Dates are as accurate as that log — events
-- created by migration 0002's own backfill carry updated_at, so they're
-- approximate.
insert into book_completions (user_id, book_id, completed_at, backdated)
select actor_user_id, book_id, created_at, false
from activity_events
where status = 'finished';

-- Backlog finishes that have no event: keep them as backdated completions so
-- they're on record (and excluded from counts).
insert into book_completions (user_id, book_id, completed_at, backdated)
select bs.user_id, bs.book_id, bs.updated_at, true
from book_statuses bs
where bs.status = 'finished'
  and bs.backdated
  and not exists (
      select 1 from book_completions c
      where c.user_id = bs.user_id and c.book_id = bs.book_id
  );

-- Reading goals: "finish N books between two dates". Period-based rather than a
-- bare `year` column so the same shape can later back other targets (a
-- friends' challenge is a goal with more than one participant; "N from a list"
-- adds a list to the count). The count query is always: this user's
-- non-backdated completions in [starts_on, ends_on] in `time_zone`, optionally
-- restricted to a set of books. Only personal yearly goals exist for now.
create table reading_goals (
    id uuid primary key default gen_random_uuid(),
    user_id uuid not null references users(id) on delete cascade,
    starts_on date not null,
    ends_on date not null,        -- inclusive
    time_zone text not null,      -- IANA name the period boundaries are read in
    target_count integer not null check (target_count between 1 and 10000),
    created_at timestamptz not null default now(),
    updated_at timestamptz not null default now(),
    check (ends_on >= starts_on),
    unique (user_id, starts_on, ends_on)
);

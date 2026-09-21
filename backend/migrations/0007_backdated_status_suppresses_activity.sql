-- Lets a single book_statuses write suppress the activity_events row it
-- would otherwise generate — for logging a backlog read from before the
-- user had the app, which shouldn't show up in friends' timelines as
-- something they "just did". Scoped via SET LOCAL (transaction-only), so it
-- can never leak into a later request that reuses the same pooled
-- connection: the setting resets automatically at commit/rollback.
create or replace function record_reading_activity() returns trigger as $$
begin
    if coalesce(current_setting('shelf_circle.suppress_activity', true), '') = 'true' then
        return new;
    end if;

    if tg_op = 'INSERT' or new.status is distinct from old.status then
        insert into activity_events (actor_user_id, book_id, status)
        values (new.user_id, new.book_id, new.status);
    end if;
    return new;
end;
$$ language plpgsql;

-- Opt-in: when true, the user's friends (and only friends) can read their
-- library via GET /users/{id}/library. Private by default — existing users
-- stay private until they flip it in profile settings.
alter table users add column share_shelves boolean not null default false;

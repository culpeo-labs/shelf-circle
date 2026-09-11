-- Auth: map a user to their external Hanko identity.
--
-- Nullable so any pre-auth rows survive the migration; the profile-create path
-- (`POST /users`) always populates hanko_user_id. `email` is a convenience copy
-- of the Hanko email claim (Hanko remains the source of truth).

alter table users
    add column hanko_user_id text unique,
    add column email text;

create index users_hanko_user_id_idx on users(hanko_user_id);

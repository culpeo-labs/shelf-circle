-- Leecommend initial schema
-- Matches the data model in /areas/reading-app architecture doc.

create extension if not exists "uuid-ossp";

create table users (
    id uuid primary key default uuid_generate_v4(),
    handle text not null unique,
    display_name text not null,
    avatar_url text,
    locale text not null default 'en',
    created_at timestamptz not null default now()
);

-- Mutual friendship: a single row per pair, canonicalized so user_a_id < user_b_id.
-- Keeps the "friends only" model simple (no one-way follows for v1).
create table friendships (
    id uuid primary key default uuid_generate_v4(),
    user_a_id uuid not null references users(id) on delete cascade,
    user_b_id uuid not null references users(id) on delete cascade,
    created_at timestamptz not null default now(),
    constraint friendships_ordered check (user_a_id < user_b_id),
    unique (user_a_id, user_b_id)
);

-- Canonical "work" — one row per logical book, independent of edition/language/translation.
create table books (
    id uuid primary key default uuid_generate_v4(),
    canonical_title text not null,
    primary_author text,
    -- external source ids for the resolution layer, so we don't re-resolve on every lookup
    open_library_work_id text,
    google_books_volume_id text,
    cover_image_url text,
    created_at timestamptz not null default now()
);
create index books_open_library_idx on books(open_library_work_id);
create index books_google_books_idx on books(google_books_volume_id);

-- A specific edition/translation of a book (language matters for multi-language support).
create table book_editions (
    id uuid primary key default uuid_generate_v4(),
    book_id uuid not null references books(id) on delete cascade,
    language text not null, -- BCP-47 tag, e.g. 'en', 'es', 'pt-BR'
    isbn_13 text,
    isbn_10 text,
    title text not null, -- title as printed in this edition/language
    publisher text,
    cover_image_url text,
    source text not null, -- 'open_library' | 'google_books'
    source_id text not null,
    created_at timestamptz not null default now(),
    unique (source, source_id)
);
create index book_editions_book_idx on book_editions(book_id);
create index book_editions_isbn13_idx on book_editions(isbn_13);

create type reading_status as enum ('want_to_read', 'currently_reading', 'finished', 'did_not_finish');

create table book_statuses (
    id uuid primary key default uuid_generate_v4(),
    user_id uuid not null references users(id) on delete cascade,
    book_id uuid not null references books(id) on delete cascade,
    status reading_status not null,
    progress_percent smallint check (progress_percent between 0 and 100),
    updated_at timestamptz not null default now(),
    created_at timestamptz not null default now(),
    unique (user_id, book_id)
);
create index book_statuses_user_idx on book_statuses(user_id);

create table recommendations (
    id uuid primary key default uuid_generate_v4(),
    from_user_id uuid not null references users(id) on delete cascade,
    to_user_id uuid not null references users(id) on delete cascade,
    book_id uuid not null references books(id) on delete cascade,
    note text,
    created_at timestamptz not null default now()
);
create index recommendations_to_user_idx on recommendations(to_user_id);
create index recommendations_from_user_idx on recommendations(from_user_id);

-- Lightweight reactions on a recommendation or status update — intentionally minimal,
-- this is not a review platform.
create table reactions (
    id uuid primary key default uuid_generate_v4(),
    user_id uuid not null references users(id) on delete cascade,
    recommendation_id uuid references recommendations(id) on delete cascade,
    book_status_id uuid references book_statuses(id) on delete cascade,
    emoji text not null,
    created_at timestamptz not null default now(),
    constraint reactions_target_check check (
        (recommendation_id is not null)::int + (book_status_id is not null)::int = 1
    )
);

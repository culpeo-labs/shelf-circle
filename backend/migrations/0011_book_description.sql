-- Blurb for the book page, from Open Library / Google Books (plain text; null
-- when no source has one or the book was entered manually).
-- description_checked_at records when we last *looked* for one, so a book whose
-- sources have nothing isn't re-fetched on every view (see routes/books.rs).
alter table books add column description text;
alter table books add column description_checked_at timestamptz;

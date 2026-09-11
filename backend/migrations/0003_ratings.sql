-- Ratings: when a user finishes a book (or stops reading one they didn't like,
-- i.e. did_not_finish) they can attach a 1-5 rating. Kept on book_statuses
-- since it's one-per-user-per-book, not a separate review record — this is
-- deliberately not a review site.

alter table book_statuses
    add column rating smallint;

alter table book_statuses
    add constraint book_statuses_rating_range
        check (rating is null or rating between 1 and 5);

-- A rating only makes sense once reading has ended.
alter table book_statuses
    add constraint book_statuses_rating_requires_finish
        check (rating is null or status in ('finished', 'did_not_finish'));

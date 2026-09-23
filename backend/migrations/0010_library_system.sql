-- Which library system (a `catalogs::SYSTEMS` id, e.g. 'seattle', 'kcls') the
-- user borrows from, for "get it at your library" links. Null = not chosen.
-- Deliberately not a foreign key / enum: the registry lives in code so adding a
-- library never needs a migration, and the API validates ids against it.
alter table users add column library_system text;

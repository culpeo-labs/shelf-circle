-- `invite_tokens.token` already has an implicit unique btree index from its
-- `unique` column constraint (see 0005_invites.sql); the explicit index
-- created alongside it was a redundant duplicate maintained on every write
-- for no query benefit.
drop index if exists invite_tokens_token_idx;

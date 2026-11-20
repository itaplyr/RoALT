-- The account's own performance profile (`performance::Profile`, its per-client settings), as
-- JSON; its clients get these instead of the ones in Settings. NULL: none of its own.
ALTER TABLE accounts ADD COLUMN profile TEXT;

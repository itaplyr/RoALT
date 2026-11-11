-- The user's own (main) account among the alts: at most one row is 1 (update_account keeps it so).
ALTER TABLE accounts ADD COLUMN is_main INTEGER NOT NULL DEFAULT 0;

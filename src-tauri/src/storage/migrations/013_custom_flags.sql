-- This account's own FFlags as a JSON object of strings (used while the custom flags scope is
-- per account); NULL: none.
ALTER TABLE accounts ADD COLUMN custom_flags TEXT;

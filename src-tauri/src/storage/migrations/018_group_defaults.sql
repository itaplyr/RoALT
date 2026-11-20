-- What a group gives its accounts where they set nothing themselves (`GroupDefaults`, JSON); NULL: nothing.
ALTER TABLE groups ADD COLUMN defaults TEXT;
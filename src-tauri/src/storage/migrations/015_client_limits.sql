-- What was last set on one of this account's clients (limits, anti-AFK), as JSON; its next client
-- gets it again. NULL: nothing of its own.
ALTER TABLE accounts ADD COLUMN client_limits TEXT;

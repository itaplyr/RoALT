-- Time each account spent in each place, from the clients RoALT follows (in a game, not
-- teleporting); grouped by game in the UI.
CREATE TABLE playtime (
  account_id INTEGER NOT NULL REFERENCES accounts(id) ON DELETE CASCADE,
  place_id INTEGER NOT NULL,
  ms INTEGER NOT NULL DEFAULT 0,
  last_played INTEGER NOT NULL,
  PRIMARY KEY (account_id, place_id)
);

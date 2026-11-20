-- What the autopsy made of a client's end (`autopsy::Verdict::code`; none for the ends RoALT made
-- itself), and each account's time in games per local day (YYYYMMDD; the last 90 days are kept).
ALTER TABLE crashes ADD COLUMN verdict TEXT;
CREATE TABLE playtime_days (
  account_id INTEGER NOT NULL REFERENCES accounts(id) ON DELETE CASCADE,
  day INTEGER NOT NULL,
  ms INTEGER NOT NULL DEFAULT 0,
  PRIMARY KEY (account_id, day)
);
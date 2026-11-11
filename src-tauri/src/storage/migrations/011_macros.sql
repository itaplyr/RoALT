-- The user's AutoHotkey v2 macros (`macros`). `trusted`: typed in RoALT or its source reviewed;
-- imported and restored ones start at 0. `hotkey` '' for none.
CREATE TABLE macros (
  id            INTEGER PRIMARY KEY,
  name          TEXT    NOT NULL UNIQUE COLLATE NOCASE,
  body          TEXT    NOT NULL,
  trusted       INTEGER NOT NULL DEFAULT 0,
  hotkey        TEXT    NOT NULL DEFAULT '',
  run_on_launch INTEGER NOT NULL DEFAULT 0,
  created_at    INTEGER NOT NULL,
  updated_at    INTEGER NOT NULL
);

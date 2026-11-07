-- Do not backfill scope from a mutable Task: historic execution scope is unknown.
ALTER TABLE attempts ADD COLUMN scope JSON CHECK(scope IS NULL OR json_valid(scope));
PRAGMA user_version=9;

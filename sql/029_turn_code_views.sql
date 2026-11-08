ALTER TABLE turns ADD COLUMN code_view JSON CHECK(code_view IS NULL OR json_valid(code_view));
PRAGMA user_version=29;

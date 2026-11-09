ALTER TABLE projects ADD COLUMN runtime_preferences JSON CHECK(runtime_preferences IS NULL OR json_valid(runtime_preferences));
PRAGMA user_version=18;

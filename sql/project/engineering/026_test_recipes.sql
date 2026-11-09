ALTER TABLE documents ADD COLUMN test_recipe JSON CHECK(test_recipe IS NULL OR json_valid(test_recipe));
PRAGMA user_version=26;

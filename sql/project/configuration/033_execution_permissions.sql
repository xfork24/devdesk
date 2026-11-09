ALTER TABLE executions ADD COLUMN permissions JSON CHECK(permissions IS NULL OR json_valid(permissions));
PRAGMA user_version=33;

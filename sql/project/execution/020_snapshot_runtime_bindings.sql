ALTER TABLE snapshots ADD COLUMN runtime_bindings JSON NOT NULL DEFAULT '[]' CHECK(json_valid(runtime_bindings));
PRAGMA user_version=20;

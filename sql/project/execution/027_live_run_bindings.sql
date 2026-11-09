ALTER TABLE attempts ADD COLUMN live_sessions JSON NOT NULL DEFAULT '{}' CHECK(json_valid(live_sessions));
ALTER TABLE executions ADD COLUMN kind TEXT NOT NULL DEFAULT 'harness' CHECK(kind IN ('harness','tests'));
ALTER TABLE snapshots ADD COLUMN execution_instances JSON NOT NULL DEFAULT '[]' CHECK(json_valid(execution_instances));
PRAGMA user_version=27;

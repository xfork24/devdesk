ALTER TABLE proposals ADD COLUMN task_instances JSON NOT NULL DEFAULT '{}' CHECK(json_valid(task_instances));
PRAGMA user_version=23;

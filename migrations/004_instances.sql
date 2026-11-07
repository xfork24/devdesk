CREATE TABLE harnesses (id TEXT PRIMARY KEY, provider TEXT NOT NULL, version TEXT NOT NULL, capabilities JSON NOT NULL CHECK(json_valid(capabilities)), origin TEXT NOT NULL);
CREATE TABLE sandboxes (id TEXT PRIMARY KEY, provider TEXT NOT NULL, version TEXT NOT NULL, platform TEXT NOT NULL, image TEXT, network BOOLEAN NOT NULL, origin TEXT NOT NULL);
ALTER TABLE projects ADD COLUMN workspace_definition JSON CHECK(workspace_definition IS NULL OR json_valid(workspace_definition));
ALTER TABLE sessions ADD COLUMN instance JSON CHECK(instance IS NULL OR json_valid(instance));
ALTER TABLE reviews ADD COLUMN assignment JSON CHECK(assignment IS NULL OR json_valid(assignment));
ALTER TABLE merges ADD COLUMN change_request JSON CHECK(change_request IS NULL OR json_valid(change_request));
PRAGMA user_version=4;

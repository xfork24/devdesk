CREATE TABLE permissions (
 id TEXT PRIMARY KEY, identity TEXT NOT NULL, name TEXT NOT NULL,
 version INTEGER NOT NULL, supersedes TEXT REFERENCES permissions(id) DEFERRABLE INITIALLY DEFERRED,
 schema_version INTEGER NOT NULL, policy JSON NOT NULL CHECK(json_valid(policy)),
 UNIQUE(identity,version)
);
ALTER TABLE harnesses ADD COLUMN permission JSON CHECK(permission IS NULL OR json_valid(permission));
ALTER TABLE sandboxes ADD COLUMN permission JSON CHECK(permission IS NULL OR json_valid(permission));
ALTER TABLE projects ADD COLUMN permission_overrides JSON CHECK(permission_overrides IS NULL OR json_valid(permission_overrides));
PRAGMA user_version=32;

-- Application configuration and the project directory; no engineering records.
CREATE TABLE settings (key TEXT PRIMARY KEY, value TEXT NOT NULL CHECK(json_valid(value)));
CREATE TABLE project_catalog (
    id TEXT PRIMARY KEY,
    command_id TEXT NOT NULL UNIQUE,
    fingerprint TEXT NOT NULL,
    project JSON CHECK(project IS NULL OR json_valid(project))
);
CREATE TABLE definitions (
    id TEXT PRIMARY KEY,
    family TEXT NOT NULL,
    name TEXT NOT NULL,
    version INTEGER NOT NULL,
    supersedes TEXT REFERENCES definitions(id) DEFERRABLE INITIALLY DEFERRED,
    body TEXT NOT NULL,
    slots JSON NOT NULL CHECK(json_valid(slots)),
    rules JSON NOT NULL CHECK(json_valid(rules))
);
CREATE TABLE harnesses (id TEXT PRIMARY KEY, provider TEXT NOT NULL, version TEXT NOT NULL, capabilities JSON NOT NULL CHECK(json_valid(capabilities)), origin TEXT NOT NULL);
CREATE TABLE sandboxes (id TEXT PRIMARY KEY, provider TEXT NOT NULL, version TEXT NOT NULL, platform TEXT NOT NULL, image TEXT, network BOOLEAN NOT NULL, origin TEXT NOT NULL);
CREATE TABLE events (id INTEGER PRIMARY KEY, target TEXT NOT NULL, kind TEXT NOT NULL, detail TEXT NOT NULL, time INTEGER NOT NULL);
CREATE TABLE operations (id TEXT PRIMARY KEY, fingerprint TEXT NOT NULL, result JSON NOT NULL CHECK(json_valid(result)));
PRAGMA user_version=1;

-- Shared artifacts are referenced, never copied into role or project records.
CREATE TABLE runtime_installations (
    id TEXT PRIMARY KEY,
    provider TEXT NOT NULL CHECK(provider IN ('codex','dsh')),
    version TEXT NOT NULL,
    os TEXT NOT NULL,
    arch TEXT NOT NULL,
    artifact JSON NOT NULL CHECK(json_valid(artifact)),
    source TEXT NOT NULL,
    license TEXT NOT NULL,
    UNIQUE(provider,version,os,arch)
);
INSERT OR IGNORE INTO settings(key,value) VALUES('default_harness','"codex"');
PRAGMA user_version=2;

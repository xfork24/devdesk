-- v1 sessions identify agent/harness/sandbox but do not prove the reviewer's full skills/standard lock.
-- Historical reconstruction is readable, never silently reusable as a verified retry lock.
ALTER TABLE runs ADD COLUMN binding_verified BOOLEAN NOT NULL DEFAULT 0;
PRAGMA user_version=6;

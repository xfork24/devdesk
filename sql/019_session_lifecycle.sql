ALTER TABLE sessions ADD COLUMN lifecycle TEXT;
-- No runtime liveness is inferred during a schema upgrade.
UPDATE sessions SET lifecycle='needs_reconciliation' WHERE scope IS NOT NULL;
PRAGMA user_version=19;

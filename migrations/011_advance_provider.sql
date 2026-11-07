ALTER TABLE advances ADD COLUMN provider TEXT NOT NULL DEFAULT 'legacy:unknown';
PRAGMA user_version=11;

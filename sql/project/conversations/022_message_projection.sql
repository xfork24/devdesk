ALTER TABLE conversation_events ADD COLUMN mode TEXT NOT NULL DEFAULT 'full';
ALTER TABLE conversation_events ADD COLUMN provider_item TEXT;
PRAGMA user_version=22;

ALTER TABLE executions ADD COLUMN protocol_artifact TEXT REFERENCES artifacts(id) DEFERRABLE INITIALLY DEFERRED;
PRAGMA user_version=21;

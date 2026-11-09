-- New typed fields; legacy trace remains explicitly unpopulated, never fabricated.
ALTER TABLE tasks ADD COLUMN scope JSON NOT NULL DEFAULT '{"requirement":null,"design":null,"acceptance":[]}' CHECK(json_valid(scope));
ALTER TABLE documents ADD COLUMN acceptance JSON NOT NULL DEFAULT '[]' CHECK(json_valid(acceptance));
ALTER TABLE documents ADD COLUMN verifies JSON NOT NULL DEFAULT '[]' CHECK(json_valid(verifies));
ALTER TABLE documents ADD COLUMN artifact_id TEXT REFERENCES artifacts(id) DEFERRABLE INITIALLY DEFERRED;
ALTER TABLE snapshots ADD COLUMN capture JSON NOT NULL DEFAULT '{"attempt":null,"runs":[],"sessions":[],"resources":[],"captured_at":null,"reason":"legacy: provenance unavailable"}' CHECK(json_valid(capture));
PRAGMA user_version=3;

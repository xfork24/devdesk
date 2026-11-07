CREATE TABLE effects (
 id TEXT PRIMARY KEY,
 run_id TEXT NOT NULL REFERENCES runs(id) DEFERRABLE INITIALLY DEFERRED,
 kind TEXT NOT NULL CHECK(kind IN ('harness','cancel','test','cleanup')),
 status TEXT NOT NULL CHECK(status IN ('pending','observed','applied','discarded','needs_reconciliation','failed')),
 request JSON NOT NULL CHECK(json_valid(request)),
 receipt JSON CHECK(receipt IS NULL OR json_valid(receipt)),
 revision INTEGER NOT NULL,
 created INTEGER NOT NULL,
 updated INTEGER NOT NULL,
 reason TEXT NOT NULL
);
CREATE INDEX idx_effects_run ON effects(run_id);
CREATE INDEX idx_effects_status ON effects(status);
PRAGMA user_version=7;

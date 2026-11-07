CREATE TABLE advances (
 id TEXT PRIMARY KEY REFERENCES merges(id) DEFERRABLE INITIALLY DEFERRED,
 target TEXT NOT NULL REFERENCES heads(id) DEFERRABLE INITIALLY DEFERRED,
 expected_target TEXT NOT NULL,
 candidate TEXT NOT NULL,
 delivery TEXT NOT NULL REFERENCES deliveries(id) DEFERRABLE INITIALLY DEFERRED,
 status TEXT NOT NULL CHECK(status IN ('prepared','dispatched','observed','applied','needs_reconciliation')),
 receipt TEXT,
 revision INTEGER NOT NULL,
 reason TEXT NOT NULL
);
CREATE INDEX idx_advances_target ON advances(target);
PRAGMA user_version=10;

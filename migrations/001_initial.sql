-- v1: typed scalar columns; JSON only for typed arrays, locks and adapter results.
PRAGMA foreign_keys=ON;
CREATE TABLE projects (
 "id" TEXT PRIMARY KEY,
 "title" TEXT NOT NULL,
 "workspace" TEXT NOT NULL,
 "execution_mode" TEXT NOT NULL,
 "binding" JSON NOT NULL CHECK ("binding" IS NULL OR json_valid("binding")),
 "concurrency" INTEGER NOT NULL,
 "revision" INTEGER NOT NULL
);
CREATE TABLE features (
 "id" TEXT PRIMARY KEY,
 "project_id" TEXT NOT NULL REFERENCES projects(id) DEFERRABLE INITIALLY DEFERRED,
 "title" TEXT NOT NULL,
 "status" TEXT NOT NULL,
 "revision" INTEGER NOT NULL,
 "input_version" INTEGER NOT NULL,
 "binding" JSON NOT NULL CHECK ("binding" IS NULL OR json_valid("binding")),
 "predecessor" TEXT REFERENCES features(id) DEFERRABLE INITIALLY DEFERRED
);
CREATE INDEX idx_features_project_id ON features("project_id");
CREATE INDEX idx_features_status ON features("status");
CREATE TABLE tasks (
 "id" TEXT PRIMARY KEY,
 "feature_id" TEXT NOT NULL REFERENCES features(id) DEFERRABLE INITIALLY DEFERRED,
 "title" TEXT NOT NULL,
 "status" TEXT NOT NULL,
 "revision" INTEGER NOT NULL,
 "binding" JSON NOT NULL CHECK ("binding" IS NULL OR json_valid("binding")),
 "inherited_snapshot" TEXT NOT NULL REFERENCES snapshots(id) DEFERRABLE INITIALLY DEFERRED,
 "dependencies" JSON NOT NULL CHECK ("dependencies" IS NULL OR json_valid("dependencies")),
 "worktree" TEXT NOT NULL
);
CREATE INDEX idx_tasks_feature_id ON tasks("feature_id");
CREATE INDEX idx_tasks_status ON tasks("status");
CREATE TABLE documents (
 "id" TEXT PRIMARY KEY,
 "feature_id" TEXT NOT NULL REFERENCES features(id) DEFERRABLE INITIALLY DEFERRED,
 "task_id" TEXT REFERENCES tasks(id) DEFERRABLE INITIALLY DEFERRED,
 "kind" TEXT NOT NULL,
 "title" TEXT NOT NULL,
 "body" TEXT NOT NULL,
 "version" INTEGER NOT NULL,
 "supersedes" TEXT REFERENCES documents(id) DEFERRABLE INITIALLY DEFERRED,
 "requirement_id" TEXT REFERENCES documents(id) DEFERRABLE INITIALLY DEFERRED,
 "design_id" TEXT REFERENCES documents(id) DEFERRABLE INITIALLY DEFERRED,
 "origin" TEXT NOT NULL
);
CREATE INDEX idx_documents_feature_id ON documents("feature_id");
CREATE INDEX idx_documents_task_id ON documents("task_id");
CREATE INDEX idx_documents_kind ON documents("kind");
CREATE INDEX idx_documents_requirement_id ON documents("requirement_id");
CREATE INDEX idx_documents_design_id ON documents("design_id");
CREATE TABLE definitions (
 "id" TEXT PRIMARY KEY,
 "family" TEXT NOT NULL,
 "name" TEXT NOT NULL,
 "version" INTEGER NOT NULL,
 "supersedes" TEXT REFERENCES definitions(id) DEFERRABLE INITIALLY DEFERRED,
 "body" TEXT NOT NULL,
 "slots" JSON NOT NULL CHECK ("slots" IS NULL OR json_valid("slots")),
 "rules" JSON NOT NULL CHECK (json_valid("rules"))
);
CREATE TABLE attempts (
 "id" TEXT PRIMARY KEY,
 "feature_id" TEXT NOT NULL REFERENCES features(id) DEFERRABLE INITIALLY DEFERRED,
 "task_id" TEXT REFERENCES tasks(id) DEFERRABLE INITIALLY DEFERRED,
 "phase" TEXT NOT NULL,
 "status" TEXT NOT NULL,
 "input_version" INTEGER NOT NULL,
 "documents" JSON NOT NULL CHECK ("documents" IS NULL OR json_valid("documents")),
 "binding" JSON NOT NULL CHECK ("binding" IS NULL OR json_valid("binding")),
 "context_content" TEXT NOT NULL,
 "context_revision" INTEGER NOT NULL,
 "paused" BOOLEAN NOT NULL,
 "revision" INTEGER NOT NULL
);
CREATE INDEX idx_attempts_feature_id ON attempts("feature_id");
CREATE INDEX idx_attempts_task_id ON attempts("task_id");
CREATE INDEX idx_attempts_status ON attempts("status");
CREATE TABLE runs (
 "id" TEXT PRIMARY KEY,
 "attempt_id" TEXT REFERENCES attempts(id) DEFERRABLE INITIALLY DEFERRED,
 "merge_id" TEXT REFERENCES merges(id) DEFERRABLE INITIALLY DEFERRED,
 "node" TEXT NOT NULL,
 "dependencies" JSON NOT NULL CHECK ("dependencies" IS NULL OR json_valid("dependencies")),
 "status" TEXT NOT NULL,
 "retry_of" TEXT REFERENCES runs(id) DEFERRABLE INITIALLY DEFERRED,
 "session_id" TEXT NOT NULL REFERENCES sessions(id) DEFERRABLE INITIALLY DEFERRED,
 "writes" BOOLEAN NOT NULL,
 "worktree" TEXT NOT NULL,
 "outcome" TEXT NOT NULL,
 "cancel_requested" BOOLEAN NOT NULL,
 "started" INTEGER,
 "finished" INTEGER,
 "result" JSON CHECK ("result" IS NULL OR json_valid("result")),
 "origin" TEXT NOT NULL,
 CHECK ((attempt_id IS NOT NULL) != (merge_id IS NOT NULL))
);
CREATE INDEX idx_runs_attempt_id ON runs("attempt_id");
CREATE INDEX idx_runs_merge_id ON runs("merge_id");
CREATE INDEX idx_runs_status ON runs("status");
CREATE INDEX idx_runs_session_id ON runs("session_id");
CREATE TABLE sessions (
 "id" TEXT PRIMARY KEY,
 "owner_id" TEXT NOT NULL,
 "agent" TEXT NOT NULL REFERENCES definitions(id) DEFERRABLE INITIALLY DEFERRED,
 "harness" TEXT NOT NULL,
 "sandbox" TEXT NOT NULL,
 "snapshot_id" TEXT REFERENCES snapshots(id) DEFERRABLE INITIALLY DEFERRED,
 "origin" TEXT NOT NULL
);
CREATE INDEX idx_sessions_owner_id ON sessions("owner_id");
CREATE INDEX idx_sessions_snapshot_id ON sessions("snapshot_id");
CREATE TABLE contexts (
 "id" TEXT PRIMARY KEY,
 "owner_id" TEXT NOT NULL,
 "content" TEXT NOT NULL,
 "revision" INTEGER NOT NULL
);
CREATE INDEX idx_contexts_owner_id ON contexts("owner_id");
CREATE TABLE snapshots (
 "id" TEXT PRIMARY KEY,
 "feature_id" TEXT NOT NULL REFERENCES features(id) DEFERRABLE INITIALLY DEFERRED,
 "task_id" TEXT REFERENCES tasks(id) DEFERRABLE INITIALLY DEFERRED,
 "kind" TEXT NOT NULL,
 "content" TEXT NOT NULL,
 "binding" JSON NOT NULL CHECK ("binding" IS NULL OR json_valid("binding")),
 "documents" JSON NOT NULL CHECK ("documents" IS NULL OR json_valid("documents")),
 "delivery_id" TEXT REFERENCES deliveries(id) DEFERRABLE INITIALLY DEFERRED,
 "inherited_from" TEXT REFERENCES snapshots(id) DEFERRABLE INITIALLY DEFERRED
);
CREATE INDEX idx_snapshots_feature_id ON snapshots("feature_id");
CREATE INDEX idx_snapshots_task_id ON snapshots("task_id");
CREATE INDEX idx_snapshots_kind ON snapshots("kind");
CREATE INDEX idx_snapshots_delivery_id ON snapshots("delivery_id");
CREATE TABLE deliveries (
 "id" TEXT PRIMARY KEY,
 "feature_id" TEXT NOT NULL REFERENCES features(id) DEFERRABLE INITIALLY DEFERRED,
 "task_id" TEXT REFERENCES tasks(id) DEFERRABLE INITIALLY DEFERRED,
 "attempt_id" TEXT REFERENCES attempts(id) DEFERRABLE INITIALLY DEFERRED,
 "merge_id" TEXT REFERENCES merges(id) DEFERRABLE INITIALLY DEFERRED,
 "kind" TEXT NOT NULL,
 "input_version" INTEGER NOT NULL,
 "documents" JSON NOT NULL CHECK ("documents" IS NULL OR json_valid("documents")),
 "evidence" JSON NOT NULL CHECK ("evidence" IS NULL OR json_valid("evidence")),
 "artifact_id" TEXT NOT NULL REFERENCES artifacts(id) DEFERRABLE INITIALLY DEFERRED,
 "candidate" TEXT,
 "stale" BOOLEAN NOT NULL,
 "origin" TEXT NOT NULL
);
CREATE INDEX idx_deliveries_feature_id ON deliveries("feature_id");
CREATE INDEX idx_deliveries_task_id ON deliveries("task_id");
CREATE INDEX idx_deliveries_attempt_id ON deliveries("attempt_id");
CREATE INDEX idx_deliveries_merge_id ON deliveries("merge_id");
CREATE INDEX idx_deliveries_kind ON deliveries("kind");
CREATE INDEX idx_deliveries_artifact_id ON deliveries("artifact_id");
CREATE TABLE reviews (
 "id" TEXT PRIMARY KEY,
 "delivery_id" TEXT NOT NULL REFERENCES deliveries(id) DEFERRABLE INITIALLY DEFERRED,
 "decision" TEXT NOT NULL,
 "checks" JSON NOT NULL CHECK ("checks" IS NULL OR json_valid("checks")),
 "actor" TEXT NOT NULL,
 "run_id" TEXT REFERENCES runs(id) DEFERRABLE INITIALLY DEFERRED,
 "reason" TEXT NOT NULL
);
CREATE INDEX idx_reviews_delivery_id ON reviews("delivery_id");
CREATE INDEX idx_reviews_run_id ON reviews("run_id");
CREATE TABLE overrides (
 "id" TEXT PRIMARY KEY,
 "delivery_id" TEXT NOT NULL REFERENCES deliveries(id) DEFERRABLE INITIALLY DEFERRED,
 "actor" TEXT NOT NULL,
 "reason" TEXT NOT NULL,
 "items" JSON NOT NULL CHECK ("items" IS NULL OR json_valid("items"))
);
CREATE INDEX idx_overrides_delivery_id ON overrides("delivery_id");
CREATE TABLE merges (
 "id" TEXT PRIMARY KEY,
 "feature_id" TEXT NOT NULL REFERENCES features(id) DEFERRABLE INITIALLY DEFERRED,
 "task_id" TEXT REFERENCES tasks(id) DEFERRABLE INITIALLY DEFERRED,
 "status" TEXT NOT NULL,
 "revision" INTEGER NOT NULL,
 "input_version" INTEGER NOT NULL,
 "source" TEXT NOT NULL,
 "source_delivery" TEXT NOT NULL REFERENCES deliveries(id) DEFERRABLE INITIALLY DEFERRED,
 "target" TEXT NOT NULL,
 "expected_target" TEXT NOT NULL,
 "candidate" TEXT NOT NULL,
 "committed_revision" TEXT
);
CREATE INDEX idx_merges_feature_id ON merges("feature_id");
CREATE INDEX idx_merges_task_id ON merges("task_id");
CREATE INDEX idx_merges_status ON merges("status");
CREATE TABLE heads (
 "id" TEXT PRIMARY KEY,
 "project_id" TEXT NOT NULL REFERENCES projects(id) DEFERRABLE INITIALLY DEFERRED,
 "revision" TEXT NOT NULL
);
CREATE INDEX idx_heads_project_id ON heads("project_id");
CREATE TABLE resources (
 "id" TEXT PRIMARY KEY,
 "run_id" TEXT NOT NULL REFERENCES runs(id) DEFERRABLE INITIALLY DEFERRED,
 "kind" TEXT NOT NULL,
 "status" TEXT NOT NULL,
 "origin" TEXT NOT NULL
);
CREATE INDEX idx_resources_run_id ON resources("run_id");
CREATE INDEX idx_resources_kind ON resources("kind");
CREATE INDEX idx_resources_status ON resources("status");
CREATE TABLE artifacts (
 "id" TEXT PRIMARY KEY,
 "feature_id" TEXT NOT NULL REFERENCES features(id) DEFERRABLE INITIALLY DEFERRED,
 "task_id" TEXT REFERENCES tasks(id) DEFERRABLE INITIALLY DEFERRED,
 "relative_path" TEXT NOT NULL,
 "sha256" TEXT NOT NULL,
 "media_type" TEXT NOT NULL,
 "origin" TEXT NOT NULL
);
CREATE INDEX idx_artifacts_feature_id ON artifacts("feature_id");
CREATE INDEX idx_artifacts_task_id ON artifacts("task_id");
CREATE TABLE events (
 "id" INTEGER PRIMARY KEY,
 "target" TEXT NOT NULL,
 "kind" TEXT NOT NULL,
 "detail" TEXT NOT NULL,
 "time" INTEGER NOT NULL
);
CREATE INDEX idx_events_kind ON events("kind");
CREATE INDEX idx_events_time ON events("time");
CREATE TABLE operations (id TEXT PRIMARY KEY, fingerprint TEXT NOT NULL, result JSON NOT NULL CHECK(json_valid(result)));
PRAGMA user_version=1;

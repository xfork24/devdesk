-- Rebuild owners using the SQLite generalized ALTER procedure. The adapter disables FK enforcement only outside the transaction, checks every FK before commit, then reenables it.
CREATE TABLE documents_owned (
 "id" TEXT PRIMARY KEY,
 "feature_id" TEXT REFERENCES features(id) DEFERRABLE INITIALLY DEFERRED,
 "task_id" TEXT REFERENCES tasks(id) DEFERRABLE INITIALLY DEFERRED,
 "kind" TEXT NOT NULL,
 "title" TEXT NOT NULL,
 "body" TEXT NOT NULL,
 "version" INTEGER NOT NULL,
 "supersedes" TEXT REFERENCES documents(id) DEFERRABLE INITIALLY DEFERRED,
 "requirement_id" TEXT REFERENCES documents(id) DEFERRABLE INITIALLY DEFERRED,
 "design_id" TEXT REFERENCES documents(id) DEFERRABLE INITIALLY DEFERRED,
 "origin" TEXT NOT NULL
, acceptance JSON NOT NULL DEFAULT '[]' CHECK(json_valid(acceptance)), verifies JSON NOT NULL DEFAULT '[]' CHECK(json_valid(verifies)), artifact_id TEXT REFERENCES artifacts(id) DEFERRABLE INITIALLY DEFERRED,
 project_id TEXT REFERENCES projects(id) DEFERRABLE INITIALLY DEFERRED,
 CHECK ((project_id IS NOT NULL) != (feature_id IS NOT NULL)),
 CHECK (task_id IS NULL OR feature_id IS NOT NULL)
);
INSERT INTO documents_owned("id","feature_id","task_id","kind","title","body","version","supersedes","requirement_id","design_id","origin","acceptance","verifies","artifact_id",project_id) SELECT "id","feature_id","task_id","kind","title","body","version","supersedes","requirement_id","design_id","origin","acceptance","verifies","artifact_id",NULL FROM documents;
DROP TABLE documents;
ALTER TABLE documents_owned RENAME TO documents;
CREATE INDEX idx_documents_feature_id ON documents("feature_id");
CREATE INDEX idx_documents_task_id ON documents("task_id");
CREATE INDEX idx_documents_kind ON documents("kind");
CREATE INDEX idx_documents_requirement_id ON documents("requirement_id");
CREATE INDEX idx_documents_design_id ON documents("design_id");
CREATE INDEX idx_documents_project_id ON documents(project_id);
CREATE TABLE snapshots_owned (
 "id" TEXT PRIMARY KEY,
 "feature_id" TEXT REFERENCES features(id) DEFERRABLE INITIALLY DEFERRED,
 "task_id" TEXT REFERENCES tasks(id) DEFERRABLE INITIALLY DEFERRED,
 "kind" TEXT NOT NULL,
 "content" TEXT NOT NULL,
 "binding" JSON NOT NULL CHECK ("binding" IS NULL OR json_valid("binding")),
 "documents" JSON NOT NULL CHECK ("documents" IS NULL OR json_valid("documents")),
 "delivery_id" TEXT REFERENCES deliveries(id) DEFERRABLE INITIALLY DEFERRED,
 "inherited_from" TEXT REFERENCES snapshots(id) DEFERRABLE INITIALLY DEFERRED
, capture JSON NOT NULL DEFAULT '{"attempt":null,"runs":[],"sessions":[],"resources":[],"captured_at":null,"reason":"legacy: provenance unavailable"}' CHECK(json_valid(capture)),
 project_id TEXT REFERENCES projects(id) DEFERRABLE INITIALLY DEFERRED,
 CHECK ((project_id IS NOT NULL) != (feature_id IS NOT NULL)),
 CHECK (task_id IS NULL OR feature_id IS NOT NULL)
);
INSERT INTO snapshots_owned("id","feature_id","task_id","kind","content","binding","documents","delivery_id","inherited_from","capture",project_id) SELECT "id","feature_id","task_id","kind","content","binding","documents","delivery_id","inherited_from","capture",NULL FROM snapshots;
DROP TABLE snapshots;
ALTER TABLE snapshots_owned RENAME TO snapshots;
CREATE INDEX idx_snapshots_feature_id ON snapshots("feature_id");
CREATE INDEX idx_snapshots_task_id ON snapshots("task_id");
CREATE INDEX idx_snapshots_kind ON snapshots("kind");
CREATE INDEX idx_snapshots_delivery_id ON snapshots("delivery_id");
CREATE INDEX idx_snapshots_project_id ON snapshots(project_id);
CREATE TABLE artifacts_owned (
 "id" TEXT PRIMARY KEY,
 "feature_id" TEXT REFERENCES features(id) DEFERRABLE INITIALLY DEFERRED,
 "task_id" TEXT REFERENCES tasks(id) DEFERRABLE INITIALLY DEFERRED,
 "relative_path" TEXT NOT NULL,
 "sha256" TEXT NOT NULL,
 "media_type" TEXT NOT NULL,
 "origin" TEXT NOT NULL
,
 project_id TEXT REFERENCES projects(id) DEFERRABLE INITIALLY DEFERRED,
 CHECK ((project_id IS NOT NULL) != (feature_id IS NOT NULL)),
 CHECK (task_id IS NULL OR feature_id IS NOT NULL)
);
INSERT INTO artifacts_owned("id","feature_id","task_id","relative_path","sha256","media_type","origin",project_id) SELECT "id","feature_id","task_id","relative_path","sha256","media_type","origin",NULL FROM artifacts;
DROP TABLE artifacts;
ALTER TABLE artifacts_owned RENAME TO artifacts;
CREATE INDEX idx_artifacts_feature_id ON artifacts("feature_id");
CREATE INDEX idx_artifacts_task_id ON artifacts("task_id");
CREATE INDEX idx_artifacts_project_id ON artifacts(project_id);
ALTER TABLE features ADD COLUMN source_requirement TEXT REFERENCES documents(id) DEFERRABLE INITIALLY DEFERRED;
PRAGMA user_version=16;

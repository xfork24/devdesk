-- Global discussions have their own store and a reserved execution namespace.
-- No synthetic Project row is created. Composite Session/Turn ownership stays enforced.
CREATE TABLE sessions_global (
 id TEXT PRIMARY KEY, owner_id TEXT NOT NULL,
 agent TEXT NOT NULL REFERENCES definitions(id) DEFERRABLE INITIALLY DEFERRED,
 harness TEXT NOT NULL, sandbox TEXT NOT NULL,
 snapshot_id TEXT REFERENCES snapshots(id) DEFERRABLE INITIALLY DEFERRED,
 origin TEXT NOT NULL, instance JSON CHECK(instance IS NULL OR json_valid(instance)),
 project_id TEXT, feature_id TEXT REFERENCES features(id) DEFERRABLE INITIALLY DEFERRED,
 task_id TEXT REFERENCES tasks(id) DEFERRABLE INITIALLY DEFERRED,
 runtime_binding JSON CHECK(runtime_binding IS NULL OR json_valid(runtime_binding)),
 external_session TEXT, predecessor TEXT REFERENCES sessions(id) DEFERRABLE INITIALLY DEFERRED,
 revision INTEGER NOT NULL DEFAULT 0,
 scope TEXT CHECK(COALESCE((
 (scope IS NULL AND project_id IS NULL AND feature_id IS NULL AND task_id IS NULL AND runtime_binding IS NULL)
 OR (scope='global' AND project_id='00000000-0000-4000-8000-000000000000' AND owner_id='global' AND feature_id IS NULL AND task_id IS NULL AND runtime_binding IS NOT NULL)
 OR (scope='project' AND project_id IS NOT NULL AND owner_id=project_id AND feature_id IS NULL AND task_id IS NULL AND runtime_binding IS NOT NULL)
 OR (scope='feature' AND project_id IS NOT NULL AND feature_id IS NOT NULL AND owner_id=feature_id AND task_id IS NULL AND runtime_binding IS NOT NULL)
 OR (scope='task' AND project_id IS NOT NULL AND feature_id IS NOT NULL AND task_id IS NOT NULL AND owner_id=task_id AND runtime_binding IS NOT NULL)
 ),0)), lifecycle TEXT
);
INSERT INTO sessions_global SELECT id,owner_id,agent,harness,sandbox,snapshot_id,origin,instance,project_id,feature_id,task_id,runtime_binding,external_session,predecessor,revision,scope,lifecycle FROM sessions;
CREATE TABLE turns_global (
 id TEXT PRIMARY KEY, project_id TEXT NOT NULL, session_id TEXT NOT NULL,
 client_message_id TEXT NOT NULL, input TEXT NOT NULL, input_digest TEXT NOT NULL,
 status TEXT NOT NULL, external_message TEXT, external_turn TEXT, consumed BOOLEAN NOT NULL,
 started INTEGER, finished INTEGER, run_id TEXT REFERENCES runs(id) DEFERRABLE INITIALLY DEFERRED,
 revision INTEGER NOT NULL, cancel_requested BOOLEAN NOT NULL DEFAULT 0,
 input_snapshot TEXT REFERENCES snapshots(id) DEFERRABLE INITIALLY DEFERRED, source_revision INTEGER,
 code_view JSON CHECK(code_view IS NULL OR json_valid(code_view)),
 UNIQUE(project_id,session_id,id), UNIQUE(session_id,id), UNIQUE(session_id,client_message_id),
 FOREIGN KEY(project_id,session_id) REFERENCES sessions(project_id,id) DEFERRABLE INITIALLY DEFERRED
);
INSERT INTO turns_global SELECT id,project_id,session_id,client_message_id,input,input_digest,status,external_message,external_turn,consumed,started,finished,run_id,revision,cancel_requested,input_snapshot,source_revision,code_view FROM turns;
DROP TABLE turns;
DROP TABLE sessions;
ALTER TABLE sessions_global RENAME TO sessions;
ALTER TABLE turns_global RENAME TO turns;
CREATE UNIQUE INDEX sessions_project_identity ON sessions(project_id,id);
CREATE UNIQUE INDEX sessions_external_identity ON sessions(json_extract(runtime_binding,'$.installation.id'),external_session) WHERE scope IS NOT NULL AND external_session IS NOT NULL;
CREATE UNIQUE INDEX turns_one_active ON turns(session_id) WHERE status NOT IN ('completed','failed','cancelled');
CREATE INDEX turns_project_session ON turns(project_id,session_id);
CREATE TRIGGER sessions_project_insert BEFORE INSERT ON sessions WHEN NEW.project_id IS NOT NULL AND NEW.scope!='global' AND NOT EXISTS(SELECT 1 FROM projects WHERE id=NEW.project_id) BEGIN SELECT RAISE(ABORT,'Session project does not exist'); END;
CREATE TRIGGER sessions_project_update BEFORE UPDATE OF project_id,scope ON sessions WHEN NEW.project_id IS NOT NULL AND NEW.scope!='global' AND NOT EXISTS(SELECT 1 FROM projects WHERE id=NEW.project_id) BEGIN SELECT RAISE(ABORT,'Session project does not exist'); END;
PRAGMA user_version=39;
CREATE INDEX idx_sessions_owner_id ON sessions(owner_id);
CREATE INDEX idx_sessions_snapshot_id ON sessions(snapshot_id);
CREATE TRIGGER projects_session_delete BEFORE DELETE ON projects WHEN EXISTS(SELECT 1 FROM sessions WHERE project_id=OLD.id AND scope!='global') BEGIN SELECT RAISE(ABORT,'Project has discussion sessions'); END;
CREATE TRIGGER projects_session_identity BEFORE UPDATE OF id ON projects WHEN NEW.id!=OLD.id AND EXISTS(SELECT 1 FROM sessions WHERE project_id=OLD.id AND scope!='global') BEGIN SELECT RAISE(ABORT,'Project has discussion sessions'); END;
-- Dataset-wide conversation snapshots and sealed outputs have an explicit all-null material owner.
CREATE TABLE snapshots_global (
 id TEXT PRIMARY KEY, feature_id TEXT REFERENCES features(id) DEFERRABLE INITIALLY DEFERRED,
 task_id TEXT REFERENCES tasks(id) DEFERRABLE INITIALLY DEFERRED, kind TEXT NOT NULL, content TEXT NOT NULL,
 binding JSON NOT NULL CHECK(json_valid(binding)), documents JSON NOT NULL CHECK(json_valid(documents)),
 delivery_id TEXT REFERENCES deliveries(id) DEFERRABLE INITIALLY DEFERRED,
 inherited_from TEXT REFERENCES snapshots(id) DEFERRABLE INITIALLY DEFERRED,
 capture JSON NOT NULL CHECK(json_valid(capture)), project_id TEXT REFERENCES projects(id) DEFERRABLE INITIALLY DEFERRED,
 runtime_bindings JSON NOT NULL DEFAULT '[]' CHECK(json_valid(runtime_bindings)),
 execution_instances JSON NOT NULL DEFAULT '[]' CHECK(json_valid(execution_instances)),
 CHECK(project_id IS NULL OR feature_id IS NULL), CHECK(task_id IS NULL OR feature_id IS NOT NULL)
);
INSERT INTO snapshots_global SELECT id,feature_id,task_id,kind,content,binding,documents,delivery_id,inherited_from,capture,project_id,runtime_bindings,execution_instances FROM snapshots;
DROP TABLE snapshots;
ALTER TABLE snapshots_global RENAME TO snapshots;
CREATE INDEX idx_snapshots_feature_id ON snapshots(feature_id);
CREATE INDEX idx_snapshots_task_id ON snapshots(task_id);
CREATE INDEX idx_snapshots_kind ON snapshots(kind);
CREATE INDEX idx_snapshots_delivery_id ON snapshots(delivery_id);
CREATE INDEX idx_snapshots_project_id ON snapshots(project_id);
CREATE TABLE artifacts_global (
 id TEXT PRIMARY KEY, feature_id TEXT REFERENCES features(id) DEFERRABLE INITIALLY DEFERRED,
 task_id TEXT REFERENCES tasks(id) DEFERRABLE INITIALLY DEFERRED, relative_path TEXT NOT NULL,
 sha256 TEXT NOT NULL, media_type TEXT NOT NULL, origin TEXT NOT NULL,
 project_id TEXT REFERENCES projects(id) DEFERRABLE INITIALLY DEFERRED,
 CHECK(project_id IS NULL OR feature_id IS NULL), CHECK(task_id IS NULL OR feature_id IS NOT NULL),
 CHECK(project_id IS NOT NULL OR feature_id IS NOT NULL OR relative_path LIKE 'artifacts/global/%')
);
INSERT INTO artifacts_global SELECT id,feature_id,task_id,relative_path,sha256,media_type,origin,project_id FROM artifacts;
DROP TABLE artifacts;
ALTER TABLE artifacts_global RENAME TO artifacts;
CREATE INDEX idx_artifacts_feature_id ON artifacts(feature_id);
CREATE INDEX idx_artifacts_task_id ON artifacts(task_id);
CREATE INDEX idx_artifacts_project_id ON artifacts(project_id);

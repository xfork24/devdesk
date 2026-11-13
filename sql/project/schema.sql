-- Current schema baseline. No legacy upgrade path is executed at startup.
BEGIN IMMEDIATE;
CREATE TABLE source_definitions (
 identity TEXT NOT NULL,
 revision INTEGER NOT NULL CHECK(revision > 0),
 type TEXT NOT NULL,
 name TEXT NOT NULL,
 scope JSON NOT NULL CHECK(json_valid(scope)),
 refs JSON NOT NULL CHECK(json_valid(refs)),
 data JSON NOT NULL CHECK(json_valid(data)),
 reference TEXT GENERATED ALWAYS AS (identity || ':' || revision) VIRTUAL UNIQUE,
 PRIMARY KEY(identity,revision)
);
CREATE INDEX idx_source_type ON source_definitions(type,identity,revision);
CREATE TRIGGER source_immutable_update BEFORE UPDATE ON source_definitions BEGIN SELECT RAISE(ABORT,'source revision is immutable'); END;
CREATE TRIGGER source_no_delete BEFORE DELETE ON source_definitions BEGIN SELECT RAISE(ABORT,'source revision cannot be deleted'); END;


CREATE TABLE "artifacts" (
 id TEXT PRIMARY KEY, feature_id TEXT REFERENCES features(id) DEFERRABLE INITIALLY DEFERRED,
 task_id TEXT REFERENCES tasks(id) DEFERRABLE INITIALLY DEFERRED, relative_path TEXT NOT NULL,
 sha256 TEXT NOT NULL, media_type TEXT NOT NULL, origin TEXT NOT NULL,
 project_id TEXT REFERENCES projects(id) DEFERRABLE INITIALLY DEFERRED,
 CHECK(project_id IS NULL OR feature_id IS NULL), CHECK(task_id IS NULL OR feature_id IS NOT NULL),
 CHECK(project_id IS NOT NULL OR feature_id IS NOT NULL OR relative_path LIKE 'artifacts/global/%')
);

CREATE TABLE contexts (
 "id" TEXT PRIMARY KEY,
 "owner_id" TEXT NOT NULL,
 "content" TEXT NOT NULL,
 "revision" INTEGER NOT NULL
);

CREATE TABLE conversation_events (
 id TEXT PRIMARY KEY,
 session_id TEXT NOT NULL,
 turn_id TEXT NOT NULL,
 sequence INTEGER NOT NULL,
 deduplication_key TEXT NOT NULL,
 kind TEXT NOT NULL,
 content TEXT NOT NULL,
 artifact_id TEXT REFERENCES artifacts(id) DEFERRABLE INITIALLY DEFERRED,
 time INTEGER NOT NULL, mode TEXT NOT NULL DEFAULT 'full', provider_item TEXT,
 UNIQUE(session_id,sequence),
 UNIQUE(session_id,deduplication_key),
 FOREIGN KEY(session_id,turn_id) REFERENCES turns(session_id,id) DEFERRABLE INITIALLY DEFERRED
);


CREATE TABLE "documents" (
 "id" TEXT PRIMARY KEY,
 "feature_id" TEXT REFERENCES features(id) DEFERRABLE INITIALLY DEFERRED,
 "task_id" TEXT REFERENCES tasks(id) DEFERRABLE INITIALLY DEFERRED,
 "kind" TEXT NOT NULL,
 "title" TEXT NOT NULL,
 "body" TEXT NOT NULL,
 "version" INTEGER NOT NULL,
 "supersedes" TEXT REFERENCES documents(id) DEFERRABLE INITIALLY DEFERRED,
 input_refs JSON NOT NULL DEFAULT '[]' CHECK(json_valid(input_refs)),
 "origin" TEXT NOT NULL
, artifact_id TEXT REFERENCES artifacts(id) DEFERRABLE INITIALLY DEFERRED,
 project_id TEXT REFERENCES projects(id) DEFERRABLE INITIALLY DEFERRED,
 CHECK ((project_id IS NOT NULL) != (feature_id IS NOT NULL)),
 CHECK (task_id IS NULL OR feature_id IS NOT NULL)
);

CREATE TABLE events (
 "id" INTEGER PRIMARY KEY,
 "target" TEXT NOT NULL,
 "kind" TEXT NOT NULL,
 "detail" TEXT NOT NULL,
 "time" INTEGER NOT NULL
);

CREATE TABLE executions (
 id TEXT PRIMARY KEY,session_id TEXT NOT NULL REFERENCES sessions(id) DEFERRABLE INITIALLY DEFERRED,
 operation_id TEXT NOT NULL UNIQUE,
 container_id TEXT,image_digest TEXT NOT NULL,status TEXT NOT NULL,writer_revoked BOOLEAN NOT NULL,
 started INTEGER,finished INTEGER
, protocol_artifact TEXT REFERENCES artifacts(id) DEFERRABLE INITIALLY DEFERRED, kind TEXT NOT NULL DEFAULT 'harness' CHECK(kind IN ('harness','tests')), permissions JSON CHECK(permissions IS NULL OR json_valid(permissions)));

CREATE TABLE features (
 "id" TEXT PRIMARY KEY,
 "project_id" TEXT NOT NULL REFERENCES projects(id) DEFERRABLE INITIALLY DEFERRED,
 "title" TEXT NOT NULL,
 "status" TEXT NOT NULL,
 "revision" INTEGER NOT NULL,
 "input_version" INTEGER NOT NULL,
 "predecessor" TEXT REFERENCES features(id) DEFERRABLE INITIALLY DEFERRED
, source_materials JSON NOT NULL DEFAULT '[]' CHECK(json_valid(source_materials)));

CREATE TABLE governance_records (
 id TEXT PRIMARY KEY,
 type TEXT NOT NULL,
 definition_id TEXT NOT NULL,
 scope JSON NOT NULL CHECK(json_valid(scope)),
 name TEXT NOT NULL,
 version INTEGER NOT NULL CHECK(version > 0),
 supersedes TEXT,
 content_hash TEXT NOT NULL,
 refs JSON NOT NULL CHECK(json_valid(refs)),
 data JSON NOT NULL CHECK(json_valid(data)),
 immutable BOOLEAN NOT NULL,
 revision INTEGER NOT NULL CHECK(revision > 0),
 created_at INTEGER NOT NULL
);


CREATE TABLE heads (
 "id" TEXT PRIMARY KEY,
 "project_id" TEXT NOT NULL REFERENCES projects(id) DEFERRABLE INITIALLY DEFERRED,
 "revision" TEXT NOT NULL
);

CREATE TABLE "interactions" (
 id TEXT PRIMARY KEY,project_id TEXT NOT NULL,session_id TEXT NOT NULL,turn_id TEXT NOT NULL,
 external_request TEXT NOT NULL,kind TEXT NOT NULL,status TEXT NOT NULL,
 questions JSON NOT NULL CHECK(json_valid(questions)),tool TEXT,reason TEXT NOT NULL,
 answer JSON CHECK(answer IS NULL OR json_valid(answer)),revision INTEGER NOT NULL,
 UNIQUE(session_id,external_request),
 FOREIGN KEY(project_id,session_id,turn_id) REFERENCES turns(project_id,session_id,id) DEFERRABLE INITIALLY DEFERRED
);

CREATE TABLE operations (id TEXT PRIMARY KEY, fingerprint TEXT NOT NULL, result JSON NOT NULL CHECK(json_valid(result)));

CREATE TABLE projects (
 "id" TEXT PRIMARY KEY,
 "title" TEXT NOT NULL,
 "workspace" TEXT NOT NULL,
 "execution_mode" TEXT NOT NULL,
 "concurrency" INTEGER NOT NULL,
 "revision" INTEGER NOT NULL
, workspace_definition JSON CHECK(workspace_definition IS NULL OR json_valid(workspace_definition)));

CREATE TABLE proposals (
 id TEXT PRIMARY KEY,project_id TEXT NOT NULL,session_id TEXT NOT NULL,turn_id TEXT NOT NULL,
 schema_version INTEGER NOT NULL,content JSON NOT NULL CHECK(json_valid(content)),target TEXT,expected_revision INTEGER,
 dependencies JSON NOT NULL CHECK(json_valid(dependencies)),status TEXT NOT NULL,
 artifact_id TEXT REFERENCES artifacts(id) DEFERRABLE INITIALLY DEFERRED,
 adopted_objects JSON NOT NULL CHECK(json_valid(adopted_objects)),
 supersedes TEXT REFERENCES proposals(id) DEFERRABLE INITIALLY DEFERRED,revision INTEGER NOT NULL,
 FOREIGN KEY(project_id,session_id,turn_id) REFERENCES turns(project_id,session_id,id) DEFERRABLE INITIALLY DEFERRED
);

CREATE TABLE repository_operations(id TEXT PRIMARY KEY,project_id TEXT NOT NULL REFERENCES projects(id) DEFERRABLE INITIALLY DEFERRED,action JSON NOT NULL CHECK(json_valid(action)),status TEXT NOT NULL,receipt JSON CHECK(receipt IS NULL OR json_valid(receipt)),reason TEXT NOT NULL,revision INTEGER NOT NULL);

CREATE TABLE repository_worktrees(id TEXT PRIMARY KEY,project_id TEXT NOT NULL REFERENCES projects(id) DEFERRABLE INITIALLY DEFERRED,owner_id TEXT NOT NULL,kind TEXT NOT NULL,branch TEXT NOT NULL,path TEXT NOT NULL UNIQUE,base TEXT NOT NULL,operation_id TEXT NOT NULL REFERENCES repository_operations(id) DEFERRABLE INITIALLY DEFERRED,UNIQUE(project_id,owner_id));


CREATE TABLE "sessions" (
 id TEXT PRIMARY KEY, owner_id TEXT NOT NULL,
 agent TEXT NOT NULL REFERENCES source_definitions(reference) DEFERRABLE INITIALLY DEFERRED,
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

CREATE TABLE "snapshots" (
 id TEXT PRIMARY KEY, feature_id TEXT REFERENCES features(id) DEFERRABLE INITIALLY DEFERRED,
 task_id TEXT REFERENCES tasks(id) DEFERRABLE INITIALLY DEFERRED, kind TEXT NOT NULL, content TEXT NOT NULL,
 documents JSON NOT NULL CHECK(json_valid(documents)),
 inherited_from TEXT REFERENCES snapshots(id) DEFERRABLE INITIALLY DEFERRED,
 capture JSON NOT NULL CHECK(json_valid(capture)), project_id TEXT REFERENCES projects(id) DEFERRABLE INITIALLY DEFERRED,
 runtime_bindings JSON NOT NULL DEFAULT '[]' CHECK(json_valid(runtime_bindings)),
 execution_instances JSON NOT NULL DEFAULT '[]' CHECK(json_valid(execution_instances)),
 CHECK(project_id IS NULL OR feature_id IS NULL), CHECK(task_id IS NULL OR feature_id IS NOT NULL)
);

CREATE TABLE tasks (
 "id" TEXT PRIMARY KEY,
 "feature_id" TEXT NOT NULL REFERENCES features(id) DEFERRABLE INITIALLY DEFERRED,
 "title" TEXT NOT NULL,
 "status" TEXT NOT NULL,
 "revision" INTEGER NOT NULL,
 "inherited_snapshot" TEXT REFERENCES snapshots(id) DEFERRABLE INITIALLY DEFERRED,
 "dependencies" JSON NOT NULL CHECK ("dependencies" IS NULL OR json_valid("dependencies")),
 "worktree" TEXT NOT NULL
, scope JSON NOT NULL DEFAULT '{"input_refs":[],"acceptance":[]}' CHECK(json_valid(scope)));

CREATE TABLE "turns" (
 id TEXT PRIMARY KEY, project_id TEXT NOT NULL, session_id TEXT NOT NULL,
 client_message_id TEXT NOT NULL, input TEXT NOT NULL, input_digest TEXT NOT NULL,
 status TEXT NOT NULL, external_message TEXT, external_turn TEXT, consumed BOOLEAN NOT NULL,
 started INTEGER, finished INTEGER,
 revision INTEGER NOT NULL, cancel_requested BOOLEAN NOT NULL DEFAULT 0,
 input_snapshot TEXT REFERENCES snapshots(id) DEFERRABLE INITIALLY DEFERRED, source_revision INTEGER,
 code_view JSON CHECK(code_view IS NULL OR json_valid(code_view)),
 UNIQUE(project_id,session_id,id), UNIQUE(session_id,id), UNIQUE(session_id,client_message_id),
 FOREIGN KEY(project_id,session_id) REFERENCES sessions(project_id,id) DEFERRABLE INITIALLY DEFERRED
);

CREATE INDEX conversation_events_turn ON conversation_events(turn_id,sequence);

CREATE INDEX idx_artifacts_feature_id ON artifacts(feature_id);

CREATE INDEX idx_artifacts_project_id ON artifacts(project_id);

CREATE INDEX idx_artifacts_task_id ON artifacts(task_id);

CREATE INDEX idx_contexts_owner_id ON contexts("owner_id");

CREATE UNIQUE INDEX idx_delivery_type_revision ON governance_records(type,definition_id,version)
 WHERE type='delivery_type';

CREATE INDEX idx_documents_feature_id ON documents("feature_id");

CREATE INDEX idx_documents_kind ON documents("kind");

CREATE INDEX idx_documents_project_id ON documents(project_id);

CREATE INDEX idx_documents_task_id ON documents("task_id");

CREATE INDEX idx_events_kind ON events("kind");

CREATE INDEX idx_events_time ON events("time");

CREATE UNIQUE INDEX idx_configured_execution_task ON governance_records(type,definition_id)
 WHERE type = 'configured_execution_task';



CREATE INDEX idx_features_project_id ON features("project_id");

CREATE INDEX idx_features_status ON features("status");

CREATE INDEX idx_governance_capability_reference ON governance_records(json_extract(refs,'$.capability_ref')) WHERE type='agent';

CREATE INDEX idx_governance_type ON governance_records(type,definition_id,version);

CREATE UNIQUE INDEX idx_governance_published_version ON governance_records(type,definition_id,version) WHERE type IN ('model','tool','harness','agent','workflow','standard','skill','spec','project_configuration');

CREATE UNIQUE INDEX idx_governance_resource_version ON governance_records(type,definition_id,version) WHERE type IN ('capability','role');

CREATE INDEX idx_governance_scope ON governance_records(json_extract(scope,'$'),type);

CREATE INDEX idx_heads_project_id ON heads("project_id");

CREATE UNIQUE INDEX idx_integration_revision ON governance_records(type,definition_id,version)
 WHERE type IN ('integration','integration_binding','skill_package','integration_project_configuration');

CREATE UNIQUE INDEX idx_permission_resource_revision ON governance_records(type,definition_id,version)
 WHERE type IN ('sandbox','permission_profile','permission_binding','permission_assignment');

CREATE INDEX idx_permission_switch ON governance_records(definition_id) WHERE type='permission_switch';

CREATE INDEX idx_sessions_owner_id ON sessions(owner_id);

CREATE INDEX idx_sessions_snapshot_id ON sessions(snapshot_id);


CREATE INDEX idx_snapshots_feature_id ON snapshots(feature_id);

CREATE INDEX idx_snapshots_kind ON snapshots(kind);

CREATE INDEX idx_snapshots_project_id ON snapshots(project_id);

CREATE INDEX idx_snapshots_task_id ON snapshots(task_id);


CREATE INDEX idx_tasks_feature_id ON tasks("feature_id");

CREATE INDEX idx_tasks_status ON tasks("status");

CREATE INDEX idx_tool_call_session ON governance_records(json_extract(data,'$.invocation.session_id'),json_extract(data,'$.invocation.turn_id')) WHERE type='tool_call';

CREATE INDEX idx_tool_call_status ON governance_records(json_extract(data,'$.status'),created_at) WHERE type='tool_call';

CREATE UNIQUE INDEX sessions_external_identity ON sessions(json_extract(runtime_binding,'$.installation.id'),external_session) WHERE scope IS NOT NULL AND external_session IS NOT NULL;

CREATE UNIQUE INDEX sessions_project_identity ON sessions(project_id,id);

CREATE UNIQUE INDEX turns_one_active ON turns(session_id) WHERE status NOT IN ('completed','failed','cancelled');

CREATE INDEX turns_project_session ON turns(project_id,session_id);

CREATE TRIGGER agent_instance_binding_update BEFORE UPDATE ON governance_records WHEN OLD.type='agent_instance' AND (json_extract(NEW.data,'$.agent_revision_id')!=json_extract(OLD.data,'$.agent_revision_id') OR json_extract(NEW.data,'$.scope')!=json_extract(OLD.data,'$.scope')) BEGIN SELECT RAISE(ABORT,'AgentID binding cannot change'); END;

CREATE TRIGGER governance_identity_update BEFORE UPDATE ON governance_records WHEN NEW.id!=OLD.id OR NEW.type!=OLD.type OR NEW.definition_id!=OLD.definition_id OR NEW.scope!=OLD.scope BEGIN SELECT RAISE(ABORT,'configuration identity cannot change'); END;

CREATE TRIGGER governance_immutable_update BEFORE UPDATE ON governance_records WHEN OLD.immutable=1 BEGIN SELECT RAISE(ABORT,'immutable configuration'); END;

CREATE TRIGGER governance_no_delete BEFORE DELETE ON governance_records BEGIN SELECT RAISE(ABORT,'configuration history cannot be deleted'); END;

CREATE TRIGGER projects_session_delete BEFORE DELETE ON projects WHEN EXISTS(SELECT 1 FROM sessions WHERE project_id=OLD.id AND scope!='global') BEGIN SELECT RAISE(ABORT,'Project has discussion sessions'); END;

CREATE TRIGGER projects_session_identity BEFORE UPDATE OF id ON projects WHEN NEW.id!=OLD.id AND EXISTS(SELECT 1 FROM sessions WHERE project_id=OLD.id AND scope!='global') BEGIN SELECT RAISE(ABORT,'Project has discussion sessions'); END;

CREATE TRIGGER sessions_project_insert BEFORE INSERT ON sessions WHEN NEW.project_id IS NOT NULL AND NEW.scope!='global' AND NOT EXISTS(SELECT 1 FROM projects WHERE id=NEW.project_id) BEGIN SELECT RAISE(ABORT,'Session project does not exist'); END;

CREATE TRIGGER sessions_project_update BEFORE UPDATE OF project_id,scope ON sessions WHEN NEW.project_id IS NOT NULL AND NEW.scope!='global' AND NOT EXISTS(SELECT 1 FROM projects WHERE id=NEW.project_id) BEGIN SELECT RAISE(ABORT,'Session project does not exist'); END;

CREATE UNIQUE INDEX idx_template_catalog_singleton ON governance_records(type) WHERE type='template_catalog_state';
CREATE UNIQUE INDEX idx_template_catalog_member ON governance_records(definition_id,json_extract(data,'$.generation')) WHERE type='template_catalog_member';
CREATE INDEX idx_template_catalog_generation ON governance_records(json_extract(data,'$.generation')) WHERE type='template_catalog_member';

PRAGMA user_version=48;

CREATE TABLE orchestration_wakeups (
 id TEXT PRIMARY KEY, controller_id TEXT NOT NULL, event_cursor INTEGER NOT NULL,
 status TEXT NOT NULL, turn_id TEXT, created_at INTEGER NOT NULL,
 UNIQUE(controller_id,event_cursor)
);
CREATE INDEX orchestration_wakeups_pending ON orchestration_wakeups(status,controller_id);
CREATE TABLE orchestration_leases (
 id TEXT PRIMARY KEY, controller_id TEXT NOT NULL UNIQUE, owner_id TEXT NOT NULL,
 root_instance_id TEXT, epoch INTEGER NOT NULL CHECK(epoch>0), expires_at INTEGER NOT NULL
);
CREATE UNIQUE INDEX orchestration_root_writer ON orchestration_leases(root_instance_id) WHERE root_instance_id IS NOT NULL;

COMMIT;

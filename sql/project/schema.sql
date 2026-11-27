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
CREATE TRIGGER conversation_event_immutable BEFORE UPDATE ON conversation_events BEGIN SELECT RAISE(ABORT,'conversation event is immutable'); END;
CREATE TRIGGER conversation_event_no_delete BEFORE DELETE ON conversation_events BEGIN SELECT RAISE(ABORT,'conversation event cannot be deleted'); END;


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
 owner_project TEXT GENERATED ALWAYS AS (CASE
   WHEN substr(json_extract(scope,'$'),1,8)='project:' THEN substr(json_extract(scope,'$'),9)
   WHEN substr(json_extract(scope,'$'),1,8)='feature:' THEN substr(json_extract(scope,'$'),9,instr(substr(json_extract(scope,'$'),9),':')-1)
 END) VIRTUAL,
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
 answer JSON CHECK(answer IS NULL OR json_valid(answer)),created_at INTEGER,answered_at INTEGER,revision INTEGER NOT NULL,
 UNIQUE(session_id,external_request),
 FOREIGN KEY(project_id,session_id,turn_id) REFERENCES turns(project_id,session_id,id) DEFERRABLE INITIALLY DEFERRED
);

CREATE TABLE operations (id TEXT PRIMARY KEY, fingerprint TEXT NOT NULL, result JSON NOT NULL CHECK(json_valid(result)), versions JSON NOT NULL DEFAULT '[]' CHECK(json_valid(versions)), cursor INTEGER NOT NULL DEFAULT 0) WITHOUT ROWID;
CREATE TABLE resource_versions(kind TEXT NOT NULL,resource_id TEXT NOT NULL,revision INTEGER NOT NULL CHECK(revision>=0),PRIMARY KEY(kind,resource_id)) WITHOUT ROWID;

CREATE TABLE projects (
 "id" TEXT PRIMARY KEY,
 "title" TEXT NOT NULL,
 "workspace" TEXT NOT NULL,
 "execution_mode" TEXT NOT NULL,
 "concurrency" INTEGER NOT NULL,
 "revision" INTEGER NOT NULL
, workspace_definition JSON CHECK(workspace_definition IS NULL OR json_valid(workspace_definition))
, connection JSON CHECK(connection IS NULL OR json_valid(connection)));

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

CREATE INDEX events_workflow_root ON events(
 json_extract(CASE WHEN kind='workflow.runtime' THEN detail ELSE '{}' END,'$.root_instance_id'),id
) WHERE kind='workflow.runtime';
CREATE INDEX idx_controller_owner ON governance_records(json_extract(data,'$.owner_id'),owner_project) WHERE type='workflow_controller';
CREATE INDEX idx_controller_root ON governance_records(json_extract(data,'$.root_instance_id'),owner_project) WHERE type='workflow_controller';
CREATE INDEX idx_controller_calls ON governance_records(definition_id,owner_project) WHERE type='controller_tool_call';
CREATE INDEX idx_controller_turn_calls ON governance_records(json_extract(data,'$.turn_id')) WHERE type='controller_tool_call';
CREATE INDEX idx_workflow_root ON governance_records(json_extract(data,'$.root_instance_id'),owner_project,id) WHERE type='workflow_instance';
CREATE INDEX idx_workflow_active_root ON governance_records(json_extract(data,'$.root_instance_id'),owner_project) WHERE type='workflow_instance' AND coalesce(json_extract(data,'$.status'),'') NOT IN ('completed','cancelled');
CREATE INDEX idx_pending_decisions ON governance_records(definition_id,owner_project) WHERE type='workflow_pending_decision' AND json_extract(data,'$.status')='pending';
CREATE INDEX idx_conversation_session_kind ON conversation_events(session_id,kind);

CREATE INDEX idx_workflow_parent_execution ON governance_records(json_extract(data,'$.parent_execution_id'),owner_project) WHERE type='workflow_instance';

CREATE INDEX idx_live_execution_session ON executions(session_id) WHERE writer_revoked=0;
CREATE INDEX idx_active_turn_status ON turns(status,session_id) WHERE status NOT IN ('queued','completed','failed','cancelled');
CREATE INDEX idx_work_root_owner ON governance_records(owner_project,definition_id) WHERE type='workflow_instance';
CREATE INDEX idx_workflow_owner_current ON governance_records(owner_project,json_extract(data,'$.owner_id')) WHERE type='workflow_instance';
CREATE INDEX idx_workflow_read_history ON governance_records(owner_project,json_extract(data,'$.workflow_instance_id')) WHERE type='workflow_execution_history';
CREATE INDEX idx_workflow_read_plan ON governance_records(owner_project,json_extract(data,'$.instance_id')) WHERE type='workflow_plan_revision';
CREATE INDEX idx_workflow_read_confirm ON governance_records(owner_project,definition_id,json_extract(data,'$.input_digest')) WHERE type='workflow_confirmation';
CREATE INDEX idx_workflow_read_waiver ON governance_records(owner_project,json_extract(data,'$.plan_revision_id')) WHERE type='workflow_waiver';
CREATE INDEX idx_workflow_read_material ON governance_records(owner_project,json_extract(data,'$.root_instance_id')) WHERE type='typed_material';
CREATE INDEX idx_configured_session_execution ON governance_records(owner_project,json_extract(data,'$.execution_id'),definition_id) WHERE type='configured_session';
CREATE UNIQUE INDEX idx_session_authority_source ON governance_records(type,definition_id) WHERE type IN ('configured_session','controller_session','agent_session');
CREATE UNIQUE INDEX idx_workflow_command_identity ON governance_records(json_extract(data,'$.command.command_id')) WHERE type='workflow_intent';
CREATE TRIGGER workflow_intent_basis_immutable BEFORE UPDATE ON governance_records WHEN OLD.type='workflow_intent' AND (json_extract(OLD.data,'$.command') IS NOT json_extract(NEW.data,'$.command') OR json_extract(OLD.data,'$.method') IS NOT json_extract(NEW.data,'$.method') OR json_extract(OLD.data,'$.execution_authority') IS NOT json_extract(NEW.data,'$.execution_authority') OR json_extract(OLD.data,'$.owner_id') IS NOT json_extract(NEW.data,'$.owner_id')) BEGIN SELECT RAISE(ABORT,'workflow intent basis is immutable'); END;
CREATE INDEX idx_permission_transition ON governance_records(definition_id,json_extract(data,'$.status'),json_extract(data,'$.command.payload.client_message_id')) WHERE type='permission_switch';
CREATE INDEX idx_turn_read_started ON turns(session_id,started,id);
CREATE INDEX idx_turn_user_event ON conversation_events(turn_id) WHERE kind='user';
CREATE INDEX idx_invocation_member ON governance_records(owner_project,json_extract(data,'$.child_instance_id'),json_extract(data,'$.member_identity')) WHERE type='workflow_invocation';
CREATE INDEX idx_experience_headers ON governance_records(owner_project,id,revision) WHERE type='experience';
-- Derived collection tokens protect absence/range dependencies. They are not business locks.
CREATE INDEX idx_interactions_pending_turn ON interactions(turn_id,id) WHERE status='pending';
CREATE TRIGGER collection_interactions_insert AFTER INSERT ON interactions BEGIN
 INSERT INTO resource_versions(kind,resource_id,revision) VALUES('collection','interactions:' || NEW.turn_id,1) ON CONFLICT(kind,resource_id) DO UPDATE SET revision=CASE WHEN resource_versions.revision<9223372036854775807 THEN resource_versions.revision+1 ELSE RAISE(ABORT,'collection revision exhausted') END;
END;
CREATE TRIGGER collection_interactions_update AFTER UPDATE ON interactions BEGIN
 INSERT INTO resource_versions(kind,resource_id,revision) VALUES('collection','interactions:' || NEW.turn_id,1) ON CONFLICT(kind,resource_id) DO UPDATE SET revision=CASE WHEN resource_versions.revision<9223372036854775807 THEN resource_versions.revision+1 ELSE RAISE(ABORT,'collection revision exhausted') END;
END;
CREATE TRIGGER interaction_owner_immutable BEFORE UPDATE ON interactions WHEN OLD.session_id!=NEW.session_id OR OLD.turn_id!=NEW.turn_id OR OLD.project_id!=NEW.project_id OR OLD.external_request!=NEW.external_request BEGIN SELECT RAISE(ABORT,'interaction ownership is immutable'); END;
CREATE TRIGGER interaction_no_delete BEFORE DELETE ON interactions BEGIN SELECT RAISE(ABORT,'interaction cannot be deleted'); END;
CREATE TRIGGER session_no_delete BEFORE DELETE ON sessions BEGIN SELECT RAISE(ABORT,'session identity cannot be reused'); END;
CREATE TRIGGER turn_no_delete BEFORE DELETE ON turns BEGIN SELECT RAISE(ABORT,'turn identity cannot be reused'); END;
CREATE TRIGGER turn_identity_immutable BEFORE UPDATE ON turns WHEN OLD.session_id!=NEW.session_id OR OLD.project_id!=NEW.project_id OR OLD.client_message_id!=NEW.client_message_id OR OLD.input!=NEW.input OR OLD.input_digest!=NEW.input_digest OR OLD.input_snapshot IS NOT NEW.input_snapshot OR OLD.code_view IS NOT NEW.code_view OR OLD.source_revision IS NOT NEW.source_revision BEGIN SELECT RAISE(ABORT,'turn input and ownership are immutable'); END;
CREATE INDEX idx_execution_session ON executions(session_id);

CREATE TRIGGER collection_repository_operations_insert AFTER INSERT ON repository_operations BEGIN
 INSERT INTO resource_versions(kind,resource_id,revision) VALUES('collection','repository:' || NEW.project_id,1) ON CONFLICT(kind,resource_id) DO UPDATE SET revision=CASE WHEN resource_versions.revision<9223372036854775807 THEN resource_versions.revision+1 ELSE RAISE(ABORT,'collection revision exhausted') END;
END;

CREATE TRIGGER collection_repository_operations_update AFTER UPDATE ON repository_operations BEGIN
 INSERT INTO resource_versions(kind,resource_id,revision) VALUES('collection','repository:' || NEW.project_id,1) ON CONFLICT(kind,resource_id) DO UPDATE SET revision=CASE WHEN resource_versions.revision<9223372036854775807 THEN resource_versions.revision+1 ELSE RAISE(ABORT,'collection revision exhausted') END;
 INSERT INTO resource_versions(kind,resource_id,revision) SELECT 'collection','repository:' || OLD.project_id,1 WHERE OLD.project_id!=NEW.project_id ON CONFLICT(kind,resource_id) DO UPDATE SET revision=CASE WHEN resource_versions.revision<9223372036854775807 THEN resource_versions.revision+1 ELSE RAISE(ABORT,'collection revision exhausted') END;
END;

CREATE TRIGGER collection_repository_operations_delete AFTER DELETE ON repository_operations BEGIN
 INSERT INTO resource_versions(kind,resource_id,revision) VALUES('collection','repository:' || OLD.project_id,1) ON CONFLICT(kind,resource_id) DO UPDATE SET revision=CASE WHEN resource_versions.revision<9223372036854775807 THEN resource_versions.revision+1 ELSE RAISE(ABORT,'collection revision exhausted') END;
END;

CREATE TRIGGER collection_repository_worktrees_insert AFTER INSERT ON repository_worktrees BEGIN
 INSERT INTO resource_versions(kind,resource_id,revision) VALUES('collection','repository:' || NEW.project_id,1) ON CONFLICT(kind,resource_id) DO UPDATE SET revision=CASE WHEN resource_versions.revision<9223372036854775807 THEN resource_versions.revision+1 ELSE RAISE(ABORT,'collection revision exhausted') END;
END;

CREATE TRIGGER collection_repository_worktrees_update AFTER UPDATE ON repository_worktrees BEGIN
 INSERT INTO resource_versions(kind,resource_id,revision) VALUES('collection','repository:' || NEW.project_id,1) ON CONFLICT(kind,resource_id) DO UPDATE SET revision=CASE WHEN resource_versions.revision<9223372036854775807 THEN resource_versions.revision+1 ELSE RAISE(ABORT,'collection revision exhausted') END;
 INSERT INTO resource_versions(kind,resource_id,revision) SELECT 'collection','repository:' || OLD.project_id,1 WHERE OLD.project_id!=NEW.project_id ON CONFLICT(kind,resource_id) DO UPDATE SET revision=CASE WHEN resource_versions.revision<9223372036854775807 THEN resource_versions.revision+1 ELSE RAISE(ABORT,'collection revision exhausted') END;
END;

CREATE TRIGGER collection_repository_worktrees_delete AFTER DELETE ON repository_worktrees BEGIN
 INSERT INTO resource_versions(kind,resource_id,revision) VALUES('collection','repository:' || OLD.project_id,1) ON CONFLICT(kind,resource_id) DO UPDATE SET revision=CASE WHEN resource_versions.revision<9223372036854775807 THEN resource_versions.revision+1 ELSE RAISE(ABORT,'collection revision exhausted') END;
END;

CREATE TRIGGER collection_heads_insert AFTER INSERT ON heads BEGIN
 INSERT INTO resource_versions(kind,resource_id,revision) VALUES('collection','repository:' || NEW.project_id,1) ON CONFLICT(kind,resource_id) DO UPDATE SET revision=CASE WHEN resource_versions.revision<9223372036854775807 THEN resource_versions.revision+1 ELSE RAISE(ABORT,'collection revision exhausted') END;
END;

CREATE TRIGGER collection_heads_update AFTER UPDATE ON heads BEGIN
 INSERT INTO resource_versions(kind,resource_id,revision) VALUES('collection','repository:' || NEW.project_id,1) ON CONFLICT(kind,resource_id) DO UPDATE SET revision=CASE WHEN resource_versions.revision<9223372036854775807 THEN resource_versions.revision+1 ELSE RAISE(ABORT,'collection revision exhausted') END;
 INSERT INTO resource_versions(kind,resource_id,revision) SELECT 'collection','repository:' || OLD.project_id,1 WHERE OLD.project_id!=NEW.project_id ON CONFLICT(kind,resource_id) DO UPDATE SET revision=CASE WHEN resource_versions.revision<9223372036854775807 THEN resource_versions.revision+1 ELSE RAISE(ABORT,'collection revision exhausted') END;
END;

CREATE TRIGGER collection_heads_delete AFTER DELETE ON heads BEGIN
 INSERT INTO resource_versions(kind,resource_id,revision) VALUES('collection','repository:' || OLD.project_id,1) ON CONFLICT(kind,resource_id) DO UPDATE SET revision=CASE WHEN resource_versions.revision<9223372036854775807 THEN resource_versions.revision+1 ELSE RAISE(ABORT,'collection revision exhausted') END;
END;

CREATE TRIGGER collection_features_insert AFTER INSERT ON features BEGIN
 INSERT INTO resource_versions(kind,resource_id,revision) VALUES('collection','features:' || NEW.project_id,1) ON CONFLICT(kind,resource_id) DO UPDATE SET revision=CASE WHEN resource_versions.revision<9223372036854775807 THEN resource_versions.revision+1 ELSE RAISE(ABORT,'collection revision exhausted') END;
END;

CREATE TRIGGER collection_features_delete AFTER DELETE ON features BEGIN
 INSERT INTO resource_versions(kind,resource_id,revision) VALUES('collection','features:' || OLD.project_id,1) ON CONFLICT(kind,resource_id) DO UPDATE SET revision=CASE WHEN resource_versions.revision<9223372036854775807 THEN resource_versions.revision+1 ELSE RAISE(ABORT,'collection revision exhausted') END;
END;

CREATE TRIGGER collection_features_owner_update AFTER UPDATE OF project_id ON features WHEN OLD.project_id!=NEW.project_id BEGIN
 INSERT INTO resource_versions(kind,resource_id,revision) VALUES('collection','features:' || OLD.project_id,1),('collection','features:' || NEW.project_id,1) ON CONFLICT(kind,resource_id) DO UPDATE SET revision=CASE WHEN resource_versions.revision<9223372036854775807 THEN resource_versions.revision+1 ELSE RAISE(ABORT,'collection revision exhausted') END;
END;

CREATE TRIGGER collection_git_call_insert AFTER INSERT ON governance_records BEGIN
 INSERT INTO resource_versions(kind,resource_id,revision) SELECT 'collection','repository:' || NEW.owner_project,1 WHERE NEW.type='platform_call' AND NEW.owner_project IS NOT NULL AND substr(coalesce(json_extract(NEW.data,'$.handler'),json_extract(NEW.data,'$.request.definition.qualified_name')),1,4)='git.' ON CONFLICT(kind,resource_id) DO UPDATE SET revision=CASE WHEN resource_versions.revision<9223372036854775807 THEN resource_versions.revision+1 ELSE RAISE(ABORT,'collection revision exhausted') END;
END;

CREATE TRIGGER collection_git_call_delete AFTER DELETE ON governance_records BEGIN
 INSERT INTO resource_versions(kind,resource_id,revision) SELECT 'collection','repository:' || OLD.owner_project,1 WHERE OLD.type='platform_call' AND OLD.owner_project IS NOT NULL AND substr(coalesce(json_extract(OLD.data,'$.handler'),json_extract(OLD.data,'$.request.definition.qualified_name')),1,4)='git.' ON CONFLICT(kind,resource_id) DO UPDATE SET revision=CASE WHEN resource_versions.revision<9223372036854775807 THEN resource_versions.revision+1 ELSE RAISE(ABORT,'collection revision exhausted') END;
END;

CREATE TRIGGER collection_git_call_update AFTER UPDATE ON governance_records BEGIN
 INSERT INTO resource_versions(kind,resource_id,revision) SELECT 'collection','repository:' || OLD.owner_project,1 WHERE OLD.type='platform_call' AND OLD.owner_project IS NOT NULL AND substr(coalesce(json_extract(OLD.data,'$.handler'),json_extract(OLD.data,'$.request.definition.qualified_name')),1,4)='git.' ON CONFLICT(kind,resource_id) DO UPDATE SET revision=CASE WHEN resource_versions.revision<9223372036854775807 THEN resource_versions.revision+1 ELSE RAISE(ABORT,'collection revision exhausted') END;
 INSERT INTO resource_versions(kind,resource_id,revision) SELECT 'collection','repository:' || NEW.owner_project,1 WHERE NEW.type='platform_call' AND NEW.owner_project IS NOT NULL AND substr(coalesce(json_extract(NEW.data,'$.handler'),json_extract(NEW.data,'$.request.definition.qualified_name')),1,4)='git.' AND NOT (OLD.type='platform_call' AND OLD.owner_project IS NEW.owner_project AND coalesce(substr(coalesce(json_extract(OLD.data,'$.handler'),json_extract(OLD.data,'$.request.definition.qualified_name')),1,4),'')='git.') ON CONFLICT(kind,resource_id) DO UPDATE SET revision=CASE WHEN resource_versions.revision<9223372036854775807 THEN resource_versions.revision+1 ELSE RAISE(ABORT,'collection revision exhausted') END;
END;
CREATE INDEX idx_repository_pending_project ON repository_operations(project_id,status) WHERE status!='completed';
CREATE INDEX idx_repository_connect_project ON repository_operations(project_id) WHERE json_extract(action,'$.kind')='connect';
CREATE INDEX idx_repository_git_calls ON governance_records(owner_project,json_extract(data,'$.status')) WHERE type='platform_call' AND substr(json_extract(data,'$.request.definition.qualified_name'),1,4)='git.';

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

-- A database clock identifies committed catalog snapshots across pooled connections.
-- This is a read-model token, never an optimistic write precondition for a project.
CREATE TABLE catalog_clock(id INTEGER PRIMARY KEY CHECK(id=1), revision INTEGER NOT NULL CHECK(revision>0));
INSERT INTO catalog_clock VALUES(1,1);
CREATE TRIGGER catalog_source_insert AFTER INSERT ON source_definitions BEGIN UPDATE catalog_clock SET revision=revision+1 WHERE id=1; END;
CREATE TRIGGER catalog_governance_insert AFTER INSERT ON governance_records WHEN NEW.type IN ('project_configuration','work_configuration','integration_project_configuration','workflow_draft','agent_instance','agent_instance_retirement','skill_package','role_retirement','configuration_retirement','template_catalog_state','template_catalog_member','standard_archive','standard_derivation','integration_discovery','integration_disabled','integration_operation','harness_materialization','permission_origin','permission_file_origin','permission_source_selection','permission_binding','permission_assignment') BEGIN UPDATE catalog_clock SET revision=revision+1 WHERE id=1; END;
CREATE TRIGGER catalog_governance_update AFTER UPDATE ON governance_records WHEN NEW.type IN ('project_configuration','work_configuration','integration_project_configuration','workflow_draft','agent_instance','agent_instance_retirement','skill_package','role_retirement','configuration_retirement','template_catalog_state','template_catalog_member','standard_archive','standard_derivation','integration_discovery','integration_disabled','integration_operation','harness_materialization','permission_origin','permission_file_origin','permission_source_selection','permission_binding','permission_assignment') BEGIN UPDATE catalog_clock SET revision=revision+1 WHERE id=1; END;
CREATE TRIGGER catalog_governance_delete AFTER DELETE ON governance_records WHEN OLD.type IN ('project_configuration','work_configuration','integration_project_configuration','workflow_draft','agent_instance','agent_instance_retirement','skill_package','role_retirement','configuration_retirement','template_catalog_state','template_catalog_member','standard_archive','standard_derivation','integration_discovery','integration_disabled','integration_operation','harness_materialization','permission_origin','permission_file_origin','permission_source_selection','permission_binding','permission_assignment') BEGIN UPDATE catalog_clock SET revision=revision+1 WHERE id=1; END;
CREATE TRIGGER catalog_projects_insert AFTER INSERT ON projects BEGIN UPDATE catalog_clock SET revision=revision+1 WHERE id=1; END;
CREATE TRIGGER catalog_projects_update AFTER UPDATE ON projects BEGIN UPDATE catalog_clock SET revision=revision+1 WHERE id=1; END;
CREATE TRIGGER catalog_projects_delete AFTER DELETE ON projects BEGIN UPDATE catalog_clock SET revision=revision+1 WHERE id=1; END;
CREATE TRIGGER catalog_repository_worktrees_insert AFTER INSERT ON repository_worktrees WHEN NEW.kind='mainline' BEGIN UPDATE catalog_clock SET revision=revision+1 WHERE id=1; END;
CREATE TRIGGER catalog_repository_worktrees_update AFTER UPDATE ON repository_worktrees WHEN OLD.kind='mainline' OR NEW.kind='mainline' BEGIN UPDATE catalog_clock SET revision=revision+1 WHERE id=1; END;
CREATE TRIGGER catalog_repository_worktrees_delete AFTER DELETE ON repository_worktrees WHEN OLD.kind='mainline' BEGIN UPDATE catalog_clock SET revision=revision+1 WHERE id=1; END;


CREATE TRIGGER transfer_predicate_insert AFTER INSERT ON governance_records WHEN NEW.type='configuration_transfer' BEGIN
 INSERT INTO resource_versions(kind,resource_id,revision) VALUES('collection','configuration-transfers',1) ON CONFLICT(kind,resource_id) DO UPDATE SET revision=revision+1;
END;
CREATE TRIGGER transfer_predicate_update AFTER UPDATE ON governance_records WHEN NEW.type='configuration_transfer' BEGIN
 INSERT INTO resource_versions(kind,resource_id,revision) VALUES('collection','configuration-transfers',1) ON CONFLICT(kind,resource_id) DO UPDATE SET revision=revision+1;
END;
CREATE TRIGGER transfer_no_delete BEFORE DELETE ON governance_records WHEN OLD.type='configuration_transfer' BEGIN SELECT RAISE(ABORT,'configuration transfer cannot be deleted'); END;

CREATE UNIQUE INDEX idx_proposal_single_successor ON proposals(supersedes) WHERE supersedes IS NOT NULL;
CREATE TRIGGER feature_owner_immutable BEFORE UPDATE ON features WHEN NEW.id!=OLD.id OR NEW.project_id!=OLD.project_id BEGIN SELECT RAISE(ABORT,'feature owner cannot change'); END;
CREATE TRIGGER task_owner_immutable BEFORE UPDATE ON tasks WHEN NEW.id!=OLD.id OR NEW.feature_id!=OLD.feature_id BEGIN SELECT RAISE(ABORT,'task owner cannot change'); END;
CREATE INDEX idx_context_owner ON contexts(owner_id);
CREATE TRIGGER head_resource_insert AFTER INSERT ON heads BEGIN
 INSERT INTO resource_versions(kind,resource_id,revision) VALUES('head',NEW.id,1) ON CONFLICT(kind,resource_id) DO UPDATE SET revision=revision+1;
END;
CREATE TRIGGER head_resource_update AFTER UPDATE ON heads BEGIN
 INSERT INTO resource_versions(kind,resource_id,revision) VALUES('head',NEW.id,1) ON CONFLICT(kind,resource_id) DO UPDATE SET revision=revision+1;
END;
CREATE TRIGGER head_no_delete BEFORE DELETE ON heads BEGIN SELECT RAISE(ABORT,'code resource history cannot be deleted'); END;
CREATE TRIGGER repository_worktree_resource_insert AFTER INSERT ON repository_worktrees BEGIN
 INSERT INTO resource_versions(kind,resource_id,revision) VALUES('repository_worktree',NEW.id,1) ON CONFLICT(kind,resource_id) DO UPDATE SET revision=revision+1;
END;
CREATE TRIGGER repository_worktree_resource_update AFTER UPDATE ON repository_worktrees BEGIN
 INSERT INTO resource_versions(kind,resource_id,revision) VALUES('repository_worktree',NEW.id,1) ON CONFLICT(kind,resource_id) DO UPDATE SET revision=revision+1;
END;
CREATE TRIGGER repository_worktree_no_delete BEFORE DELETE ON repository_worktrees BEGIN SELECT RAISE(ABORT,'code resource history cannot be deleted'); END;
CREATE TRIGGER session_family_insert AFTER INSERT ON sessions WHEN NEW.scope IS NOT NULL BEGIN
 INSERT INTO resource_versions(kind,resource_id,revision) VALUES('session_family',json_array(NEW.owner_id,coalesce((SELECT identity FROM source_definitions WHERE reference=NEW.agent AND type='agent'),NEW.agent),coalesce(json_extract(NEW.runtime_binding,'$.agent_configuration.instance_ref'),json_extract(NEW.runtime_binding,'$.agent_configuration.agent_instance_id'))),1) ON CONFLICT(kind,resource_id) DO UPDATE SET revision=revision+1;
END;
CREATE TRIGGER session_family_update AFTER UPDATE ON sessions WHEN NEW.scope IS NOT NULL BEGIN
 INSERT INTO resource_versions(kind,resource_id,revision) VALUES('session_family',json_array(NEW.owner_id,coalesce((SELECT identity FROM source_definitions WHERE reference=NEW.agent AND type='agent'),NEW.agent),coalesce(json_extract(NEW.runtime_binding,'$.agent_configuration.instance_ref'),json_extract(NEW.runtime_binding,'$.agent_configuration.agent_instance_id'))),1) ON CONFLICT(kind,resource_id) DO UPDATE SET revision=revision+1;
END;
CREATE INDEX idx_session_discussion_family ON sessions(owner_id,coalesce(json_extract(runtime_binding,'$.agent_configuration.instance_ref'),json_extract(runtime_binding,'$.agent_configuration.agent_instance_id')),agent);

-- The driver seeks active IDs without inspecting settled tool history.
CREATE INDEX idx_tool_call_drivable ON governance_records(id) WHERE type='tool_call' AND (json_extract(data,'$.status') IN ('prepared','dispatched','needs_reconciliation') OR (json_extract(data,'$.status') IN ('succeeded','failed','cancelled') AND coalesce(json_extract(data,'$.reply_sent'),0)=0 AND json_extract(data,'$.invocation.session_id') IS NOT NULL));
CREATE TRIGGER execution_no_unrevoke BEFORE UPDATE ON executions WHEN OLD.writer_revoked=1 AND NEW.writer_revoked=0 BEGIN SELECT RAISE(ABORT,'revoked execution cannot resume'); END;
CREATE TRIGGER execution_no_delete BEFORE DELETE ON executions BEGIN SELECT RAISE(ABORT,'execution identity cannot be reused'); END;
CREATE TRIGGER execution_version_insert AFTER INSERT ON executions BEGIN
 INSERT INTO resource_versions(kind,resource_id,revision) VALUES('execution',NEW.id,1);
 INSERT INTO resource_versions(kind,resource_id,revision) VALUES('collection','workers:' || NEW.session_id,1) ON CONFLICT(kind,resource_id) DO UPDATE SET revision=CASE WHEN revision<9223372036854775807 THEN revision+1 ELSE RAISE(ABORT,'worker collection revision exhausted') END;
 INSERT INTO resource_versions(kind,resource_id,revision) SELECT 'collection','conversation-capacity:' || project_id,1 FROM sessions WHERE id=NEW.session_id AND project_id IS NOT NULL ON CONFLICT(kind,resource_id) DO UPDATE SET revision=CASE WHEN revision<9223372036854775807 THEN revision+1 ELSE RAISE(ABORT,'conversation capacity revision exhausted') END;
END;
CREATE TRIGGER execution_version_update AFTER UPDATE ON executions BEGIN
 UPDATE resource_versions SET revision=CASE WHEN revision<9223372036854775807 THEN revision+1 ELSE RAISE(ABORT,'execution revision exhausted') END WHERE kind='execution' AND resource_id=NEW.id;
 INSERT INTO resource_versions(kind,resource_id,revision) VALUES('collection','workers:' || NEW.session_id,1) ON CONFLICT(kind,resource_id) DO UPDATE SET revision=CASE WHEN revision<9223372036854775807 THEN revision+1 ELSE RAISE(ABORT,'worker collection revision exhausted') END;
END;
CREATE TRIGGER execution_capacity_update AFTER UPDATE OF writer_revoked ON executions WHEN OLD.writer_revoked!=NEW.writer_revoked BEGIN
 INSERT INTO resource_versions(kind,resource_id,revision) SELECT 'collection','conversation-capacity:' || project_id,1 FROM sessions WHERE id=NEW.session_id AND project_id IS NOT NULL ON CONFLICT(kind,resource_id) DO UPDATE SET revision=CASE WHEN revision<9223372036854775807 THEN revision+1 ELSE RAISE(ABORT,'conversation capacity revision exhausted') END;
END;
CREATE TRIGGER turn_capacity_insert AFTER INSERT ON turns WHEN NEW.status NOT IN ('queued','completed','failed','cancelled') BEGIN
 INSERT INTO resource_versions(kind,resource_id,revision) VALUES('collection','conversation-capacity:' || NEW.project_id,1) ON CONFLICT(kind,resource_id) DO UPDATE SET revision=CASE WHEN revision<9223372036854775807 THEN revision+1 ELSE RAISE(ABORT,'conversation capacity revision exhausted') END;
END;
CREATE TRIGGER turn_capacity_update AFTER UPDATE OF status ON turns WHEN (OLD.status NOT IN ('queued','completed','failed','cancelled')) != (NEW.status NOT IN ('queued','completed','failed','cancelled')) BEGIN
 INSERT INTO resource_versions(kind,resource_id,revision) VALUES('collection','conversation-capacity:' || NEW.project_id,1) ON CONFLICT(kind,resource_id) DO UPDATE SET revision=CASE WHEN revision<9223372036854775807 THEN revision+1 ELSE RAISE(ABORT,'conversation capacity revision exhausted') END;
END;
CREATE TRIGGER session_authority_immutable BEFORE UPDATE ON sessions WHEN OLD.project_id IS NOT NEW.project_id OR OLD.feature_id IS NOT NEW.feature_id OR OLD.task_id IS NOT NEW.task_id OR OLD.scope IS NOT NEW.scope OR OLD.owner_id!=NEW.owner_id OR OLD.agent!=NEW.agent OR OLD.harness!=NEW.harness OR OLD.sandbox!=NEW.sandbox OR OLD.runtime_binding IS NOT NEW.runtime_binding OR OLD.predecessor IS NOT NEW.predecessor OR (OLD.external_session IS NOT NULL AND OLD.external_session IS NOT NEW.external_session) BEGIN SELECT RAISE(ABORT,'session authority is immutable'); END;
CREATE UNIQUE INDEX idx_worker_stop_once ON governance_records(definition_id) WHERE type='worker_stop_intent';
CREATE INDEX idx_runtime_work_seek ON governance_records(type,json_extract(data,'$.status'),id)
 WHERE json_extract(data,'$.status') IS NOT NULL;
CREATE UNIQUE INDEX idx_conversation_open_once ON governance_records(definition_id) WHERE type='conversation_open_intent';
CREATE INDEX idx_conversation_active_seek ON turns(id) WHERE status NOT IN ('queued','completed','failed','cancelled');
CREATE INDEX idx_conversation_queued_seek ON turns(id) WHERE status='queued';
CREATE INDEX idx_conversation_answer_pending ON interactions(turn_id,id) WHERE status='answered';
CREATE INDEX idx_context_call_pending ON governance_records(id) WHERE type='agent_context_call' AND (json_extract(data,'$.status') IN ('prepared','running') OR coalesce(json_extract(data,'$.reply_sent'),0)=0);
CREATE INDEX idx_context_observations ON governance_records(definition_id,json_extract(data,'$.call_version.revision'),json_extract(data,'$.phase'),id) WHERE type='context_call_observation';
CREATE INDEX idx_platform_call_active ON governance_records(id) WHERE type='platform_call' AND json_extract(data,'$.status') IN ('prepared','running');
CREATE INDEX idx_agent_platform_failed_intent ON governance_records(json_extract(data,'$.command.payload.agent_platform_call_id')) WHERE type='workflow_intent' AND json_extract(data,'$.status')='failed';
CREATE INDEX idx_conversation_turn_kind ON conversation_events(turn_id,kind);
CREATE INDEX idx_documents_project ON documents(project_id);
CREATE INDEX idx_documents_feature ON documents(feature_id);
CREATE INDEX idx_document_successors ON documents(supersedes,kind) WHERE supersedes IS NOT NULL;
CREATE INDEX idx_step_authority_execution ON resource_versions(json_extract(resource_id,'$[1]')) WHERE kind='workflow_step_authority';
CREATE TRIGGER conversation_association_before_worker BEFORE INSERT ON governance_records WHEN NEW.type IN ('configured_session','controller_session') AND EXISTS(SELECT 1 FROM executions WHERE session_id=NEW.definition_id) BEGIN SELECT RAISE(ABORT,'conversation authority must precede its worker'); END;
-- Authority tokens change only when the selected lease changes, rather than
-- whenever unrelated progress/outputs in an aggregate are observed.
CREATE TRIGGER controller_authority_insert AFTER INSERT ON governance_records WHEN NEW.type='workflow_controller' BEGIN
 INSERT INTO resource_versions(kind,resource_id,revision) VALUES('controller_authority',NEW.id,1);
END;
CREATE TRIGGER controller_authority_update AFTER UPDATE ON governance_records WHEN NEW.type='workflow_controller' AND json_array(json_extract(OLD.data,'$.epoch'),json_extract(OLD.data,'$.status'),json_extract(OLD.data,'$.session_id')) != json_array(json_extract(NEW.data,'$.epoch'),json_extract(NEW.data,'$.status'),json_extract(NEW.data,'$.session_id')) BEGIN
 UPDATE resource_versions SET revision=CASE WHEN revision<9223372036854775807 THEN revision+1 ELSE RAISE(ABORT,'controller authority revision exhausted') END WHERE kind='controller_authority' AND resource_id=NEW.id;
END;
CREATE TRIGGER workflow_authority_insert AFTER INSERT ON governance_records WHEN NEW.type='workflow_instance' BEGIN
 INSERT INTO resource_versions(kind,resource_id,revision) VALUES('workflow_run_authority',NEW.id,1);
 INSERT INTO resource_versions(kind,resource_id,revision) SELECT 'workflow_step_authority',json_array(NEW.id,json_extract(step.value,'$.id')),1 FROM json_each(NEW.data,'$.steps') step;
 INSERT INTO resource_versions(kind,resource_id,revision) VALUES('workflow_header',NEW.id,1);
 INSERT INTO resource_versions(kind,resource_id,revision) SELECT 'workflow_step',json_array(NEW.id,json_extract(step.value,'$.id')),1 FROM json_each(NEW.data,'$.steps') step;
END;
CREATE TRIGGER workflow_unique_steps_insert BEFORE INSERT ON governance_records WHEN NEW.type='workflow_instance' AND (SELECT count(*)!=count(DISTINCT json_extract(value,'$.id')) FROM json_each(NEW.data,'$.steps')) BEGIN SELECT RAISE(ABORT,'workflow execution identities must be unique'); END;
CREATE TRIGGER workflow_unique_steps_update BEFORE UPDATE OF data ON governance_records WHEN NEW.type='workflow_instance' AND (SELECT count(*)!=count(DISTINCT json_extract(value,'$.id')) FROM json_each(NEW.data,'$.steps')) BEGIN SELECT RAISE(ABORT,'workflow execution identities must be unique'); END;
CREATE TRIGGER workflow_header_version AFTER UPDATE OF data ON governance_records WHEN NEW.type='workflow_instance' AND json_remove(OLD.data,'$.steps')!=json_remove(NEW.data,'$.steps') BEGIN
 UPDATE resource_versions SET revision=CASE WHEN revision<9223372036854775807 THEN revision+1 ELSE RAISE(ABORT,'workflow header revision exhausted') END WHERE kind='workflow_header' AND resource_id=NEW.id;
END;
-- Materialize each array once so identity joins use SQLite's temporary index.
-- Correlated json_each scans made each step update quadratic in plan size.
CREATE TRIGGER workflow_step_version AFTER UPDATE OF data ON governance_records WHEN NEW.type='workflow_instance' BEGIN
 INSERT INTO resource_versions(kind,resource_id,revision)
 SELECT 'workflow_step',json_array(NEW.id,execution_id),1 FROM (
  WITH previous AS MATERIALIZED (SELECT json_extract(value,'$.id') AS id,value FROM json_each(OLD.data,'$.steps'))
  SELECT json_extract(step.value,'$.id') AS execution_id FROM json_each(NEW.data,'$.steps') step
  LEFT JOIN previous prior ON prior.id=json_extract(step.value,'$.id')
  WHERE prior.value IS NULL OR prior.value!=step.value
 ) WHERE true
 ON CONFLICT(kind,resource_id) DO UPDATE SET revision=CASE WHEN revision<9223372036854775807 THEN revision+1 ELSE RAISE(ABORT,'workflow step revision exhausted') END;
 INSERT INTO resource_versions(kind,resource_id,revision)
 SELECT 'workflow_step',json_array(OLD.id,execution_id),1 FROM (
  WITH current_steps AS MATERIALIZED (SELECT json_extract(value,'$.id') AS id FROM json_each(NEW.data,'$.steps'))
  SELECT json_extract(step.value,'$.id') AS execution_id FROM json_each(OLD.data,'$.steps') step
  LEFT JOIN current_steps current_step ON current_step.id=json_extract(step.value,'$.id') WHERE current_step.id IS NULL
 ) WHERE true
 ON CONFLICT(kind,resource_id) DO UPDATE SET revision=CASE WHEN revision<9223372036854775807 THEN revision+1 ELSE RAISE(ABORT,'workflow step revision exhausted') END;
END;
CREATE TRIGGER workflow_run_authority_update AFTER UPDATE ON governance_records WHEN NEW.type='workflow_instance' AND json_array(json_extract(OLD.data,'$.project_id'),json_extract(OLD.data,'$.status'),json_extract(OLD.data,'$.paused'),json_extract(OLD.data,'$.cancel_requested'),json_extract(OLD.data,'$.parent_execution_id')) != json_array(json_extract(NEW.data,'$.project_id'),json_extract(NEW.data,'$.status'),json_extract(NEW.data,'$.paused'),json_extract(NEW.data,'$.cancel_requested'),json_extract(NEW.data,'$.parent_execution_id')) BEGIN
 UPDATE resource_versions SET revision=CASE WHEN revision<9223372036854775807 THEN revision+1 ELSE RAISE(ABORT,'workflow authority revision exhausted') END WHERE kind='workflow_run_authority' AND resource_id=NEW.id;
END;
CREATE TRIGGER workflow_step_authority_update AFTER UPDATE ON governance_records WHEN NEW.type='workflow_instance' BEGIN
 INSERT INTO resource_versions(kind,resource_id,revision)
 SELECT 'workflow_step_authority',json_array(NEW.id,execution_id),1 FROM (
  WITH previous AS MATERIALIZED (SELECT json_extract(value,'$.id') AS id,json_array(json_extract(value,'$.status'),json_extract(value,'$.plan_revision_id'),json_extract(value,'$.input_digest'),json_extract(value,'$.contract_digest'),json_extract(value,'$.node_id'),json_extract(value,'$.session_id')) AS authority FROM json_each(OLD.data,'$.steps'))
  SELECT json_extract(step.value,'$.id') AS execution_id FROM json_each(NEW.data,'$.steps') step
  LEFT JOIN previous prior ON prior.id=json_extract(step.value,'$.id')
  WHERE coalesce(prior.authority,'')!=json_array(json_extract(step.value,'$.status'),json_extract(step.value,'$.plan_revision_id'),json_extract(step.value,'$.input_digest'),json_extract(step.value,'$.contract_digest'),json_extract(step.value,'$.node_id'),json_extract(step.value,'$.session_id'))
 ) WHERE true
 ON CONFLICT(kind,resource_id) DO UPDATE SET revision=CASE WHEN revision<9223372036854775807 THEN revision+1 ELSE RAISE(ABORT,'step authority revision exhausted') END;
 INSERT INTO resource_versions(kind,resource_id,revision)
 SELECT 'workflow_step_authority',json_array(OLD.id,execution_id),1 FROM (
  WITH current_steps AS MATERIALIZED (SELECT json_extract(value,'$.id') AS id FROM json_each(NEW.data,'$.steps'))
  SELECT json_extract(step.value,'$.id') AS execution_id FROM json_each(OLD.data,'$.steps') step
  LEFT JOIN current_steps current_step ON current_step.id=json_extract(step.value,'$.id') WHERE current_step.id IS NULL
 ) WHERE true
 ON CONFLICT(kind,resource_id) DO UPDATE SET revision=CASE WHEN revision<9223372036854775807 THEN revision+1 ELSE RAISE(ABORT,'step authority revision exhausted') END;
END;
CREATE TRIGGER execution_owner_immutable BEFORE UPDATE ON executions WHEN OLD.session_id!=NEW.session_id OR OLD.operation_id!=NEW.operation_id OR OLD.image_digest!=NEW.image_digest OR OLD.kind!=NEW.kind OR OLD.permissions IS NOT NEW.permissions BEGIN SELECT RAISE(ABORT,'execution ownership is immutable'); END;

-- Pending commands seek active IDs without scanning completed intent history.
CREATE INDEX idx_workflow_intent_prepared ON governance_records(id) WHERE type='workflow_intent' AND json_extract(data,'$.status')='prepared';

CREATE INDEX idx_provider_tool_prepared ON governance_records(id) WHERE type='provider_tool_intent' AND json_extract(data,'$.status')='prepared';
CREATE INDEX idx_provider_poll_prepared ON governance_records(id) WHERE type='provider_poll_pending' AND json_extract(data,'$.status')='prepared';
CREATE INDEX idx_conversation_capture_pending ON governance_records(id) WHERE type IN ('conversation_capture_intent','captured_experience_intent') AND json_extract(data,'$.status') IN ('prepared','capturing','captured');
CREATE INDEX idx_capture_context ON governance_records(json_extract(data,'$.session_id')) WHERE type='conversation_context';
CREATE INDEX idx_capture_context_read ON governance_records(json_extract(data,'$.invocation.session_id'),json_extract(data,'$.status')) WHERE type='agent_context_call';
CREATE TRIGGER snapshot_immutable_update BEFORE UPDATE ON snapshots BEGIN SELECT RAISE(ABORT,'snapshot is immutable'); END;
CREATE TRIGGER snapshot_no_delete BEFORE DELETE ON snapshots BEGIN SELECT RAISE(ABORT,'snapshot cannot be deleted'); END;
CREATE TRIGGER document_immutable_update BEFORE UPDATE ON documents BEGIN SELECT RAISE(ABORT,'document revision is immutable'); END;
CREATE TRIGGER document_no_delete BEFORE DELETE ON documents BEGIN SELECT RAISE(ABORT,'document revision cannot be deleted'); END;

CREATE INDEX idx_platform_receipt_proof ON governance_records(json_extract(data,'$.root_instance_id'),json_extract(data,'$.handler'),json_extract(data,'$.receipt_digest'),id)
 WHERE type='platform_call' AND json_extract(data,'$.status')='succeeded';
CREATE INDEX idx_platform_effect_observation ON governance_records(definition_id,json_extract(data,'$.phase'),id) WHERE type='platform_effect_observation';
CREATE INDEX idx_platform_workspace_fence ON governance_records(owner_project,json_extract(data,'$.status')) WHERE type='platform_workspace_fence';
CREATE INDEX idx_workflow_waiver_source ON governance_records(json_extract(data,'$.plan_revision_id'),json_extract(data,'$.node_id'),json_extract(data,'$.requirement'),id) WHERE type='workflow_waiver';
CREATE TRIGGER platform_workspace_fence_insert AFTER INSERT ON governance_records WHEN NEW.type='platform_workspace_fence' BEGIN
 INSERT INTO resource_versions(kind,resource_id,revision) VALUES('collection','conversation-capacity:' || NEW.owner_project,1) ON CONFLICT(kind,resource_id) DO UPDATE SET revision=CASE WHEN revision<9223372036854775807 THEN revision+1 ELSE RAISE(ABORT,'capacity epoch exhausted') END;
END;
CREATE TRIGGER platform_workspace_fence_update AFTER UPDATE OF data ON governance_records WHEN NEW.type='platform_workspace_fence' AND json_extract(NEW.data,'$.status') IS NOT json_extract(OLD.data,'$.status') BEGIN
 INSERT INTO resource_versions(kind,resource_id,revision) VALUES('collection','conversation-capacity:' || NEW.owner_project,1) ON CONFLICT(kind,resource_id) DO UPDATE SET revision=CASE WHEN revision<9223372036854775807 THEN revision+1 ELSE RAISE(ABORT,'capacity epoch exhausted') END;
END;

CREATE INDEX idx_platform_workspace_grant ON governance_records(json_extract(data,'$.root_instance_id'),json_extract(data,'$.receipt.workspace_id'),json_extract(data,'$.receipt.revision'),json_extract(data,'$.handler'),id) WHERE type='platform_call' AND json_extract(data,'$.status')='succeeded';

CREATE UNIQUE INDEX idx_workflow_driver_root ON governance_records(definition_id) WHERE type='workflow_driver';
CREATE UNIQUE INDEX idx_workflow_active_pause ON governance_records(definition_id) WHERE type='workflow_pause_interval' AND json_extract(data,'$.finished') IS NULL;
CREATE INDEX idx_workflow_pause_time ON governance_records(definition_id,CAST(json_extract(data,'$.started') AS INTEGER)) WHERE type='workflow_pause_interval';
CREATE INDEX idx_controller_turn_input ON governance_records(definition_id,json_extract(data,'$.command.payload.controller_epoch'),json_extract(data,'$.command.payload.client_message_id')) WHERE type='workflow_intent' AND json_extract(data,'$.method')='conversation.send';

CREATE INDEX idx_step_platform_calls ON governance_records(coalesce(json_extract(data,'$.request.caller.execution_id'),definition_id),json_extract(data,'$.status'),id) WHERE type='platform_call';
CREATE TRIGGER step_platform_calls_insert AFTER INSERT ON governance_records WHEN NEW.type='platform_call' BEGIN
 INSERT INTO resource_versions(kind,resource_id,revision) VALUES('collection','platforms:' || coalesce(json_extract(NEW.data,'$.request.caller.execution_id'),NEW.definition_id),1) ON CONFLICT(kind,resource_id) DO UPDATE SET revision=CASE WHEN revision<9223372036854775807 THEN revision+1 ELSE RAISE(ABORT,'platform call epoch exhausted') END;
END;
CREATE TRIGGER step_platform_calls_update AFTER UPDATE OF data ON governance_records WHEN NEW.type='platform_call' AND (json_extract(NEW.data,'$.status') IS NOT json_extract(OLD.data,'$.status') OR json_extract(NEW.data,'$.cancel_requested') IS NOT json_extract(OLD.data,'$.cancel_requested')) BEGIN
 INSERT INTO resource_versions(kind,resource_id,revision) VALUES('collection','platforms:' || coalesce(json_extract(NEW.data,'$.request.caller.execution_id'),NEW.definition_id),1) ON CONFLICT(kind,resource_id) DO UPDATE SET revision=CASE WHEN revision<9223372036854775807 THEN revision+1 ELSE RAISE(ABORT,'platform call epoch exhausted') END;
END;

CREATE INDEX idx_controller_owner_source ON governance_records(json_extract(data,'$.owner_id'),coalesce(json_extract(data,'$.workspace_only'),0),json_extract(data,'$.status')) WHERE type='workflow_controller';
CREATE TRIGGER orchestration_lease_epoch_insert AFTER INSERT ON orchestration_leases BEGIN
 INSERT INTO resource_versions(kind,resource_id,revision) VALUES('orchestration_lease',NEW.id,1) ON CONFLICT(kind,resource_id) DO UPDATE SET revision=CASE WHEN revision<9223372036854775807 THEN revision+1 ELSE RAISE(ABORT,'orchestration_lease epoch exhausted') END;
END;
CREATE TRIGGER orchestration_lease_epoch_update AFTER UPDATE ON orchestration_leases BEGIN
 INSERT INTO resource_versions(kind,resource_id,revision) VALUES('orchestration_lease',NEW.id,1) ON CONFLICT(kind,resource_id) DO UPDATE SET revision=CASE WHEN revision<9223372036854775807 THEN revision+1 ELSE RAISE(ABORT,'orchestration_lease epoch exhausted') END;
END;
CREATE TRIGGER orchestration_lease_epoch_delete AFTER DELETE ON orchestration_leases BEGIN
 INSERT INTO resource_versions(kind,resource_id,revision) VALUES('orchestration_lease',OLD.id,1) ON CONFLICT(kind,resource_id) DO UPDATE SET revision=CASE WHEN revision<9223372036854775807 THEN revision+1 ELSE RAISE(ABORT,'orchestration_lease epoch exhausted') END;
END;
CREATE TRIGGER orchestration_wakeup_epoch_insert AFTER INSERT ON orchestration_wakeups BEGIN
 INSERT INTO resource_versions(kind,resource_id,revision) VALUES('orchestration_wakeup',NEW.id,1) ON CONFLICT(kind,resource_id) DO UPDATE SET revision=CASE WHEN revision<9223372036854775807 THEN revision+1 ELSE RAISE(ABORT,'orchestration_wakeup epoch exhausted') END;
END;
CREATE TRIGGER orchestration_wakeup_epoch_update AFTER UPDATE ON orchestration_wakeups BEGIN
 INSERT INTO resource_versions(kind,resource_id,revision) VALUES('orchestration_wakeup',NEW.id,1) ON CONFLICT(kind,resource_id) DO UPDATE SET revision=CASE WHEN revision<9223372036854775807 THEN revision+1 ELSE RAISE(ABORT,'orchestration_wakeup epoch exhausted') END;
END;
CREATE TRIGGER orchestration_wakeup_epoch_delete AFTER DELETE ON orchestration_wakeups BEGIN
 INSERT INTO resource_versions(kind,resource_id,revision) VALUES('orchestration_wakeup',OLD.id,1) ON CONFLICT(kind,resource_id) DO UPDATE SET revision=CASE WHEN revision<9223372036854775807 THEN revision+1 ELSE RAISE(ABORT,'orchestration_wakeup epoch exhausted') END;
END;

CREATE TRIGGER workflow_capacity_insert AFTER INSERT ON governance_records WHEN NEW.type='workflow_instance' BEGIN
 INSERT INTO resource_versions(kind,resource_id,revision) VALUES('collection','workflow-capacity:' || json_extract(NEW.data,'$.root_instance_id'),1) ON CONFLICT(kind,resource_id) DO UPDATE SET revision=CASE WHEN revision<9223372036854775807 THEN revision+1 ELSE RAISE(ABORT,'workflow capacity epoch exhausted') END;
END;
CREATE TRIGGER workflow_capacity_update AFTER UPDATE OF data ON governance_records WHEN NEW.type='workflow_instance' AND (
 json_extract(NEW.data,'$.status') IS NOT json_extract(OLD.data,'$.status') OR json_extract(NEW.data,'$.plan') IS NOT json_extract(OLD.data,'$.plan') OR
 (SELECT json_group_array(json_array(json_extract(value,'$.id'),json_extract(value,'$.status'))) FROM json_each(NEW.data,'$.steps')) IS NOT (SELECT json_group_array(json_array(json_extract(value,'$.id'),json_extract(value,'$.status'))) FROM json_each(OLD.data,'$.steps'))
) BEGIN
 INSERT INTO resource_versions(kind,resource_id,revision) VALUES('collection','workflow-capacity:' || json_extract(NEW.data,'$.root_instance_id'),1) ON CONFLICT(kind,resource_id) DO UPDATE SET revision=CASE WHEN revision<9223372036854775807 THEN revision+1 ELSE RAISE(ABORT,'workflow capacity epoch exhausted') END;
END;

CREATE UNIQUE INDEX idx_workflow_stop_source ON governance_records(definition_id) WHERE type='workflow_stop';
CREATE TRIGGER workflow_stop_authority AFTER INSERT ON governance_records WHEN NEW.type='workflow_stop' BEGIN
 UPDATE resource_versions SET revision=CASE WHEN revision<9223372036854775807 THEN revision+1 ELSE RAISE(ABORT,'step authority exhausted') END WHERE kind='workflow_step_authority' AND json_extract(resource_id,'$[1]')=NEW.definition_id;
END;

CREATE INDEX idx_workflow_retry_count ON governance_records(json_extract(data,'$.workflow_instance_id'),json_extract(data,'$.node_id'),json_extract(data,'$.round'),definition_id) WHERE type='workflow_execution_history';
CREATE INDEX idx_workflow_last_outputs ON governance_records(json_extract(data,'$.workflow_instance_id'),json_extract(data,'$.node_id'),CAST(json_extract(data,'$.round') AS INTEGER) DESC,CAST(json_extract(data,'$.sequence') AS INTEGER) DESC,CAST(json_extract(data,'$.finished') AS INTEGER) DESC) WHERE type='workflow_execution_history' AND json_extract(data,'$.outputs')!='{}';
CREATE INDEX idx_workflow_snapshot_checkpoint ON governance_records(json_extract(data,'$.value.owner_id'),json_extract(data,'$.value.workspace_id'),json_extract(data,'$.value.revision')) WHERE type='workflow_output' AND immutable=1 AND json_extract(data,'$.provenance.kind')='platform' AND json_extract(data,'$.provenance.tool')='workspace.snapshot';
CREATE UNIQUE INDEX idx_workflow_reconciliation_active ON governance_records(definition_id) WHERE type='workflow_reconciliation' AND json_extract(data,'$.status') IN ('prepared','inspecting');

CREATE UNIQUE INDEX idx_workflow_active_wait ON governance_records(definition_id) WHERE type='workflow_wait_interval' AND json_extract(data,'$.finished') IS NULL;
CREATE INDEX idx_workflow_wait_time ON governance_records(definition_id,CAST(json_extract(data,'$.started') AS INTEGER)) WHERE type='workflow_wait_interval';
CREATE TRIGGER workflow_pause_interval_clock_insert AFTER INSERT ON governance_records WHEN NEW.type='workflow_pause_interval' BEGIN
 INSERT INTO resource_versions(kind,resource_id,revision) VALUES('collection','workflow-pauses:' || NEW.definition_id,1) ON CONFLICT(kind,resource_id) DO UPDATE SET revision=CASE WHEN revision<9223372036854775807 THEN revision+1 ELSE RAISE(ABORT,'wait epoch exhausted') END;
END;
CREATE TRIGGER workflow_pause_interval_clock_update AFTER UPDATE ON governance_records WHEN NEW.type='workflow_pause_interval' BEGIN
 INSERT INTO resource_versions(kind,resource_id,revision) VALUES('collection','workflow-pauses:' || NEW.definition_id,1) ON CONFLICT(kind,resource_id) DO UPDATE SET revision=CASE WHEN revision<9223372036854775807 THEN revision+1 ELSE RAISE(ABORT,'wait epoch exhausted') END;
END;
CREATE TRIGGER workflow_wait_interval_clock_insert AFTER INSERT ON governance_records WHEN NEW.type='workflow_wait_interval' BEGIN
 INSERT INTO resource_versions(kind,resource_id,revision) VALUES('collection','workflow-waits:' || NEW.definition_id,1) ON CONFLICT(kind,resource_id) DO UPDATE SET revision=CASE WHEN revision<9223372036854775807 THEN revision+1 ELSE RAISE(ABORT,'wait epoch exhausted') END;
END;
CREATE TRIGGER workflow_wait_interval_clock_update AFTER UPDATE ON governance_records WHEN NEW.type='workflow_wait_interval' BEGIN
 INSERT INTO resource_versions(kind,resource_id,revision) VALUES('collection','workflow-waits:' || NEW.definition_id,1) ON CONFLICT(kind,resource_id) DO UPDATE SET revision=CASE WHEN revision<9223372036854775807 THEN revision+1 ELSE RAISE(ABORT,'wait epoch exhausted') END;
END;

CREATE INDEX idx_execution_task_source ON governance_records(json_extract(data,'$.source.execution_id'),definition_id) WHERE type IN ('configured_execution_task','archived_configured_execution_task');
CREATE INDEX idx_interaction_wait_time ON interactions(turn_id,created_at) WHERE created_at IS NOT NULL;
CREATE TRIGGER workflow_tree_waits_insert AFTER INSERT ON governance_records WHEN NEW.type IN ('workflow_wait_interval','workflow_pause_interval') BEGIN
 INSERT INTO resource_versions(kind,resource_id,revision)
 SELECT 'collection','workflow-tree-waits:' || json_extract(i.data,'$.root_instance_id'),1 FROM governance_records i WHERE i.type='workflow_instance' AND i.id=CASE WHEN NEW.type='workflow_pause_interval' THEN NEW.definition_id ELSE (SELECT json_extract(v.resource_id,'$[0]') FROM resource_versions v WHERE v.kind='workflow_step_authority' AND json_extract(v.resource_id,'$[1]')=NEW.definition_id LIMIT 1) END
 ON CONFLICT(kind,resource_id) DO UPDATE SET revision=CASE WHEN revision<9223372036854775807 THEN revision+1 ELSE RAISE(ABORT,'workflow waiting epoch exhausted') END;
END;
CREATE TRIGGER workflow_interaction_waits_insert AFTER INSERT ON interactions BEGIN
 INSERT INTO resource_versions(kind,resource_id,revision)
 SELECT 'collection','workflow-tree-waits:' || json_extract(i.data,'$.root_instance_id'),1 FROM governance_records t JOIN governance_records i ON i.id=json_extract(t.data,'$.source.instance_id') AND i.type='workflow_instance' WHERE t.type IN ('configured_execution_task','archived_configured_execution_task') AND t.definition_id=NEW.turn_id
 ON CONFLICT(kind,resource_id) DO UPDATE SET revision=CASE WHEN revision<9223372036854775807 THEN revision+1 ELSE RAISE(ABORT,'workflow waiting epoch exhausted') END;
END;
CREATE TRIGGER workflow_tree_waits_update AFTER UPDATE ON governance_records WHEN NEW.type IN ('workflow_wait_interval','workflow_pause_interval') BEGIN
 INSERT INTO resource_versions(kind,resource_id,revision)
 SELECT 'collection','workflow-tree-waits:' || json_extract(i.data,'$.root_instance_id'),1 FROM governance_records i WHERE i.type='workflow_instance' AND i.id=CASE WHEN NEW.type='workflow_pause_interval' THEN NEW.definition_id ELSE (SELECT json_extract(v.resource_id,'$[0]') FROM resource_versions v WHERE v.kind='workflow_step_authority' AND json_extract(v.resource_id,'$[1]')=NEW.definition_id LIMIT 1) END
 ON CONFLICT(kind,resource_id) DO UPDATE SET revision=CASE WHEN revision<9223372036854775807 THEN revision+1 ELSE RAISE(ABORT,'workflow waiting epoch exhausted') END;
END;
CREATE TRIGGER workflow_interaction_waits_update AFTER UPDATE ON interactions BEGIN
 INSERT INTO resource_versions(kind,resource_id,revision)
 SELECT 'collection','workflow-tree-waits:' || json_extract(i.data,'$.root_instance_id'),1 FROM governance_records t JOIN governance_records i ON i.id=json_extract(t.data,'$.source.instance_id') AND i.type='workflow_instance' WHERE t.type IN ('configured_execution_task','archived_configured_execution_task') AND t.definition_id=NEW.turn_id
 ON CONFLICT(kind,resource_id) DO UPDATE SET revision=CASE WHEN revision<9223372036854775807 THEN revision+1 ELSE RAISE(ABORT,'workflow waiting epoch exhausted') END;
END;

CREATE INDEX idx_events_target_id ON events(target,id);
CREATE INDEX idx_native_driver_mode ON governance_records(type,json_extract(data,'$.mode'),id) WHERE type='workflow_driver';
CREATE INDEX idx_controller_epoch_intent ON governance_records(definition_id,json_extract(data,'$.command.payload.controller_epoch'),json_extract(data,'$.method')) WHERE type='workflow_intent' AND json_extract(data,'$.controller')=1;
CREATE INDEX idx_controller_unconsumed_inputs ON governance_records(definition_id) WHERE type='controller_input' AND json_extract(data,'$.consumed')=0;
CREATE INDEX idx_workflow_applied_plan ON governance_records(definition_id,json_extract(data,'$.new_plan_revision_id')) WHERE type='workflow_change' AND json_extract(data,'$.status')='applied';
CREATE TRIGGER native_controller_input_insert AFTER INSERT ON governance_records WHEN NEW.type='controller_input' BEGIN
 INSERT INTO resource_versions(kind,resource_id,revision) VALUES('collection','controller-inputs:'||NEW.definition_id,1) ON CONFLICT(kind,resource_id) DO UPDATE SET revision=revision+1;
END;
CREATE TRIGGER native_controller_input_update AFTER UPDATE ON governance_records WHEN NEW.type='controller_input' BEGIN
 INSERT INTO resource_versions(kind,resource_id,revision) VALUES('collection','controller-inputs:'||NEW.definition_id,1) ON CONFLICT(kind,resource_id) DO UPDATE SET revision=revision+1;
END;

CREATE UNIQUE INDEX idx_repository_one_worktree_request ON repository_operations(json_extract(action,'$.owner')) WHERE json_extract(action,'$.kind')='create_worktree';
CREATE UNIQUE INDEX idx_repository_one_connect ON repository_operations(project_id) WHERE json_extract(action,'$.kind')='connect';
CREATE INDEX idx_repository_work_seek ON repository_operations(id) WHERE status IN ('prepared','dispatched','observed') OR (status='needs_reconciliation' AND reason='reconcile_requested');
CREATE INDEX idx_task_unmaterialized ON tasks(id) WHERE worktree='' AND status!='completed';

CREATE TRIGGER summary_projects_insert AFTER INSERT ON projects BEGIN INSERT INTO resource_versions(kind,resource_id,revision) VALUES('collection','record-summary:projects',1) ON CONFLICT(kind,resource_id) DO UPDATE SET revision=CASE WHEN revision<9223372036854775807 THEN revision+1 ELSE RAISE(ABORT,'summary revision exhausted') END; END;

CREATE TRIGGER summary_projects_update AFTER UPDATE ON projects BEGIN INSERT INTO resource_versions(kind,resource_id,revision) VALUES('collection','record-summary:projects',1) ON CONFLICT(kind,resource_id) DO UPDATE SET revision=CASE WHEN revision<9223372036854775807 THEN revision+1 ELSE RAISE(ABORT,'summary revision exhausted') END; END;

CREATE TRIGGER summary_projects_delete AFTER DELETE ON projects BEGIN INSERT INTO resource_versions(kind,resource_id,revision) VALUES('collection','record-summary:projects',1) ON CONFLICT(kind,resource_id) DO UPDATE SET revision=CASE WHEN revision<9223372036854775807 THEN revision+1 ELSE RAISE(ABORT,'summary revision exhausted') END; END;

CREATE TRIGGER summary_features_insert AFTER INSERT ON features BEGIN INSERT INTO resource_versions(kind,resource_id,revision) VALUES('collection','record-summary:features',1) ON CONFLICT(kind,resource_id) DO UPDATE SET revision=CASE WHEN revision<9223372036854775807 THEN revision+1 ELSE RAISE(ABORT,'summary revision exhausted') END; END;

CREATE TRIGGER summary_features_update AFTER UPDATE ON features BEGIN INSERT INTO resource_versions(kind,resource_id,revision) VALUES('collection','record-summary:features',1) ON CONFLICT(kind,resource_id) DO UPDATE SET revision=CASE WHEN revision<9223372036854775807 THEN revision+1 ELSE RAISE(ABORT,'summary revision exhausted') END; END;

CREATE TRIGGER summary_features_delete AFTER DELETE ON features BEGIN INSERT INTO resource_versions(kind,resource_id,revision) VALUES('collection','record-summary:features',1) ON CONFLICT(kind,resource_id) DO UPDATE SET revision=CASE WHEN revision<9223372036854775807 THEN revision+1 ELSE RAISE(ABORT,'summary revision exhausted') END; END;

CREATE TRIGGER summary_tasks_insert AFTER INSERT ON tasks BEGIN INSERT INTO resource_versions(kind,resource_id,revision) VALUES('collection','record-summary:tasks',1) ON CONFLICT(kind,resource_id) DO UPDATE SET revision=CASE WHEN revision<9223372036854775807 THEN revision+1 ELSE RAISE(ABORT,'summary revision exhausted') END; END;

CREATE TRIGGER summary_tasks_update AFTER UPDATE ON tasks BEGIN INSERT INTO resource_versions(kind,resource_id,revision) VALUES('collection','record-summary:tasks',1) ON CONFLICT(kind,resource_id) DO UPDATE SET revision=CASE WHEN revision<9223372036854775807 THEN revision+1 ELSE RAISE(ABORT,'summary revision exhausted') END; END;

CREATE TRIGGER summary_tasks_delete AFTER DELETE ON tasks BEGIN INSERT INTO resource_versions(kind,resource_id,revision) VALUES('collection','record-summary:tasks',1) ON CONFLICT(kind,resource_id) DO UPDATE SET revision=CASE WHEN revision<9223372036854775807 THEN revision+1 ELSE RAISE(ABORT,'summary revision exhausted') END; END;

CREATE TRIGGER summary_documents_insert AFTER INSERT ON documents BEGIN INSERT INTO resource_versions(kind,resource_id,revision) VALUES('collection','record-summary:documents',1) ON CONFLICT(kind,resource_id) DO UPDATE SET revision=CASE WHEN revision<9223372036854775807 THEN revision+1 ELSE RAISE(ABORT,'summary revision exhausted') END; END;

CREATE TRIGGER summary_documents_update AFTER UPDATE ON documents BEGIN INSERT INTO resource_versions(kind,resource_id,revision) VALUES('collection','record-summary:documents',1) ON CONFLICT(kind,resource_id) DO UPDATE SET revision=CASE WHEN revision<9223372036854775807 THEN revision+1 ELSE RAISE(ABORT,'summary revision exhausted') END; END;

CREATE TRIGGER summary_documents_delete AFTER DELETE ON documents BEGIN INSERT INTO resource_versions(kind,resource_id,revision) VALUES('collection','record-summary:documents',1) ON CONFLICT(kind,resource_id) DO UPDATE SET revision=CASE WHEN revision<9223372036854775807 THEN revision+1 ELSE RAISE(ABORT,'summary revision exhausted') END; END;

CREATE TRIGGER summary_sessions_insert AFTER INSERT ON sessions BEGIN INSERT INTO resource_versions(kind,resource_id,revision) VALUES('collection','record-summary:sessions',1) ON CONFLICT(kind,resource_id) DO UPDATE SET revision=CASE WHEN revision<9223372036854775807 THEN revision+1 ELSE RAISE(ABORT,'summary revision exhausted') END; END;

CREATE TRIGGER summary_sessions_update AFTER UPDATE ON sessions BEGIN INSERT INTO resource_versions(kind,resource_id,revision) VALUES('collection','record-summary:sessions',1) ON CONFLICT(kind,resource_id) DO UPDATE SET revision=CASE WHEN revision<9223372036854775807 THEN revision+1 ELSE RAISE(ABORT,'summary revision exhausted') END; END;

CREATE TRIGGER summary_sessions_delete AFTER DELETE ON sessions BEGIN INSERT INTO resource_versions(kind,resource_id,revision) VALUES('collection','record-summary:sessions',1) ON CONFLICT(kind,resource_id) DO UPDATE SET revision=CASE WHEN revision<9223372036854775807 THEN revision+1 ELSE RAISE(ABORT,'summary revision exhausted') END; END;

CREATE TRIGGER summary_contexts_insert AFTER INSERT ON contexts BEGIN INSERT INTO resource_versions(kind,resource_id,revision) VALUES('collection','record-summary:contexts',1) ON CONFLICT(kind,resource_id) DO UPDATE SET revision=CASE WHEN revision<9223372036854775807 THEN revision+1 ELSE RAISE(ABORT,'summary revision exhausted') END; END;

CREATE TRIGGER summary_contexts_update AFTER UPDATE ON contexts BEGIN INSERT INTO resource_versions(kind,resource_id,revision) VALUES('collection','record-summary:contexts',1) ON CONFLICT(kind,resource_id) DO UPDATE SET revision=CASE WHEN revision<9223372036854775807 THEN revision+1 ELSE RAISE(ABORT,'summary revision exhausted') END; END;

CREATE TRIGGER summary_contexts_delete AFTER DELETE ON contexts BEGIN INSERT INTO resource_versions(kind,resource_id,revision) VALUES('collection','record-summary:contexts',1) ON CONFLICT(kind,resource_id) DO UPDATE SET revision=CASE WHEN revision<9223372036854775807 THEN revision+1 ELSE RAISE(ABORT,'summary revision exhausted') END; END;

CREATE TRIGGER summary_snapshots_insert AFTER INSERT ON snapshots BEGIN INSERT INTO resource_versions(kind,resource_id,revision) VALUES('collection','record-summary:snapshots',1) ON CONFLICT(kind,resource_id) DO UPDATE SET revision=CASE WHEN revision<9223372036854775807 THEN revision+1 ELSE RAISE(ABORT,'summary revision exhausted') END; END;

CREATE TRIGGER summary_snapshots_update AFTER UPDATE ON snapshots BEGIN INSERT INTO resource_versions(kind,resource_id,revision) VALUES('collection','record-summary:snapshots',1) ON CONFLICT(kind,resource_id) DO UPDATE SET revision=CASE WHEN revision<9223372036854775807 THEN revision+1 ELSE RAISE(ABORT,'summary revision exhausted') END; END;

CREATE TRIGGER summary_snapshots_delete AFTER DELETE ON snapshots BEGIN INSERT INTO resource_versions(kind,resource_id,revision) VALUES('collection','record-summary:snapshots',1) ON CONFLICT(kind,resource_id) DO UPDATE SET revision=CASE WHEN revision<9223372036854775807 THEN revision+1 ELSE RAISE(ABORT,'summary revision exhausted') END; END;

CREATE TRIGGER summary_heads_insert AFTER INSERT ON heads BEGIN INSERT INTO resource_versions(kind,resource_id,revision) VALUES('collection','record-summary:heads',1) ON CONFLICT(kind,resource_id) DO UPDATE SET revision=CASE WHEN revision<9223372036854775807 THEN revision+1 ELSE RAISE(ABORT,'summary revision exhausted') END; END;

CREATE TRIGGER summary_heads_update AFTER UPDATE ON heads BEGIN INSERT INTO resource_versions(kind,resource_id,revision) VALUES('collection','record-summary:heads',1) ON CONFLICT(kind,resource_id) DO UPDATE SET revision=CASE WHEN revision<9223372036854775807 THEN revision+1 ELSE RAISE(ABORT,'summary revision exhausted') END; END;

CREATE TRIGGER summary_heads_delete AFTER DELETE ON heads BEGIN INSERT INTO resource_versions(kind,resource_id,revision) VALUES('collection','record-summary:heads',1) ON CONFLICT(kind,resource_id) DO UPDATE SET revision=CASE WHEN revision<9223372036854775807 THEN revision+1 ELSE RAISE(ABORT,'summary revision exhausted') END; END;

CREATE TRIGGER summary_artifacts_insert AFTER INSERT ON artifacts BEGIN INSERT INTO resource_versions(kind,resource_id,revision) VALUES('collection','record-summary:artifacts',1) ON CONFLICT(kind,resource_id) DO UPDATE SET revision=CASE WHEN revision<9223372036854775807 THEN revision+1 ELSE RAISE(ABORT,'summary revision exhausted') END; END;

CREATE TRIGGER summary_artifacts_update AFTER UPDATE ON artifacts BEGIN INSERT INTO resource_versions(kind,resource_id,revision) VALUES('collection','record-summary:artifacts',1) ON CONFLICT(kind,resource_id) DO UPDATE SET revision=CASE WHEN revision<9223372036854775807 THEN revision+1 ELSE RAISE(ABORT,'summary revision exhausted') END; END;

CREATE TRIGGER summary_artifacts_delete AFTER DELETE ON artifacts BEGIN INSERT INTO resource_versions(kind,resource_id,revision) VALUES('collection','record-summary:artifacts',1) ON CONFLICT(kind,resource_id) DO UPDATE SET revision=CASE WHEN revision<9223372036854775807 THEN revision+1 ELSE RAISE(ABORT,'summary revision exhausted') END; END;

CREATE TRIGGER summary_events_insert AFTER INSERT ON events BEGIN INSERT INTO resource_versions(kind,resource_id,revision) VALUES('collection','record-summary:events',1) ON CONFLICT(kind,resource_id) DO UPDATE SET revision=CASE WHEN revision<9223372036854775807 THEN revision+1 ELSE RAISE(ABORT,'summary revision exhausted') END; END;

CREATE TRIGGER summary_events_update AFTER UPDATE ON events BEGIN INSERT INTO resource_versions(kind,resource_id,revision) VALUES('collection','record-summary:events',1) ON CONFLICT(kind,resource_id) DO UPDATE SET revision=CASE WHEN revision<9223372036854775807 THEN revision+1 ELSE RAISE(ABORT,'summary revision exhausted') END; END;

CREATE TRIGGER summary_events_delete AFTER DELETE ON events BEGIN INSERT INTO resource_versions(kind,resource_id,revision) VALUES('collection','record-summary:events',1) ON CONFLICT(kind,resource_id) DO UPDATE SET revision=CASE WHEN revision<9223372036854775807 THEN revision+1 ELSE RAISE(ABORT,'summary revision exhausted') END; END;

CREATE TRIGGER summary_turns_insert AFTER INSERT ON turns BEGIN INSERT INTO resource_versions(kind,resource_id,revision) VALUES('collection','record-summary:turns',1) ON CONFLICT(kind,resource_id) DO UPDATE SET revision=CASE WHEN revision<9223372036854775807 THEN revision+1 ELSE RAISE(ABORT,'summary revision exhausted') END; END;

CREATE TRIGGER summary_turns_update AFTER UPDATE ON turns BEGIN INSERT INTO resource_versions(kind,resource_id,revision) VALUES('collection','record-summary:turns',1) ON CONFLICT(kind,resource_id) DO UPDATE SET revision=CASE WHEN revision<9223372036854775807 THEN revision+1 ELSE RAISE(ABORT,'summary revision exhausted') END; END;

CREATE TRIGGER summary_turns_delete AFTER DELETE ON turns BEGIN INSERT INTO resource_versions(kind,resource_id,revision) VALUES('collection','record-summary:turns',1) ON CONFLICT(kind,resource_id) DO UPDATE SET revision=CASE WHEN revision<9223372036854775807 THEN revision+1 ELSE RAISE(ABORT,'summary revision exhausted') END; END;

CREATE TRIGGER summary_conversation_events_insert AFTER INSERT ON conversation_events BEGIN INSERT INTO resource_versions(kind,resource_id,revision) VALUES('collection','record-summary:conversation_events',1) ON CONFLICT(kind,resource_id) DO UPDATE SET revision=CASE WHEN revision<9223372036854775807 THEN revision+1 ELSE RAISE(ABORT,'summary revision exhausted') END; END;

CREATE TRIGGER summary_conversation_events_update AFTER UPDATE ON conversation_events BEGIN INSERT INTO resource_versions(kind,resource_id,revision) VALUES('collection','record-summary:conversation_events',1) ON CONFLICT(kind,resource_id) DO UPDATE SET revision=CASE WHEN revision<9223372036854775807 THEN revision+1 ELSE RAISE(ABORT,'summary revision exhausted') END; END;

CREATE TRIGGER summary_conversation_events_delete AFTER DELETE ON conversation_events BEGIN INSERT INTO resource_versions(kind,resource_id,revision) VALUES('collection','record-summary:conversation_events',1) ON CONFLICT(kind,resource_id) DO UPDATE SET revision=CASE WHEN revision<9223372036854775807 THEN revision+1 ELSE RAISE(ABORT,'summary revision exhausted') END; END;

CREATE TRIGGER summary_interactions_insert AFTER INSERT ON interactions BEGIN INSERT INTO resource_versions(kind,resource_id,revision) VALUES('collection','record-summary:interactions',1) ON CONFLICT(kind,resource_id) DO UPDATE SET revision=CASE WHEN revision<9223372036854775807 THEN revision+1 ELSE RAISE(ABORT,'summary revision exhausted') END; END;

CREATE TRIGGER summary_interactions_update AFTER UPDATE ON interactions BEGIN INSERT INTO resource_versions(kind,resource_id,revision) VALUES('collection','record-summary:interactions',1) ON CONFLICT(kind,resource_id) DO UPDATE SET revision=CASE WHEN revision<9223372036854775807 THEN revision+1 ELSE RAISE(ABORT,'summary revision exhausted') END; END;

CREATE TRIGGER summary_interactions_delete AFTER DELETE ON interactions BEGIN INSERT INTO resource_versions(kind,resource_id,revision) VALUES('collection','record-summary:interactions',1) ON CONFLICT(kind,resource_id) DO UPDATE SET revision=CASE WHEN revision<9223372036854775807 THEN revision+1 ELSE RAISE(ABORT,'summary revision exhausted') END; END;

CREATE TRIGGER summary_proposals_insert AFTER INSERT ON proposals BEGIN INSERT INTO resource_versions(kind,resource_id,revision) VALUES('collection','record-summary:proposals',1) ON CONFLICT(kind,resource_id) DO UPDATE SET revision=CASE WHEN revision<9223372036854775807 THEN revision+1 ELSE RAISE(ABORT,'summary revision exhausted') END; END;

CREATE TRIGGER summary_proposals_update AFTER UPDATE ON proposals BEGIN INSERT INTO resource_versions(kind,resource_id,revision) VALUES('collection','record-summary:proposals',1) ON CONFLICT(kind,resource_id) DO UPDATE SET revision=CASE WHEN revision<9223372036854775807 THEN revision+1 ELSE RAISE(ABORT,'summary revision exhausted') END; END;

CREATE TRIGGER summary_proposals_delete AFTER DELETE ON proposals BEGIN INSERT INTO resource_versions(kind,resource_id,revision) VALUES('collection','record-summary:proposals',1) ON CONFLICT(kind,resource_id) DO UPDATE SET revision=CASE WHEN revision<9223372036854775807 THEN revision+1 ELSE RAISE(ABORT,'summary revision exhausted') END; END;

CREATE TRIGGER summary_executions_insert AFTER INSERT ON executions BEGIN INSERT INTO resource_versions(kind,resource_id,revision) VALUES('collection','record-summary:executions',1) ON CONFLICT(kind,resource_id) DO UPDATE SET revision=CASE WHEN revision<9223372036854775807 THEN revision+1 ELSE RAISE(ABORT,'summary revision exhausted') END; END;

CREATE TRIGGER summary_executions_update AFTER UPDATE ON executions BEGIN INSERT INTO resource_versions(kind,resource_id,revision) VALUES('collection','record-summary:executions',1) ON CONFLICT(kind,resource_id) DO UPDATE SET revision=CASE WHEN revision<9223372036854775807 THEN revision+1 ELSE RAISE(ABORT,'summary revision exhausted') END; END;

CREATE TRIGGER summary_executions_delete AFTER DELETE ON executions BEGIN INSERT INTO resource_versions(kind,resource_id,revision) VALUES('collection','record-summary:executions',1) ON CONFLICT(kind,resource_id) DO UPDATE SET revision=CASE WHEN revision<9223372036854775807 THEN revision+1 ELSE RAISE(ABORT,'summary revision exhausted') END; END;

CREATE TRIGGER summary_repository_operations_insert AFTER INSERT ON repository_operations BEGIN INSERT INTO resource_versions(kind,resource_id,revision) VALUES('collection','record-summary:repository_operations',1) ON CONFLICT(kind,resource_id) DO UPDATE SET revision=CASE WHEN revision<9223372036854775807 THEN revision+1 ELSE RAISE(ABORT,'summary revision exhausted') END; END;

CREATE TRIGGER summary_repository_operations_update AFTER UPDATE ON repository_operations BEGIN INSERT INTO resource_versions(kind,resource_id,revision) VALUES('collection','record-summary:repository_operations',1) ON CONFLICT(kind,resource_id) DO UPDATE SET revision=CASE WHEN revision<9223372036854775807 THEN revision+1 ELSE RAISE(ABORT,'summary revision exhausted') END; END;

CREATE TRIGGER summary_repository_operations_delete AFTER DELETE ON repository_operations BEGIN INSERT INTO resource_versions(kind,resource_id,revision) VALUES('collection','record-summary:repository_operations',1) ON CONFLICT(kind,resource_id) DO UPDATE SET revision=CASE WHEN revision<9223372036854775807 THEN revision+1 ELSE RAISE(ABORT,'summary revision exhausted') END; END;

CREATE TRIGGER summary_repository_worktrees_insert AFTER INSERT ON repository_worktrees BEGIN INSERT INTO resource_versions(kind,resource_id,revision) VALUES('collection','record-summary:repository_worktrees',1) ON CONFLICT(kind,resource_id) DO UPDATE SET revision=CASE WHEN revision<9223372036854775807 THEN revision+1 ELSE RAISE(ABORT,'summary revision exhausted') END; END;

CREATE TRIGGER summary_repository_worktrees_update AFTER UPDATE ON repository_worktrees BEGIN INSERT INTO resource_versions(kind,resource_id,revision) VALUES('collection','record-summary:repository_worktrees',1) ON CONFLICT(kind,resource_id) DO UPDATE SET revision=CASE WHEN revision<9223372036854775807 THEN revision+1 ELSE RAISE(ABORT,'summary revision exhausted') END; END;

CREATE TRIGGER summary_repository_worktrees_delete AFTER DELETE ON repository_worktrees BEGIN INSERT INTO resource_versions(kind,resource_id,revision) VALUES('collection','record-summary:repository_worktrees',1) ON CONFLICT(kind,resource_id) DO UPDATE SET revision=CASE WHEN revision<9223372036854775807 THEN revision+1 ELSE RAISE(ABORT,'summary revision exhausted') END; END;

CREATE TRIGGER summary_governance_records_insert AFTER INSERT ON governance_records BEGIN INSERT INTO resource_versions(kind,resource_id,revision) VALUES('collection','record-summary:governance',1) ON CONFLICT(kind,resource_id) DO UPDATE SET revision=CASE WHEN revision<9223372036854775807 THEN revision+1 ELSE RAISE(ABORT,'summary revision exhausted') END; END;

CREATE TRIGGER summary_governance_records_update AFTER UPDATE ON governance_records BEGIN INSERT INTO resource_versions(kind,resource_id,revision) VALUES('collection','record-summary:governance',1) ON CONFLICT(kind,resource_id) DO UPDATE SET revision=CASE WHEN revision<9223372036854775807 THEN revision+1 ELSE RAISE(ABORT,'summary revision exhausted') END; END;

CREATE TRIGGER summary_governance_records_delete AFTER DELETE ON governance_records BEGIN INSERT INTO resource_versions(kind,resource_id,revision) VALUES('collection','record-summary:governance',1) ON CONFLICT(kind,resource_id) DO UPDATE SET revision=CASE WHEN revision<9223372036854775807 THEN revision+1 ELSE RAISE(ABORT,'summary revision exhausted') END; END;

CREATE TRIGGER summary_orchestration_leases_insert AFTER INSERT ON orchestration_leases BEGIN INSERT INTO resource_versions(kind,resource_id,revision) VALUES('collection','record-summary:orchestration_leases',1) ON CONFLICT(kind,resource_id) DO UPDATE SET revision=CASE WHEN revision<9223372036854775807 THEN revision+1 ELSE RAISE(ABORT,'summary revision exhausted') END; END;

CREATE TRIGGER summary_orchestration_leases_update AFTER UPDATE ON orchestration_leases BEGIN INSERT INTO resource_versions(kind,resource_id,revision) VALUES('collection','record-summary:orchestration_leases',1) ON CONFLICT(kind,resource_id) DO UPDATE SET revision=CASE WHEN revision<9223372036854775807 THEN revision+1 ELSE RAISE(ABORT,'summary revision exhausted') END; END;

CREATE TRIGGER summary_orchestration_leases_delete AFTER DELETE ON orchestration_leases BEGIN INSERT INTO resource_versions(kind,resource_id,revision) VALUES('collection','record-summary:orchestration_leases',1) ON CONFLICT(kind,resource_id) DO UPDATE SET revision=CASE WHEN revision<9223372036854775807 THEN revision+1 ELSE RAISE(ABORT,'summary revision exhausted') END; END;

CREATE TRIGGER summary_orchestration_wakeups_insert AFTER INSERT ON orchestration_wakeups BEGIN INSERT INTO resource_versions(kind,resource_id,revision) VALUES('collection','record-summary:orchestration_wakeups',1) ON CONFLICT(kind,resource_id) DO UPDATE SET revision=CASE WHEN revision<9223372036854775807 THEN revision+1 ELSE RAISE(ABORT,'summary revision exhausted') END; END;

CREATE TRIGGER summary_orchestration_wakeups_update AFTER UPDATE ON orchestration_wakeups BEGIN INSERT INTO resource_versions(kind,resource_id,revision) VALUES('collection','record-summary:orchestration_wakeups',1) ON CONFLICT(kind,resource_id) DO UPDATE SET revision=CASE WHEN revision<9223372036854775807 THEN revision+1 ELSE RAISE(ABORT,'summary revision exhausted') END; END;

CREATE TRIGGER summary_orchestration_wakeups_delete AFTER DELETE ON orchestration_wakeups BEGIN INSERT INTO resource_versions(kind,resource_id,revision) VALUES('collection','record-summary:orchestration_wakeups',1) ON CONFLICT(kind,resource_id) DO UPDATE SET revision=CASE WHEN revision<9223372036854775807 THEN revision+1 ELSE RAISE(ABORT,'summary revision exhausted') END; END;

CREATE TRIGGER summary_source_definitions_insert AFTER INSERT ON source_definitions BEGIN INSERT INTO resource_versions(kind,resource_id,revision) VALUES('collection','record-summary:governance',1) ON CONFLICT(kind,resource_id) DO UPDATE SET revision=CASE WHEN revision<9223372036854775807 THEN revision+1 ELSE RAISE(ABORT,'summary revision exhausted') END; END;

CREATE TRIGGER summary_source_definitions_update AFTER UPDATE ON source_definitions BEGIN INSERT INTO resource_versions(kind,resource_id,revision) VALUES('collection','record-summary:governance',1) ON CONFLICT(kind,resource_id) DO UPDATE SET revision=CASE WHEN revision<9223372036854775807 THEN revision+1 ELSE RAISE(ABORT,'summary revision exhausted') END; END;

CREATE TRIGGER summary_source_definitions_delete AFTER DELETE ON source_definitions BEGIN INSERT INTO resource_versions(kind,resource_id,revision) VALUES('collection','record-summary:governance',1) ON CONFLICT(kind,resource_id) DO UPDATE SET revision=CASE WHEN revision<9223372036854775807 THEN revision+1 ELSE RAISE(ABORT,'summary revision exhausted') END; END;

-- Small committed read metadata; it contains no expanded configuration bodies.
CREATE TABLE workflow_display_headers(instance_id TEXT PRIMARY KEY,record JSON NOT NULL CHECK(json_valid(record)));
CREATE TABLE workflow_display_steps(instance_id TEXT NOT NULL,execution_id TEXT NOT NULL,data JSON NOT NULL CHECK(json_valid(data)),step_revision INTEGER NOT NULL CHECK(step_revision>0),PRIMARY KEY(instance_id,execution_id));
CREATE INDEX idx_workflow_display_root ON workflow_display_headers(json_extract(record,'$.data.root_instance_id'),instance_id);
CREATE INDEX idx_workflow_display_owner ON workflow_display_headers(json_extract(record,'$.data.owner_id'),instance_id);

CREATE TABLE dataset_layout(id INTEGER PRIMARY KEY CHECK(id=1),contract TEXT NOT NULL);
INSERT INTO dataset_layout VALUES(1,'single-host-resource-v2');

CREATE INDEX idx_workflow_timed_wait ON workflow_display_steps(instance_id,json_extract(data,'$.time_budget.next_deadline_at')) WHERE json_extract(data,'$.time_budget.next_deadline_at') IS NOT NULL;
CREATE TABLE payload_pack_objects (
 sha256 TEXT PRIMARY KEY,
 pack TEXT NOT NULL,
 offset INTEGER NOT NULL CHECK(offset >= 0),
 bytes INTEGER NOT NULL CHECK(bytes >= 0)
) WITHOUT ROWID;
CREATE INDEX idx_payload_pack_file ON payload_pack_objects(pack);
-- Source dependency index.
CREATE TABLE governance_source_scans (
        consumer TEXT PRIMARY KEY REFERENCES governance_records(id) ON DELETE CASCADE,
        content_hash TEXT NOT NULL, codec INTEGER NOT NULL,
        reference_count INTEGER NOT NULL CHECK(reference_count>=0)
    ) WITHOUT ROWID;
    CREATE TABLE governance_source_refs (
        consumer TEXT NOT NULL REFERENCES governance_source_scans(consumer) ON DELETE CASCADE,
        reference TEXT NOT NULL, PRIMARY KEY(consumer,reference)
    ) WITHOUT ROWID;
    CREATE INDEX idx_governance_source_ref ON governance_source_refs(reference,consumer);
PRAGMA user_version=63;
COMMIT;

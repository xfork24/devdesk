-- Legacy Run sessions retain NULL scope/binding and revision=0; no external identity is invented.
ALTER TABLE sessions ADD COLUMN project_id TEXT REFERENCES projects(id) DEFERRABLE INITIALLY DEFERRED;
ALTER TABLE sessions ADD COLUMN feature_id TEXT REFERENCES features(id) DEFERRABLE INITIALLY DEFERRED;
ALTER TABLE sessions ADD COLUMN task_id TEXT REFERENCES tasks(id) DEFERRABLE INITIALLY DEFERRED;
ALTER TABLE sessions ADD COLUMN runtime_binding JSON CHECK(runtime_binding IS NULL OR json_valid(runtime_binding));
ALTER TABLE sessions ADD COLUMN external_session TEXT;
ALTER TABLE sessions ADD COLUMN predecessor TEXT REFERENCES sessions(id) DEFERRABLE INITIALLY DEFERRED;
ALTER TABLE sessions ADD COLUMN revision INTEGER NOT NULL DEFAULT 0;
ALTER TABLE sessions ADD COLUMN scope TEXT CHECK(COALESCE((
 (scope IS NULL AND project_id IS NULL AND feature_id IS NULL AND task_id IS NULL AND runtime_binding IS NULL)
 OR (scope='project' AND project_id IS NOT NULL AND owner_id=project_id AND feature_id IS NULL AND task_id IS NULL AND runtime_binding IS NOT NULL)
 OR (scope='feature' AND project_id IS NOT NULL AND feature_id IS NOT NULL AND owner_id=feature_id AND task_id IS NULL AND runtime_binding IS NOT NULL)
 OR (scope='task' AND project_id IS NOT NULL AND feature_id IS NOT NULL AND task_id IS NOT NULL AND owner_id=task_id AND runtime_binding IS NOT NULL)
),0));
CREATE UNIQUE INDEX sessions_project_identity ON sessions(project_id,id);
CREATE UNIQUE INDEX sessions_external_identity ON sessions(json_extract(runtime_binding,'$.installation.id'),external_session) WHERE scope IS NOT NULL AND external_session IS NOT NULL;
CREATE TABLE turns (
 id TEXT PRIMARY KEY,
 project_id TEXT NOT NULL REFERENCES projects(id) DEFERRABLE INITIALLY DEFERRED,
 session_id TEXT NOT NULL,
 client_message_id TEXT NOT NULL,
 input TEXT NOT NULL,
 input_digest TEXT NOT NULL,
 status TEXT NOT NULL,
 external_message TEXT,
 external_turn TEXT,
 consumed BOOLEAN NOT NULL,
 started INTEGER,
 finished INTEGER,
 run_id TEXT REFERENCES runs(id) DEFERRABLE INITIALLY DEFERRED,
 revision INTEGER NOT NULL,
 UNIQUE(project_id,session_id,id),
 UNIQUE(session_id,id),
 UNIQUE(session_id,client_message_id),
 FOREIGN KEY(project_id,session_id) REFERENCES sessions(project_id,id) DEFERRABLE INITIALLY DEFERRED
);
CREATE UNIQUE INDEX turns_one_active ON turns(session_id) WHERE status NOT IN ('completed','failed','cancelled');
CREATE INDEX turns_project_session ON turns(project_id,session_id);
CREATE TABLE conversation_events (
 id TEXT PRIMARY KEY,
 session_id TEXT NOT NULL,
 turn_id TEXT NOT NULL,
 sequence INTEGER NOT NULL,
 deduplication_key TEXT NOT NULL,
 kind TEXT NOT NULL,
 content TEXT NOT NULL,
 artifact_id TEXT REFERENCES artifacts(id) DEFERRABLE INITIALLY DEFERRED,
 time INTEGER NOT NULL,
 UNIQUE(session_id,sequence),
 UNIQUE(session_id,deduplication_key),
 FOREIGN KEY(session_id,turn_id) REFERENCES turns(session_id,id) DEFERRABLE INITIALLY DEFERRED
);
CREATE INDEX conversation_events_turn ON conversation_events(turn_id,sequence);
CREATE TABLE interactions (
 id TEXT PRIMARY KEY,project_id TEXT NOT NULL,session_id TEXT NOT NULL,turn_id TEXT NOT NULL,
 external_request TEXT NOT NULL,kind TEXT NOT NULL,status TEXT NOT NULL,
 questions JSON NOT NULL CHECK(json_valid(questions)),tool TEXT,reason TEXT NOT NULL,answer TEXT,revision INTEGER NOT NULL,
 UNIQUE(session_id,external_request),
 FOREIGN KEY(project_id,session_id,turn_id) REFERENCES turns(project_id,session_id,id) DEFERRABLE INITIALLY DEFERRED
);
CREATE TABLE proposals (
 id TEXT PRIMARY KEY,project_id TEXT NOT NULL,session_id TEXT NOT NULL,turn_id TEXT NOT NULL,
 schema_version INTEGER NOT NULL,content JSON NOT NULL CHECK(json_valid(content)),target TEXT,expected_revision INTEGER,
 dependencies JSON NOT NULL CHECK(json_valid(dependencies)),status TEXT NOT NULL,
 artifact_id TEXT REFERENCES artifacts(id) DEFERRABLE INITIALLY DEFERRED,
 adopted_objects JSON NOT NULL CHECK(json_valid(adopted_objects)),
 supersedes TEXT REFERENCES proposals(id) DEFERRABLE INITIALLY DEFERRED,revision INTEGER NOT NULL,
 FOREIGN KEY(project_id,session_id,turn_id) REFERENCES turns(project_id,session_id,id) DEFERRABLE INITIALLY DEFERRED
);
CREATE TABLE executions (
 id TEXT PRIMARY KEY,session_id TEXT NOT NULL REFERENCES sessions(id) DEFERRABLE INITIALLY DEFERRED,
 run_id TEXT REFERENCES runs(id) DEFERRABLE INITIALLY DEFERRED,operation_id TEXT NOT NULL UNIQUE,
 container_id TEXT,image_digest TEXT NOT NULL,status TEXT NOT NULL,writer_revoked BOOLEAN NOT NULL,
 started INTEGER,finished INTEGER
);
PRAGMA user_version=15;

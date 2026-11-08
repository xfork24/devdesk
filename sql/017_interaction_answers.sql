ALTER TABLE turns ADD COLUMN cancel_requested BOOLEAN NOT NULL DEFAULT 0;
CREATE TABLE interactions_typed (
 id TEXT PRIMARY KEY,project_id TEXT NOT NULL,session_id TEXT NOT NULL,turn_id TEXT NOT NULL,
 external_request TEXT NOT NULL,kind TEXT NOT NULL,status TEXT NOT NULL,
 questions JSON NOT NULL CHECK(json_valid(questions)),tool TEXT,reason TEXT NOT NULL,
 answer JSON CHECK(answer IS NULL OR json_valid(answer)),revision INTEGER NOT NULL,
 UNIQUE(session_id,external_request),
 FOREIGN KEY(project_id,session_id,turn_id) REFERENCES turns(project_id,session_id,id) DEFERRABLE INITIALLY DEFERRED
);
INSERT INTO interactions_typed SELECT id,project_id,session_id,turn_id,external_request,kind,status,questions,tool,reason,
 CASE WHEN answer IS NULL THEN NULL ELSE json_object('kind','legacy','text',answer) END,revision FROM interactions;
DROP TABLE interactions;
ALTER TABLE interactions_typed RENAME TO interactions;
PRAGMA user_version=17;

-- Versioned configuration and durable workflow projections. Payloads are typed by kind.
CREATE TABLE governance_records (
 id TEXT PRIMARY KEY,
 kind TEXT NOT NULL,
 definition_id TEXT NOT NULL,
 scope JSON NOT NULL CHECK(json_valid(scope)),
 name TEXT NOT NULL,
 version INTEGER NOT NULL CHECK(version > 0),
 supersedes TEXT,
 content_hash TEXT NOT NULL,
 data JSON NOT NULL CHECK(json_valid(data)),
 immutable BOOLEAN NOT NULL,
 revision INTEGER NOT NULL CHECK(revision > 0),
 created_at INTEGER NOT NULL
);
CREATE INDEX idx_governance_kind ON governance_records(kind,definition_id,version);
CREATE INDEX idx_governance_scope ON governance_records(json_extract(scope,'$.project_id'),kind);
CREATE UNIQUE INDEX idx_governance_published_version ON governance_records(kind,definition_id,version) WHERE kind IN ('model','tool','harness','agent','workflow','standard','skill','spec','project_configuration');
CREATE UNIQUE INDEX idx_feature_configuration_lock ON governance_records(definition_id) WHERE kind='feature_lock';
CREATE TRIGGER governance_immutable_update BEFORE UPDATE ON governance_records WHEN OLD.immutable=1 BEGIN SELECT RAISE(ABORT,'immutable configuration'); END;
CREATE TRIGGER governance_no_delete BEFORE DELETE ON governance_records BEGIN SELECT RAISE(ABORT,'configuration history cannot be deleted'); END;
CREATE TRIGGER governance_identity_update BEFORE UPDATE ON governance_records WHEN NEW.id!=OLD.id OR NEW.kind!=OLD.kind OR NEW.definition_id!=OLD.definition_id OR NEW.scope!=OLD.scope BEGIN SELECT RAISE(ABORT,'configuration identity cannot change'); END;
CREATE TRIGGER agent_instance_binding_update BEFORE UPDATE ON governance_records WHEN OLD.kind='agent_instance' AND (json_extract(NEW.data,'$.agent_revision_id')!=json_extract(OLD.data,'$.agent_revision_id') OR json_extract(NEW.data,'$.scope')!=json_extract(OLD.data,'$.scope')) BEGIN SELECT RAISE(ABORT,'AgentID binding cannot change'); END;
PRAGMA user_version=34;

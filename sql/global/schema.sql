-- Current schema baseline. No legacy upgrade path is executed at startup.
BEGIN IMMEDIATE;
CREATE TABLE definitions (
    id TEXT PRIMARY KEY,
    family TEXT NOT NULL,
    name TEXT NOT NULL,
    version INTEGER NOT NULL,
    supersedes TEXT REFERENCES definitions(id) DEFERRABLE INITIALLY DEFERRED,
    body TEXT NOT NULL,
    slots JSON NOT NULL CHECK(json_valid(slots)),
    rules JSON NOT NULL CHECK(json_valid(rules))
, agent_profile JSON CHECK(agent_profile IS NULL OR json_valid(agent_profile)));

CREATE TABLE events (id INTEGER PRIMARY KEY, target TEXT NOT NULL, kind TEXT NOT NULL, detail TEXT NOT NULL, time INTEGER NOT NULL);

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

CREATE TABLE harnesses (id TEXT PRIMARY KEY, provider TEXT NOT NULL, version TEXT NOT NULL, capabilities JSON NOT NULL CHECK(json_valid(capabilities)), origin TEXT NOT NULL, permission JSON CHECK(permission IS NULL OR json_valid(permission)));

CREATE TABLE operations (id TEXT PRIMARY KEY, fingerprint TEXT NOT NULL, result JSON NOT NULL CHECK(json_valid(result)));

CREATE TABLE project_catalog (
    id TEXT PRIMARY KEY,
    command_id TEXT NOT NULL UNIQUE,
    fingerprint TEXT NOT NULL,
    project JSON CHECK(project IS NULL OR json_valid(project))
);

CREATE TABLE runtime_installations (
    id TEXT PRIMARY KEY,
    provider TEXT NOT NULL CHECK(provider IN ('codex','dsh')),
    version TEXT NOT NULL,
    os TEXT NOT NULL,
    arch TEXT NOT NULL,
    artifact JSON NOT NULL CHECK(json_valid(artifact)),
    source TEXT NOT NULL,
    license TEXT NOT NULL,
    UNIQUE(provider,version,os,arch)
);

CREATE TABLE sandboxes (id TEXT PRIMARY KEY, provider TEXT NOT NULL, version TEXT NOT NULL, platform TEXT NOT NULL, image TEXT, network BOOLEAN NOT NULL, origin TEXT NOT NULL, permission JSON CHECK(permission IS NULL OR json_valid(permission)));

CREATE TABLE settings (key TEXT PRIMARY KEY, value TEXT NOT NULL CHECK(json_valid(value)));

CREATE UNIQUE INDEX idx_delivery_type_revision ON governance_records(kind,definition_id,version)
 WHERE kind='delivery_type';

CREATE INDEX idx_governance_capability_reference ON governance_records(json_extract(data,'$.capability_ref')) WHERE kind='agent';

CREATE INDEX idx_governance_kind ON governance_records(kind,definition_id,version);

CREATE UNIQUE INDEX idx_governance_published_version ON governance_records(kind,definition_id,version) WHERE kind IN ('model','tool','harness','agent','workflow','standard','skill','spec');

CREATE UNIQUE INDEX idx_governance_resource_version ON governance_records(kind,definition_id,version) WHERE kind IN ('capability','role');

CREATE UNIQUE INDEX idx_integration_revision ON governance_records(kind,definition_id,version)
 WHERE kind IN ('integration','integration_binding','skill_package','integration_project_configuration');

CREATE UNIQUE INDEX idx_permission_resource_revision ON governance_records(kind,definition_id,version)
 WHERE kind IN ('sandbox','permission_profile','permission_binding','permission_assignment');

CREATE INDEX idx_permission_target ON governance_records(json_extract(data,'$.target.revision_id'),version)
 WHERE kind IN ('permission_binding','permission_assignment');

CREATE INDEX idx_tool_call_status ON governance_records(json_extract(data,'$.status'),created_at) WHERE kind='tool_call';

CREATE TRIGGER agent_instance_binding_update BEFORE UPDATE ON governance_records WHEN OLD.kind='agent_instance' AND (json_extract(NEW.data,'$.agent_revision_id')!=json_extract(OLD.data,'$.agent_revision_id') OR json_extract(NEW.data,'$.scope')!=json_extract(OLD.data,'$.scope')) BEGIN SELECT RAISE(ABORT,'AgentID binding cannot change'); END;

CREATE TRIGGER governance_identity_update BEFORE UPDATE ON governance_records WHEN NEW.id!=OLD.id OR NEW.kind!=OLD.kind OR NEW.definition_id!=OLD.definition_id OR NEW.scope!=OLD.scope BEGIN SELECT RAISE(ABORT,'configuration identity cannot change'); END;

CREATE TRIGGER governance_immutable_update BEFORE UPDATE ON governance_records WHEN OLD.immutable=1 BEGIN SELECT RAISE(ABORT,'immutable configuration'); END;

CREATE TRIGGER governance_no_delete BEFORE DELETE ON governance_records BEGIN SELECT RAISE(ABORT,'configuration history cannot be deleted'); END;
PRAGMA user_version=10;
COMMIT;

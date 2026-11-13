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


CREATE TABLE events (id INTEGER PRIMARY KEY, target TEXT NOT NULL, kind TEXT NOT NULL, detail TEXT NOT NULL, time INTEGER NOT NULL);

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


CREATE TABLE settings (key TEXT PRIMARY KEY, value TEXT NOT NULL CHECK(json_valid(value)));

CREATE UNIQUE INDEX idx_delivery_type_revision ON governance_records(type,definition_id,version)
 WHERE type='delivery_type';

CREATE INDEX idx_governance_capability_reference ON governance_records(json_extract(refs,'$.capability_ref')) WHERE type='agent';

CREATE INDEX idx_governance_type ON governance_records(type,definition_id,version);

CREATE UNIQUE INDEX idx_governance_published_version ON governance_records(type,definition_id,version) WHERE type IN ('model','tool','harness','agent','workflow','standard','skill','spec');

CREATE UNIQUE INDEX idx_governance_resource_version ON governance_records(type,definition_id,version) WHERE type IN ('capability','role');

CREATE UNIQUE INDEX idx_integration_revision ON governance_records(type,definition_id,version)
 WHERE type IN ('integration','integration_binding','skill_package','integration_project_configuration');

CREATE UNIQUE INDEX idx_permission_resource_revision ON governance_records(type,definition_id,version)
 WHERE type IN ('sandbox','permission_profile','permission_binding','permission_assignment');

CREATE INDEX idx_permission_target ON governance_records(json_extract(data,'$.target.revision_id'),version)
 WHERE type IN ('permission_binding','permission_assignment');

CREATE INDEX idx_tool_call_status ON governance_records(json_extract(data,'$.status'),created_at) WHERE type='tool_call';

CREATE TRIGGER agent_instance_binding_update BEFORE UPDATE ON governance_records WHEN OLD.type='agent_instance' AND (json_extract(NEW.data,'$.agent_revision_id')!=json_extract(OLD.data,'$.agent_revision_id') OR json_extract(NEW.data,'$.scope')!=json_extract(OLD.data,'$.scope')) BEGIN SELECT RAISE(ABORT,'AgentID binding cannot change'); END;

CREATE TRIGGER governance_identity_update BEFORE UPDATE ON governance_records WHEN NEW.id!=OLD.id OR NEW.type!=OLD.type OR NEW.definition_id!=OLD.definition_id OR NEW.scope!=OLD.scope BEGIN SELECT RAISE(ABORT,'configuration identity cannot change'); END;

CREATE TRIGGER governance_immutable_update BEFORE UPDATE ON governance_records WHEN OLD.immutable=1 BEGIN SELECT RAISE(ABORT,'immutable configuration'); END;

CREATE TRIGGER governance_no_delete BEFORE DELETE ON governance_records BEGIN SELECT RAISE(ABORT,'configuration history cannot be deleted'); END;

-- Catalog metadata uses the existing immutable record ledger and transaction boundary.
CREATE UNIQUE INDEX idx_template_catalog_singleton ON governance_records(type) WHERE type='template_catalog_state';
CREATE UNIQUE INDEX idx_template_catalog_member ON governance_records(definition_id,json_extract(data,'$.generation')) WHERE type='template_catalog_member';
CREATE INDEX idx_template_catalog_generation ON governance_records(json_extract(data,'$.generation')) WHERE type='template_catalog_member';

PRAGMA user_version=17;
COMMIT;

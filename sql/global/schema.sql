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


CREATE TABLE operations (id TEXT PRIMARY KEY, fingerprint TEXT NOT NULL, result JSON NOT NULL CHECK(json_valid(result)), versions JSON NOT NULL DEFAULT '[]' CHECK(json_valid(versions)), cursor INTEGER NOT NULL DEFAULT 0);
CREATE TABLE resource_versions(kind TEXT NOT NULL,resource_id TEXT NOT NULL,revision INTEGER NOT NULL CHECK(revision>=0),PRIMARY KEY(kind,resource_id));

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
    recipe_digest TEXT,
    UNIQUE(provider,version,os,arch,recipe_digest)
);


CREATE TABLE settings (key TEXT PRIMARY KEY, value TEXT NOT NULL CHECK(json_valid(value)));
CREATE UNIQUE INDEX idx_runtime_identity ON runtime_installations(provider,version,os,arch,coalesce(recipe_digest,''));

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

-- A database clock identifies committed catalog snapshots across pooled connections.
-- This is a read-model token, never an optimistic write precondition for a project.
CREATE TABLE catalog_clock(id INTEGER PRIMARY KEY CHECK(id=1), revision INTEGER NOT NULL CHECK(revision>0));
INSERT INTO catalog_clock VALUES(1,1);
CREATE TRIGGER catalog_source_insert AFTER INSERT ON source_definitions BEGIN UPDATE catalog_clock SET revision=revision+1 WHERE id=1; END;
CREATE TRIGGER catalog_governance_insert AFTER INSERT ON governance_records WHEN NEW.type IN ('project_configuration','work_configuration','integration_project_configuration','workflow_draft','agent_instance','agent_instance_retirement','skill_package','role_retirement','configuration_retirement','template_catalog_state','template_catalog_member','standard_archive','standard_derivation','integration_discovery','integration_disabled','integration_operation','harness_materialization','permission_origin','permission_file_origin','permission_source_selection','permission_binding','permission_assignment') BEGIN UPDATE catalog_clock SET revision=revision+1 WHERE id=1; END;
CREATE TRIGGER catalog_governance_update AFTER UPDATE ON governance_records WHEN NEW.type IN ('project_configuration','work_configuration','integration_project_configuration','workflow_draft','agent_instance','agent_instance_retirement','skill_package','role_retirement','configuration_retirement','template_catalog_state','template_catalog_member','standard_archive','standard_derivation','integration_discovery','integration_disabled','integration_operation','harness_materialization','permission_origin','permission_file_origin','permission_source_selection','permission_binding','permission_assignment') BEGIN UPDATE catalog_clock SET revision=revision+1 WHERE id=1; END;
CREATE TRIGGER catalog_governance_delete AFTER DELETE ON governance_records WHEN OLD.type IN ('project_configuration','work_configuration','integration_project_configuration','workflow_draft','agent_instance','agent_instance_retirement','skill_package','role_retirement','configuration_retirement','template_catalog_state','template_catalog_member','standard_archive','standard_derivation','integration_discovery','integration_disabled','integration_operation','harness_materialization','permission_origin','permission_file_origin','permission_source_selection','permission_binding','permission_assignment') BEGIN UPDATE catalog_clock SET revision=revision+1 WHERE id=1; END;


CREATE INDEX idx_runtime_work_seek ON governance_records(type,json_extract(data,'$.status'),id);
CREATE TRIGGER transfer_predicate_insert AFTER INSERT ON governance_records WHEN NEW.type='configuration_transfer' BEGIN
 INSERT INTO resource_versions(kind,resource_id,revision) VALUES('collection','configuration-transfers',1) ON CONFLICT(kind,resource_id) DO UPDATE SET revision=revision+1;
END;
CREATE TRIGGER transfer_predicate_update AFTER UPDATE ON governance_records WHEN NEW.type='configuration_transfer' BEGIN
 INSERT INTO resource_versions(kind,resource_id,revision) VALUES('collection','configuration-transfers',1) ON CONFLICT(kind,resource_id) DO UPDATE SET revision=revision+1;
END;
CREATE TRIGGER transfer_no_delete BEFORE DELETE ON governance_records WHEN OLD.type='configuration_transfer' BEGIN SELECT RAISE(ABORT,'configuration transfer cannot be deleted'); END;


-- The driver seeks active IDs without inspecting settled tool history.
CREATE INDEX idx_tool_call_drivable ON governance_records(id) WHERE type='tool_call' AND (json_extract(data,'$.status') IN ('prepared','dispatched','needs_reconciliation') OR (json_extract(data,'$.status') IN ('succeeded','failed','cancelled') AND coalesce(json_extract(data,'$.reply_sent'),0)=0 AND json_extract(data,'$.invocation.session_id') IS NOT NULL));

CREATE TABLE dataset_layout(id INTEGER PRIMARY KEY CHECK(id=1),contract TEXT NOT NULL);
INSERT INTO dataset_layout VALUES(1,'single-host-resource-v2');

PRAGMA user_version=24;
COMMIT;

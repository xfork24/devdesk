BEGIN IMMEDIATE;
CREATE UNIQUE INDEX idx_template_catalog_singleton ON governance_records(kind) WHERE kind='template_catalog_state';
CREATE UNIQUE INDEX idx_template_catalog_member ON governance_records(definition_id) WHERE kind='template_catalog_member';
CREATE INDEX idx_template_catalog_generation ON governance_records(json_extract(data,'$.generation')) WHERE kind='template_catalog_member';
PRAGMA user_version=42;
COMMIT;

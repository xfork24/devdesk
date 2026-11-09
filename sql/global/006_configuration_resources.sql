-- Independently versioned roles/capabilities and resumable conversion receipts.
CREATE UNIQUE INDEX idx_governance_resource_version ON governance_records(kind,definition_id,version) WHERE kind IN ('capability','role');
CREATE INDEX idx_governance_capability_reference ON governance_records(json_extract(data,'$.capability_ref')) WHERE kind='agent';
CREATE INDEX idx_governance_conversion_source ON governance_records(definition_id) WHERE kind IN ('agent_capability','configuration_conversion');
PRAGMA user_version=6;

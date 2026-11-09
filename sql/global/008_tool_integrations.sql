CREATE UNIQUE INDEX idx_integration_revision ON governance_records(kind,definition_id,version)
 WHERE kind IN ('integration','integration_binding','skill_package','integration_project_configuration');
CREATE INDEX idx_tool_call_status ON governance_records(json_extract(data,'$.status'),created_at) WHERE kind='tool_call';
PRAGMA user_version=8;

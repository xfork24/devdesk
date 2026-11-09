CREATE UNIQUE INDEX idx_integration_revision ON governance_records(kind,definition_id,version)
 WHERE kind IN ('integration','integration_binding','skill_package','integration_project_configuration');
CREATE INDEX idx_tool_call_status ON governance_records(json_extract(data,'$.status'),created_at) WHERE kind='tool_call';
CREATE INDEX idx_tool_call_session ON governance_records(json_extract(data,'$.invocation.session_id'),json_extract(data,'$.invocation.turn_id')) WHERE kind='tool_call';
PRAGMA user_version=37;

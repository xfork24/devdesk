CREATE UNIQUE INDEX idx_permission_resource_revision ON governance_records(kind,definition_id,version)
 WHERE kind IN ('sandbox','permission_profile','permission_binding','permission_assignment');
CREATE INDEX idx_permission_target ON governance_records(json_extract(data,'$.target.revision_id'),version)
 WHERE kind IN ('permission_binding','permission_assignment');
PRAGMA user_version=7;

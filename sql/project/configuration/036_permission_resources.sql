CREATE UNIQUE INDEX idx_permission_resource_revision ON governance_records(kind,definition_id,version)
 WHERE kind IN ('sandbox','permission_profile','permission_binding','permission_assignment');
CREATE INDEX idx_permission_switch ON governance_records(definition_id) WHERE kind='permission_switch';
CREATE UNIQUE INDEX idx_feature_permission_lock ON governance_records(definition_id) WHERE kind='feature_permission_lock';
PRAGMA user_version=36;

CREATE UNIQUE INDEX idx_execution_task ON governance_records(kind,definition_id)
 WHERE kind IN ('execution_task','execution_task_binding');
CREATE INDEX idx_task_step ON governance_records(json_extract(data,'$.step_instance_id'))
 WHERE kind='execution_task_binding';
PRAGMA user_version=37;

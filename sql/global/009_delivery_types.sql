CREATE UNIQUE INDEX idx_delivery_type_revision ON governance_records(kind,definition_id,version)
 WHERE kind='delivery_type';
PRAGMA user_version=9;

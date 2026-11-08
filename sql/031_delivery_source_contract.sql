ALTER TABLE deliveries ADD COLUMN source_deliveries JSON NOT NULL DEFAULT '[]';
-- This relation already exists explicitly in Merge; copy it without inventing
-- historical Feature closure inputs or rewriting any sealed Artifact.
UPDATE deliveries
SET source_deliveries = json_array((SELECT source_delivery FROM merges WHERE merges.id=deliveries.merge_id))
WHERE merge_id IS NOT NULL AND EXISTS(SELECT 1 FROM merges WHERE merges.id=deliveries.merge_id);
PRAGMA user_version=31;

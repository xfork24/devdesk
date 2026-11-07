-- Legacy inputs remain frozen; unknown generated-output provenance stays empty.
ALTER TABLE attempts ADD COLUMN outputs JSON NOT NULL DEFAULT '[]' CHECK(json_valid(outputs));
PRAGMA user_version=8;

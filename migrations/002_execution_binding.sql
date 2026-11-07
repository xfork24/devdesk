-- Preserve original reviewer identity; do not infer it from the author on retry.
ALTER TABLE runs ADD COLUMN binding JSON NOT NULL DEFAULT '{}' CHECK(json_valid(binding));
UPDATE runs SET binding = json_set(
 COALESCE((SELECT a.binding FROM attempts a WHERE a.id=runs.attempt_id),
          (SELECT f.binding FROM merges m JOIN features f ON f.id=m.feature_id WHERE m.id=runs.merge_id)),
 '$.agent',(SELECT agent FROM sessions WHERE id=runs.session_id),
 '$.harness',(SELECT harness FROM sessions WHERE id=runs.session_id),
 '$.sandbox',(SELECT sandbox FROM sessions WHERE id=runs.session_id));
PRAGMA user_version=2;

ALTER TABLE runtime_installations RENAME TO runtime_installations_v24;

CREATE TABLE runtime_installations (
    id TEXT PRIMARY KEY,
    provider TEXT NOT NULL CHECK(provider IN ('codex','dsh','pi','mcode','zcode','opencode')),
    version TEXT NOT NULL,
    os TEXT NOT NULL,
    arch TEXT NOT NULL,
    artifact JSON NOT NULL CHECK(json_valid(artifact)),
    source TEXT NOT NULL,
    license TEXT NOT NULL,
    recipe_digest TEXT,
    UNIQUE(provider,version,os,arch,recipe_digest)
);

INSERT INTO runtime_installations (id, provider, version, os, arch, artifact, source, license, recipe_digest)
SELECT id, provider, version, os, arch, artifact, source, license, recipe_digest FROM runtime_installations_v24;
DROP TABLE runtime_installations_v24;
CREATE UNIQUE INDEX idx_runtime_identity ON runtime_installations(provider,version,os,arch,coalesce(recipe_digest,''));

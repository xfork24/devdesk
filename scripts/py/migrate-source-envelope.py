#!/usr/bin/env python3
"""One-time migration of resources/configuration to the flattened envelope:
kind -> type, scope object -> scope string, whitelisted data refs hoisted to root."""
import json
import sys
from pathlib import Path

ROOT = Path(__file__).resolve().parents[2] / "resources" / "configuration"

REFERENCE_FIELDS = {
    "capability": ["harness_ref", "model_ref", "skills", "tools"],
    "agent": ["role_ref", "capability_ref"],
    "delivery_type": ["spec_refs"],
    "harness": ["model_providers", "model_catalog_ref"],
    "permission_context": ["sandbox_ref"],
    "standard": ["root_workflow_refs"],
}


def migrate(path: Path) -> bool:
    source = json.loads(path.read_text())
    if "type" in source and "kind" not in source:
        return False
    definition_type = source.pop("kind")
    scope = source.pop("scope")
    if isinstance(scope, dict):
        assert scope == {"kind": "global"}, f"{path}: non-global scope {scope}"
        scope = "global"
    data = source.pop("data")
    hoisted = {}
    for field in REFERENCE_FIELDS.get(definition_type, []):
        if field in data:
            hoisted[field] = data.pop(field)
    leftover = set(data) & set(
        f for fields in REFERENCE_FIELDS.values() for f in fields
    )
    assert not leftover, f"{path}: unhoisted reference fields {leftover}"
    migrated = {"type": definition_type}
    migrated.update(source)  # identity, revision, name
    migrated["scope"] = scope
    migrated.update(sorted(hoisted.items()))
    migrated["data"] = data
    path.write_text(json.dumps(migrated, ensure_ascii=False, indent=2) + "\n")
    return True


def main() -> None:
    manifest = json.loads((ROOT / "manifest.json").read_text())
    changed = 0
    for paths in manifest["types"].values():
        for rel in paths:
            if migrate(ROOT / rel):
                changed += 1
    print(f"migrated {changed} source files")


if __name__ == "__main__":
    main()

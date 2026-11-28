#!/usr/bin/env python3
"""Check ownership and reference closure for the core and software catalogs."""
import json
import sys
from pathlib import Path

sys.path.insert(0, str(Path(__file__).resolve().parent))
from configuration_usage import usage_index

ROOT = Path(__file__).resolve().parents[2] / "plugins"
REFERENCE_FIELDS = (
    "capability_ref",
    "role_ref",
    "model_ref",
    "harness_ref",
    "sandbox_ref",
    "model_catalog_ref",
    "spec_refs",
    "skills",
    "tools",
    "model_providers",
    "root_workflow_refs",
)


def check_catalog(directory, dependencies, domain=None):
    manifest = json.loads((directory / "manifest.json").read_text())
    owned = {"manifest.json", manifest["defaults"]}
    by_identity = set()
    sources = []
    for kind, paths in manifest["types"].items():
        for relative in paths:
            path = directory / relative
            source = json.loads(path.read_text())
            identity = source["identity"]
            if source["type"] != kind or source["scope"] != "global" or identity in by_identity:
                raise ValueError(f"{path}: unexpected type, scope or duplicate identity")
            if domain and not identity.startswith(f"{domain}."):
                raise ValueError(f"{path}: source identity is outside {domain}")
            if "delivery_policy" in source["data"]:
                raise ValueError(f"{path}: use explicit operation_evidence")
            by_identity.add(identity)
            owned.add(relative)
            sources.append(source)
            for key, value in source["data"].items():
                if key.endswith("_file"):
                    body = path.parent / value
                    body.read_text()
                    owned.add(body.relative_to(directory).as_posix())

    references = dependencies | {f"{s['identity']}:{s['revision']}" for s in sources}

    def resolve(reference, owner):
        if reference not in references:
            raise ValueError(f"{owner} references unpublished revision {reference!r}")

    for source in sources:
        for field in REFERENCE_FIELDS:
            value = source.get(field, [])
            for reference in [value] if isinstance(value, str) else value:
                resolve(reference, source["identity"])

    defaults = json.loads((directory / manifest["defaults"]).read_text())
    for key, value in defaults.items():
        if key.endswith("_ref"):
            resolve(value, key)
    for role, agent in defaults.get("role_agent_bindings", {}).items():
        resolve(role, "default role")
        resolve(agent, f"default agent for {role}")

    if index := manifest.get("usage_index"):
        owned.add(index)
        if json.loads((directory / index).read_text()) != {
            "schema_version": 1, "references": usage_index(sources),
        }:
            raise ValueError("core usage index is stale; run python3 scripts/check/configuration_usage.py --write")
    orphans = sorted(path.relative_to(directory).as_posix() for path in directory.rglob("*")
                     if path.is_file() and path.relative_to(directory).as_posix() not in owned)
    if orphans:
        raise ValueError(f"{directory}: unowned files: {orphans}")

    print(f"check-defaults: {manifest['bundle_version']} — {len(sources)} sources")
    return references


if __name__ == "__main__":
    try:
        core = check_catalog(ROOT.parent / "kernel/core", set())
        check_catalog(ROOT / "software-development/catalog", core, "software-development")
    except (OSError, KeyError, ValueError) as error:
        raise SystemExit(f"check-defaults: {error}") from error

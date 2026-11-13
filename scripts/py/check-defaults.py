#!/usr/bin/env python3
"""Factory configuration self-consistency gate.

Replaces Rust-side content-hash constants: the published bundle is verified against
the directory itself (manifest ↔ files ↔ defaults references), so factory edits no
longer require recompiling tests. Structural validation and cross-reference checks
remain in `cargo test` via build.rs and materialize().
"""
import json
import sys
from pathlib import Path

ROOT = Path(__file__).resolve().parents[2] / "resources" / "configuration"

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


def fail(message: str) -> None:
    print(f"check-defaults: {message}", file=sys.stderr)
    sys.exit(1)


def main() -> None:
    # Manifest structure itself is the gate (the Rust Manifest struct enforces the
    # shape at load time); schema_version is recorded metadata, never a version check.
    manifest = json.loads((ROOT / "manifest.json").read_text())

    published: dict[str, dict] = {}  # (type, identity) -> {revision, path}
    sources: dict[str, dict] = {}  # rel -> parsed source
    md_bodies: set[Path] = set()
    for kind, paths in sorted(manifest.get("types", {}).items()):
        for rel in paths:
            path = ROOT / rel
            if not path.is_file():
                fail(f"manifest lists missing file: {rel}")
            try:
                source = json.loads(path.read_text())
            except json.JSONDecodeError as e:
                fail(f"{rel}: invalid JSON: {e}")
            for field in ("type", "identity", "revision", "name", "scope", "data"):
                if field not in source:
                    fail(f"{rel}: missing envelope field {field!r}")
            if source["type"] != kind:
                fail(f"{rel}: type {source['type']!r} does not match manifest type {kind!r}")
            if source["scope"] != "global":
                fail(f"{rel}: factory sources must use the global scope")
            key = (kind, source["identity"])
            if key in published:
                fail(
                    f"identity {source['identity']!r} published twice: "
                    f"{published[key]} and {rel}"
                )
            published[key] = {"revision": source["revision"], "path": rel}
            sources[rel] = source
            for field, value in source["data"].items():
                if field.endswith("_file"):
                    body = path.parent / value
                    if not body.is_file():
                        fail(f"{rel}: body file {value!r} missing")
                    md_bodies.add(body)

    # Every file in the tree must be owned: markdown by a published body_file, JSON by
    # the manifest or the documented root meta files. Anything else is dead weight that
    # build.rs would still embed into the binary.
    root_meta = {
        "manifest.json",
        "defaults.json",
        "README.md",
    }
    listed = {
        rel for paths in manifest.get("types", {}).values() for rel in paths
    }
    orphans = sorted(
        str(p.relative_to(ROOT))
        for p in ROOT.rglob("*")
        if p.is_file()
        and str(p.relative_to(ROOT)) not in root_meta
        and str(p.relative_to(ROOT)) not in listed
        and p.suffix in (".json", ".md")
        and p not in md_bodies
    )
    if orphans:
        fail(f"files not owned by any published definition or root meta: {orphans}")

    def resolve(ref: str, what: str) -> None:
        identity, _, revision = ref.rpartition(":")
        for (_, ident), meta in published.items():
            if ident == identity and str(meta["revision"]) == revision:
                return
        fail(f"{what} references unpublished revision {ref!r}")

    # Flattened root references must point at published revisions.
    for rel, source in sources.items():
        for field in REFERENCE_FIELDS:
            value = source.get(field)
            if isinstance(value, str):
                resolve(value, f"{rel} {field}")
            elif isinstance(value, list):
                for item in value:
                    resolve(item, f"{rel} {field}")

    defaults = json.loads((ROOT / manifest["defaults"]).read_text())
    scalar_refs = [
        "workflow_ref",
        "task_workflow_ref",
        "model_ref",
        "harness_ref",
        "sandbox_ref",
        "capability_ref",
        "standard_ref",
        "permission_context_ref",
        "orchestrator_role_ref",
        "harness_permission_ref",
        "sandbox_permission_ref",
    ]
    for key in scalar_refs:
        value = defaults.get(key)
        if isinstance(value, str):
            resolve(value, key)
    for role, agent in defaults.get("role_agent_bindings", {}).items():
        resolve(role, f"role_agent_bindings key {role}")
        resolve(agent, f"role_agent_bindings[{role}]")

    kinds = {kind: len(paths) for kind, paths in manifest.get("types", {}).items()}
    print(
        "check-defaults: OK — "
        + ", ".join(f"{kind}={count}" for kind, count in sorted(kinds.items()))
        + f"; bundle_version={manifest.get('bundle_version')}"
    )


if __name__ == "__main__":
    main()

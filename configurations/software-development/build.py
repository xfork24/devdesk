"""Build the software-development domain release candidate."""
import argparse
import hashlib
import json
from pathlib import Path
import runpy
from zipfile import ZIP_DEFLATED, ZipFile, ZipInfo

PACKAGE = Path(__file__).resolve().parent
REPO = PACKAGE.parents[1]
DOMAIN = "software-development"
DISCOVER = f"{DOMAIN}.tests.discover"
BRANCH = f"{DOMAIN}.branch.plan"


def source(kind, key, data, **references):
    return {"type": kind, "identity": f"{DOMAIN}.{key}", "revision": 1,
            "scope": "global", "name": key.replace("-", " ").title(), "data": data, **references}


def build(target):
    coordinate = {
        "type": "object", "additionalProperties": False, "required": ["path", "revision"],
        "properties": {"path": {"type": "string", "minLength": 1, "maxLength": 4096},
                       "revision": {"type": "string", "pattern": "^([0-9a-f]{40}|[0-9a-f]{64})$"}},
    }
    rpc = json.loads((REPO / "schemas/rpc.schema.json").read_text())["$defs"]
    # The platform validator uses a linear-time regex engine.
    rpc["ProjectTestRecipe"]["properties"]["files"]["items"] = {
        "type": "string", "pattern": r"^[^/]+(?:/[^/]+)*$",
        "not": {"pattern": r"(^[-/]|[\\\r\n\u0000]|(?:^|/)\.{1,2}(?:/|$))"},
    }
    output = {
        "type": "object", "additionalProperties": False,
        "required": ["revision", "source", "files", "notes", "configuration"],
        "properties": {
            "revision": coordinate["properties"]["revision"],
            "source": {"enum": ["repository", "unconfigured"]},
            "files": {"type": "array", "maxItems": 100, "items": {"type": "string"}},
            "notes": {"type": "array", "items": {"type": "string"}},
            "configuration": {"$ref": "#/$defs/ProjectTestConfiguration"},
        },
        "$defs": {key: rpc[key] for key in ("ProjectTestConfiguration", "ProjectTestRecipe", "TestCommand")},
    }
    branch_request = {
        "type": "object", "additionalProperties": False, "required": ["kind", "owner"],
        "properties": {
            "kind": {"enum": ["feature", "task", "candidate"]},
            "owner": {"type": "string", "pattern": "^[0-9a-f]{8}(-[0-9a-f]{4}){3}-[0-9a-f]{12}$"},
        },
    }
    branch_result = {**branch_request, "required": ["kind", "owner", "branch"],
                     "properties": {**branch_request["properties"], "branch": {"type": "string"}}}
    workflow = f"{DOMAIN}.repository-preflight:1"
    tool = f"{DOMAIN}.detect-test-entry:1"
    result_type = f"{DOMAIN}.test-configuration:1"
    entries = [
        source("spec", "preflight-contract", {"schema_version": 3,
               "body": "Identify a test command from tracked files at the exact commit. Runtime selection owns the image.",
               "checks": [{"key": "revision", "name": "Revision",
                           "instructions": "Preserve the exact selected Git commit."}]}),
        source("delivery_type", "repository-revision", {"format": "json", "content_schema": coordinate},
               spec_refs=[f"{DOMAIN}.preflight-contract:1"]),
        source("delivery_type", "test-configuration", {
            "format": "json", "content_schema": output,
            "producer": {"kind": "platform", "tools": ["domain.read"]},
        }, spec_refs=[f"{DOMAIN}.preflight-contract:1"]),
        source("tool", "detect-test-entry", {
            "qualified_name": "domain.read", "implementation": "platform",
            "input_schema": {"type": "object", "required": ["handler", "input"]},
            "output_schema": output,
        }),
        source("workflow", "repository-preflight", {
            "schema_version": 6, "stages": [{"key": "prepare", "name": "Prepare"}],
            "inputs": {"repository": {"delivery_ref": f"{DOMAIN}.repository-revision:1"}},
            "steps": [{"key": "discover", "name": "Discover tests", "stage": "prepare",
                       "inputs": {"repository": {"type": "source", "source": {"step": "$input", "port": "repository"}}},
                       "executor": {"type": "tool", "tool_ref": tool, "arguments": {
                           "handler": {"type": "constant", "value": DISCOVER},
                           "input": {"type": "source", "source": {"step": "$input", "port": "repository"}},
                       }}, "outputs": {"test_configuration": {"delivery_ref": result_type}}}],
            "outputs": {"test_configuration": {"step": "discover", "port": "test_configuration"}},
        }),
    ]

    types = {}
    files = {"runtime/worker.mjs": (PACKAGE / "runtime/worker.mjs").read_bytes()}
    for record in entries:
        filename = f"{record['identity']}.json"
        types.setdefault(record["type"], []).append(filename)
        files[f"configurations/{filename}"] = json.dumps(record).encode()
    usage = runpy.run_path(str(REPO / "scripts/py/configuration_usage.py"))["usage_index"]
    entity_types = "configurations/entity-types/repository.json"
    files.update({
        "configurations/manifest.json": json.dumps({
            "schema_version": 1, "bundle_version": f"{DOMAIN}-1", "types": types,
            "defaults": "defaults.json", "usage_index": "usage.json",
        }).encode(),
        "configurations/defaults.json": b"{}",
        "configurations/usage.json": json.dumps({"references": usage(entries)}).encode(),
        entity_types: json.dumps({
            f"{DOMAIN}.repository:1": {"name": "Repository revision", "schema": coordinate,
                  "workflows": {workflow: {"input": "repository", "outputs": {"test_configuration": result_type}}}},
            result_type: {"name": "Test configuration", "schema": output},
        }).encode(),
    })
    manifest = {"format": 1, "id": DOMAIN, "version": "0.1.0", "entity_types": entity_types,
                "files": {path: hashlib.sha256(contents).hexdigest() for path, contents in files.items()},
                "runtime": {"entry": "runtime/worker.mjs", "worktree_branch": BRANCH, "test_discovery": DISCOVER,
                            "handlers": {
                    BRANCH: {"input_schema": branch_request, "output_schema": branch_result},
                    DISCOVER: {"core": "git_snapshot", "input_schema": coordinate, "output_schema": output,
                               "git_snapshot": {
                                   "files": [".devdesk/tests.json", "package.json", "pyproject.toml", "pytest.ini",
                                             "setup.cfg", "Cargo.toml", "go.mod"],
                                   "searches": {"rust_tests": {"glob": "*.rs",
                                      "needles": ["#[test", "#[tokio::test", "#[async_std::test"]}},
                               }},
                }}}
    files["manifest.json"] = json.dumps(manifest).encode()
    target.parent.mkdir(parents=True, exist_ok=True)
    with ZipFile(target, "w") as archive:
        for name, data in sorted(files.items()):
            archive.writestr(ZipInfo(name), data, compress_type=ZIP_DEFLATED)


if __name__ == "__main__":
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("target", type=Path)
    build(parser.parse_args().target)

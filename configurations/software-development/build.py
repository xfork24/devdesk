"""Build the software-development domain release ZIP."""
import argparse
import hashlib
import json
from pathlib import Path
import runpy
from zipfile import ZIP_DEFLATED, ZipFile, ZipInfo

PACKAGE = Path(__file__).resolve().parent
REPO = PACKAGE.parents[1]
DOMAIN = "software-development"
VERSION = "0.4.0"
DISCOVER = f"{DOMAIN}.tests.discover"
BRANCH = f"{DOMAIN}.branch.plan"
CONNECT = f"{DOMAIN}.project.connect"
PR_CREATE = f"{DOMAIN}.pull-request.create"


def source(kind, key, data, **references):
    return {"type": kind, "identity": f"{DOMAIN}.{key}", "revision": 1,
            "scope": "global", "name": key.replace("-", " ").title(), "data": data, **references}


def delivery_configuration():
    configurations = REPO / "configurations"
    manifest = json.loads((configurations / "manifest.json").read_text())
    core = json.loads((configurations / "core/manifest.json").read_text())
    core_paths = {path for paths in core["types"].values() for path in paths}
    entries = []
    for path in (path for paths in manifest["types"].values() for path in paths if path not in core_paths):
        source_path = configurations / path
        record = json.loads(source_path.read_text())
        for key in tuple(record["data"]):
            if key.endswith("_file"):
                asset = source_path.parent / record["data"].pop(key)
                record["data"][key.removesuffix("_file")] = asset.read_text()
        entries.append(record)
    refs = {}
    for record in entries:
        identity = record["identity"]
        translated = f"{DOMAIN}.{identity.removeprefix('builtin-')}"
        refs[identity] = translated
        refs[f"{identity}:{record['revision']}"] = f"{translated}:{record['revision']}"

    def translate(value):
        if isinstance(value, str):
            return refs.get(value, value)
        if isinstance(value, dict):
            return {refs.get(key, key): translate(child) for key, child in value.items()}
        if isinstance(value, list):
            return [translate(child) for child in value]
        return value

    defaults = json.loads((configurations / manifest["defaults"]).read_text())
    return translate(entries), translate(defaults)


def build(target, web, version):
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

    delivery_sources, defaults = delivery_configuration()
    entries.extend(delivery_sources)
    connection_input = {
        "type": "object", "additionalProperties": False, "required": ["repository"],
        "properties": {"repository": {"type": "string", "minLength": 1, "maxLength": 300},
                       "mainline": {"type": "string", "minLength": 1, "maxLength": 200}},
    }
    connection_result = {
        **connection_input, "required": ["repository", "mainline"],
        "properties": {**connection_input["properties"],
                       "repository": {"type": "string", "pattern": r"^[A-Za-z0-9_.-]+/[A-Za-z0-9_.-]+$"}},
    }
    pr_request = {
        "type": "object", "additionalProperties": False, "required": ["request", "marker"],
        "properties": {
            "marker": {"type": "string", "minLength": 1, "maxLength": 256},
            "request": {"type": "object",
                        "required": ["merge", "head_branch", "base_branch", "candidate", "expected_target"],
                        "properties": {key: {"type": "string", "minLength": 1}
                                       for key in ("merge", "head_branch", "base_branch", "candidate", "expected_target")}},
        },
    }
    pr_result = {"type": "object", "additionalProperties": False,
                 "required": ["head", "base", "title", "body"],
                 "properties": {key: {"type": "string", "minLength": 1}
                                for key in ("head", "base", "title", "body")}}
    types = {}
    files = {"runtime/worker.mjs": (PACKAGE / "runtime/worker.mjs").read_bytes()}
    reporters = {"node_test": "runtime/node-reporter.mjs",
                 "python_unittest": "runtime/project-tests.py", "command": "runtime/project-tests.py"}
    for path in set(reporters.values()):
        files[path] = (PACKAGE / path).read_bytes()
    for record in entries:
        filename = f"{record['identity']}-v{record['revision']}.json"
        types.setdefault(record["type"], []).append(filename)
        files[f"configurations/{filename}"] = json.dumps(record).encode()
    usage = runpy.run_path(str(REPO / "scripts/py/configuration_usage.py"))["usage_index"]
    entity_types = "configurations/entity-types/repository.json"
    files.update({
        "configurations/manifest.json": json.dumps({
            "schema_version": 1, "bundle_version": f"{DOMAIN}-1", "types": types,
            "defaults": "defaults.json", "usage_index": "usage.json",
        }).encode(),
        "configurations/defaults.json": json.dumps(defaults).encode(),
        "configurations/usage.json": json.dumps({"references": usage(entries)}).encode(),
        entity_types: json.dumps({
            f"{DOMAIN}.repository:1": {"name": "Repository revision", "schema": coordinate,
                  "workflows": {workflow: {"input": "repository", "outputs": {"test_configuration": result_type}}}},
            result_type: {"name": "Test configuration", "schema": output},
        }).encode(),
    })
    ui = None
    if web is not None:
        if not (web / "workbench.mjs").is_file():
            raise ValueError(f"Missing Web entry in {web}; run npm --prefix web run build:software")
        contributions = PACKAGE / "web/contributions.json"
        files["ui/contributions.json"] = contributions.read_bytes()
        stylesheet = "ui/web/workbench.css"
        files[stylesheet] = (PACKAGE / "web/workbench.css").read_bytes()
        for asset in web.rglob("*.mjs"):
            files[f"ui/web/{asset.relative_to(web).as_posix()}"] = asset.read_bytes()
        ui = {
            "contributions": "ui/contributions.json",
            "targets": [{"renderer": "web.react", "api": 2, "styles": [stylesheet], "bindings": [
                {"contribution": row["id"], "entry": "ui/web/workbench.mjs", "export": export}
                for row, export in zip(json.loads(contributions.read_text()),
                                       ("BranchQueue", "DeliveryWorkbench", "EngineeringSettings"), strict=True)
            ]}],
        }
    manifest = {"format": 1, "id": DOMAIN, "version": version, "entity_types": entity_types,
                "files": {path: hashlib.sha256(contents).hexdigest() for path, contents in files.items()},
                "runtime": {"entry": "runtime/worker.mjs", "worktree_branch": BRANCH, "test_discovery": DISCOVER,
                            "project_connection": CONNECT, "test_reporters": reporters,
                            "pull_request_creation": PR_CREATE, "handlers": {
                    PR_CREATE: {"input_schema": pr_request, "output_schema": pr_result},
                    CONNECT: {"input_schema": connection_input, "output_schema": connection_result},
                    BRANCH: {"input_schema": branch_request, "output_schema": branch_result},
                    DISCOVER: {"core": "git_snapshot", "input_schema": coordinate, "output_schema": output,
                               "git_snapshot": {
                                   "files": [".devdesk/tests.json", "package.json", "pyproject.toml", "pytest.ini",
                                             "setup.cfg", "Cargo.toml", "go.mod"],
                                   "searches": {"rust_tests": {"glob": "*.rs",
                                      "needles": ["#[test", "#[tokio::test", "#[async_std::test"]}},
                               }},
                }}}
    if ui:
        manifest["ui"] = ui
    files["manifest.json"] = json.dumps(manifest).encode()
    target.parent.mkdir(parents=True, exist_ok=True)
    with ZipFile(target, "w") as archive:
        for name, data in sorted(files.items()):
            archive.writestr(ZipInfo(name), data, compress_type=ZIP_DEFLATED)


if __name__ == "__main__":
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--version", action="version", version=VERSION)
    parser.add_argument("target", type=Path)
    parser.add_argument("--web-dir", type=Path, default=REPO / "web/dist/software")
    parser.add_argument("--legacy-runtime-only", action="store_true",
                        help="Reproduce the 0.3.0 runtime-only release for compatibility tests")
    args = parser.parse_args()
    build(args.target, None if args.legacy_runtime_only else args.web_dir,
          "0.3.0" if args.legacy_runtime_only else VERSION)

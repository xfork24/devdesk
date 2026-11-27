"""Build the software-development domain release ZIP."""
import argparse
import hashlib
import json
from pathlib import Path
import runpy
import tomllib
from zipfile import ZIP_DEFLATED, ZipFile, ZipInfo

PACKAGE = Path(__file__).resolve().parent
REPO = PACKAGE.parents[1]
DOMAIN = "software-development"
VERSION = "0.6.0"
DISCOVER = f"{DOMAIN}.tests.discover"
BRANCH = f"{DOMAIN}.branch.plan"
CONNECT = f"{DOMAIN}.project.connect"
PR_CREATE = f"{DOMAIN}.pull-request.create"
AUTHORIZE = f"{DOMAIN}.delivery.authorize"
# The lowest Host this package runs on is the one built from the same checkout.
HOST_VERSION = tomllib.loads((REPO / "Cargo.toml").read_text())["workspace"]["package"]["version"]
# The pages use Web Core's component kit, so they name the Web Core release they were built against.
WEB_SDK_MAJOR, KIT_RELEASE = 1, ".".join(
    json.loads((REPO / "web/package.json").read_text())["version"].split(".")[:2])
# What the package uses beyond the kernel-wide surface. The kernel checks at install that
# every engineering handler, kernel input and hook the package touches belongs to a declared group.
EXTENSION_GROUPS = ["engineering.git/1", "engineering.tests/1"]


def platform_handlers(entries):
    """Platform tool handlers the package calls through its own or the kernel's tool definitions."""
    tools = {f"{record['identity']}:{record['revision']}": record["data"]
             for record in entries if record["type"] == "tool"}
    kernel = json.loads((REPO / "plugins/core/manifest.json").read_text())
    for path in kernel["types"]["tool"]:
        record = json.loads((REPO / "plugins/core" / path).read_text())
        tools[f"{record['identity']}:{record['revision']}"] = record["data"]

    def visit(value):
        if isinstance(value, dict):
            for key, item in value.items():
                if key == "tool_ref" and isinstance(item, str):
                    used.add(item)
                elif key in ("tools", "tool_refs") and isinstance(item, list):
                    used.update(ref for ref in item if isinstance(ref, str))
                else:
                    visit(item)
        elif isinstance(value, list):
            for item in value:
                visit(item)

    used = set()
    for record in entries:
        visit(record)
    return sorted(tools[ref]["qualified_name"] for ref in used
                  if tools.get(ref, {}).get("implementation") == "platform")


def source(kind, key, data, **references):
    return {"type": kind, "identity": f"{DOMAIN}.{key}", "revision": 1,
            "scope": "global", "name": key.replace("-", " ").title(), "data": data, **references}


def load_catalog():
    catalog = PACKAGE / "catalog"
    manifest = json.loads((catalog / "manifest.json").read_text())
    entries = []
    for paths in manifest["types"].values():
        for path in paths:
            source_path = catalog / path
            record = json.loads(source_path.read_text())
            data = record["data"]
            for key in tuple(data):
                if key.endswith("_file"):
                    asset = source_path.parent / data.pop(key)
                    data[key.removesuffix("_file")] = asset.read_text()
            entries.append(record)
    return entries, json.loads((catalog / manifest["defaults"]).read_text()), manifest["bundle_version"]


def build(target, web):
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
            "producer": {"kind": "platform", "tools": [f"{DOMAIN}.detect-test-entry"]},
        }, spec_refs=[f"{DOMAIN}.preflight-contract:1"]),
        # A tool of this package: it names one of its own handlers and takes the handler's input.
        source("tool", "detect-test-entry", {
            "qualified_name": f"{DOMAIN}.detect-test-entry", "implementation": "domain",
            "handler": DISCOVER, "input_schema": coordinate, "output_schema": output,
        }),
        source("workflow", "repository-preflight", {
            "schema_version": 6, "stages": [{"key": "prepare", "name": "Prepare"}],
            "inputs": {"repository": {"delivery_ref": f"{DOMAIN}.repository-revision:1"}},
            "steps": [{"key": "discover", "name": "Discover tests", "stage": "prepare",
                       "inputs": {"repository": {"type": "source", "source": {"step": "$input", "port": "repository"}}},
                       "executor": {"type": "tool", "tool_ref": tool, "arguments": {
                           "path": {"type": "source", "source": {"step": "$input", "port": "repository", "pointer": "/path"}},
                           "revision": {"type": "source", "source": {"step": "$input", "port": "repository", "pointer": "/revision"}},
                       }}, "outputs": {"test_configuration": {"delivery_ref": result_type}}}],
            "outputs": {"test_configuration": {"step": "discover", "port": "test_configuration"}},
        }),
    ]

    delivery_sources, defaults, bundle_version = load_catalog()
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

    # A bundle manifest lists every source type, as the factory's does; unused ones stay empty.
    types = {kind: [] for kind in json.loads(
        (REPO / "schemas/template-manifest.schema.json").read_text())["properties"]["types"]["required"]}
    files = {"runtime/worker.mjs": (PACKAGE / "runtime/worker.mjs").read_bytes()}
    reporters = {"node_test": "runtime/node-reporter.mjs",
                 "python_unittest": "runtime/project-tests.py", "command": "runtime/project-tests.py"}
    for path in sorted(set(reporters.values()), reverse=True):
        files[path] = (PACKAGE / path).read_bytes()
    for record in entries:
        filename = f"{record['identity']}-v{record['revision']}.json"
        types.setdefault(record["type"], []).append(filename)
        files[f"configurations/{filename}"] = json.dumps(record).encode()
    usage = runpy.run_path(str(REPO / "scripts/check/configuration_usage.py"))["usage_index"]
    entity_types = "configurations/entity-types/repository.json"
    files.update({
        "configurations/manifest.json": json.dumps({
            "schema_version": 1, "bundle_version": bundle_version, "types": types,
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
    if web is not None:
        contributions = PACKAGE / "web/contributions.json"
        files["ui/contributions.json"] = contributions.read_bytes()
        stylesheet = "ui/web/workbench.css"
        files[stylesheet] = (PACKAGE / "web/workbench.css").read_bytes()
        for asset in web.rglob("*.mjs"):
            files[f"ui/web/{asset.relative_to(web).as_posix()}"] = asset.read_bytes()
        ui = {
            "contributions": "ui/contributions.json",
            "targets": [{"renderer": "web.react", "sdk": WEB_SDK_MAJOR, "kit": KIT_RELEASE,
                         "styles": [stylesheet], "bindings": [
                {"contribution": row["id"], "entry": "ui/web/workbench.mjs", "export": export,
                 "home": export == "BranchQueue"}
                for row, export in zip(json.loads(contributions.read_text()),
                                       ("BranchQueue", "DeliveryWorkbench", "EngineeringSettings"), strict=True)
            ]}],
        }
    manifest = {"format": 2, "id": DOMAIN, "version": VERSION, "domain_protocol": 2,
                "requires": {"host": HOST_VERSION, "handlers": platform_handlers(entries),
                             "extensions": EXTENSION_GROUPS},
                "entity_types": entity_types,
                "files": {path: hashlib.sha256(contents).hexdigest() for path, contents in files.items()},
                "extensions": {
                    "engineering.git/1": {"handlers": {"worktree_branch": BRANCH,
                                                       "pull_request_creation": PR_CREATE}},
                    "engineering.tests/1": {"handlers": {"test_discovery": DISCOVER},
                                            "resources": {"reporters": reporters}},
                },
                "runtime": {"entry": "runtime/worker.mjs", "project_connection": CONNECT, "handlers": {
                    PR_CREATE: {"input_schema": pr_request, "output_schema": pr_result},
                    CONNECT: {"input_schema": connection_input, "output_schema": connection_result},
                    BRANCH: {"input_schema": branch_request, "output_schema": branch_result},
                    DISCOVER: {"core": "engineering.git.snapshot", "input_schema": coordinate, "output_schema": output,
                               "git_snapshot": {
                                   "files": [".devdesk/tests.json", "package.json", "pyproject.toml", "pytest.ini",
                                             "setup.cfg", "Cargo.toml", "go.mod"],
                                   "searches": {"rust_tests": {"glob": "*.rs",
                                      "needles": ["#[test", "#[tokio::test", "#[async_std::test"]}},
                               }},
                }}}
    manifest["runtime"]["handlers"][AUTHORIZE] = {
        "input_schema": {
            "type": "object", "additionalProperties": False,
            "required": ["operation", "source_revision", "source_branch", "destination",
                         "candidate_revision", "regression_configured", "facts"],
            "properties": {
                "operation": {"enum": ["workspace.target", "git.source.publish", "git.delivery.open",
                                       "git.delivery.merge"]},
                "source_revision": coordinate["properties"]["revision"],
                "source_branch": {"type":"string", "minLength":1},
                "destination": {"enum": ["mainline", "parent_work"]},
                "candidate_revision": {"type":["null", "string"]},
                "regression_configured": {"type":"boolean"},
                "facts": {"type":"object", "maxProperties": 32},
            },
        },
        "output_schema": {
            "type": "object", "required": ["allowed"], "additionalProperties": False,
            "properties": {"allowed": {"type":"boolean"}, "reason": {"type":"string"}},
        }
    }
    if web is not None:
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
    parser.add_argument("--web-dir", type=Path, default=REPO / "target/web/software")
    parser.add_argument("--runtime-only", action="store_true",
                        help="omit the Web target; used to build test fixtures")
    args = parser.parse_args()
    build(args.target, None if args.runtime_only else args.web_dir)

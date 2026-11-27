"""Build deterministic ZIPs for the real Host/Docker and dual-client acceptance."""
import hashlib
import json
import runpy
from pathlib import Path
import sys
from zipfile import ZIP_DEFLATED, ZipFile, ZipInfo

ROOT = Path(__file__).parent


HOST_VERSION = (  # read without a TOML parser: the workspace version line in Cargo.toml
    next(line.split('"')[1] for line in (ROOT.parents[2] / "Cargo.toml").read_text().splitlines()
         if line.startswith("version = ")))
KERNEL_TOOLS = {}
for _path in json.loads((ROOT.parents[2] / "plugins/core/manifest.json").read_text())["types"]["tool"]:
    _record = json.loads((ROOT.parents[2] / "plugins/core" / _path).read_text())
    KERNEL_TOOLS[f"{_record['identity']}:{_record['revision']}"] = _record["data"]


def requires(handlers, extensions):
    return {"host": HOST_VERSION, "handlers": sorted(handlers), "extensions": sorted(extensions)}


def platform_handlers(sources):
    """Platform handlers the fixture reaches through the kernel tools its sources adopt."""
    tools = {**KERNEL_TOOLS, **{f"{record['identity']}:{record['revision']}": record["data"]
                                for record in sources if record["type"] == "tool"}}
    used = set()

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

    # A capability's tools are references on the record, next to its data.
    visit(sources)
    return sorted({tools[ref]["qualified_name"] for ref in used
                   if tools.get(ref, {}).get("implementation") == "platform"})


GIT_HEAD = "engineering.git.head"
# Handler bodies of the fixture entries, by name without the package prefix.
HANDLERS = {
    "repository.head": '(_input, core) => ({ head: core.head, observedBy: "domain-worker" })',
    # A refusal: the kernel returns the handler's code and message to the caller.
    "title.read": """(input) => {
      const title = String(input.title ?? "").trim();
      if (title === "refuse") throw Object.assign(new Error("文稿标题不被接受"), { code: "title_rejected" });
      return { title, observedBy: "domain-worker" };
    }""",
    "artifact.create": '(input) => ({ kind: "artifact", title: String(input.title ?? "").trim() })',
    # The kernel input domain.entities: the documents of the project the handler runs in.
    "documents.list": """(_input, core) => {
      const titles = core.entities.map((entity) => entity.data.title);
      return { title: titles.join("\\n"), titles, truncated: core.truncated, observedBy: "domain-worker" };
    }""",
    "inventory.read": """(_input, core) => ({
      title: core.entities.map((entity) => entity.data.title.trim()).sort().join("\\n"),
      observedBy: "domain-worker",
    })""",
}


def package(target, identity, title, handler, core, sdk=1):
    method = f"{identity}.{handler}"
    contribution = f"{identity}.overview"
    artifact_method = f"{identity}.artifact.create" if identity == "manuscript" else None
    # Handlers reading the kernel input domain.entities: a list for pages and RPC, and one
    # whose result fits the Proof type, for a tool step of a workflow.
    entity_methods = ([f"{identity}.documents.list", f"{identity}.inventory.read"]
                      if identity == "manuscript" else [])
    names = [method] + ([artifact_method] if artifact_method else []) + entity_methods
    worker = (ROOT / "worker.mjs").read_text().replace("/* HANDLERS */", ",\n".join(
        f'  "{name}": {HANDLERS[name.removeprefix(identity + ".")]}' for name in names))
    files = {
        "runtime/worker.mjs": worker.encode(),
        "ui/contributions.json": json.dumps([{
            "id": contribution, "title": title, "kind": "collection", "context": "workspace",
            "query": method,
            "inputs": [{"key": "path" if core else "title", "label": "Repository" if core else "Title"}],
            "fields": [{"key": "head" if core else "title", "label": "Observed value"}],
        }]).encode(),
        "ui/web/page.mjs": (ROOT / "page.mjs").read_bytes(),
    }
    entity_types = None
    sources = []
    if identity == "manuscript":
        entity_types = "configurations/entity-types/editorial.json"
        manuscript_schema = {
            "type": "object", "required": ["title"],
            "additionalProperties": False,
            "properties": {"title": {"type": "string", "minLength": 1}},
        }
        files[entity_types] = json.dumps({
            "manuscript.document:1": {"name": "Manuscript", "schema": manuscript_schema,
                "relations": {"previous": {"target": "manuscript.document:1", "max_items": 1}},
                "workflows": {"manuscript.workflow:1": {
                    "input": "manuscript", "outputs": {"observation": "manuscript.proof:1"},
                    "owner_relations": {"observation": ["manuscript"]},
                }, "manuscript.resource-workflow:1": {
                    "input": "manuscript", "outputs": {"proof": "manuscript.proof:1"},
                    "owner_relations": {"proof": ["manuscript"]},
                }, "manuscript.inventory:1": {
                    "input": "manuscript", "outputs": {"observation": "manuscript.proof:1"},
                    "owner_relations": {"observation": ["manuscript"]},
                }}},
            "manuscript.proof:1": {"name": "Proof",
                "relations": {"manuscript": {"target": "manuscript.document:1", "min_items": 1, "max_items": 1}},
                "schema": {
                "type": "object", "required": ["title", "observedBy"],
                "additionalProperties": False,
                "properties": {"title": {"type": "string"}, "observedBy": {"type": "string"}},
            }},
        }).encode()

        def source(kind, key, data, **refs):
            return {"type": kind, "identity": f"{identity}.{key}", "revision": 1,
                    "scope": "global", "name": key.replace("-", " ").title(), "data": data, **refs}

        tool = f"{identity}.read-title:1"
        delivery = f"{identity}.observation:1"
        sources = [
            source("delivery_type", "input", {"format": "json", "content_schema": manuscript_schema},
                   spec_refs=[f"{identity}.observation-spec:1"]),
            # Tools of the package: each names one of its handlers and takes the handler's input.
            source("tool", "read-title", {"qualified_name": f"{identity}.read-title", "implementation": "domain",
                   "handler": method, "input_schema": {"type": "object", "required": ["title"]},
                   "output_schema": {"type": "object", "required": ["title", "observedBy"]}}),
            source("spec", "observation-spec", {
                "schema_version": 3, "body": "Record the title emitted by the worker.",
                "checks": [{"key": "title", "name": "Title", "instructions": "Record the observed title."}],
            }),
            source("delivery_type", "observation", {
                "format": "json",
                "content_schema": {"type": "object", "required": ["title", "observedBy"]},
                "producer": {"kind": "platform",
                             "tools": [f"{identity}.read-title", f"{identity}.read-inventory"]},
            }, spec_refs=[f"{identity}.observation-spec:1"]),
            source("skill", "title-review", {"body": "Review the manuscript title.", "rules": []}),
            source("role", "editor", {"instructions": (
                "You are the chief editor scheduling a single manuscript proofreading step named title. "
                "Use workflow.inspect for current versions, then workflow.step.dispatch to run the trusted worker. "
                "When the step waits for confirmation, use workflow.decision.request for workflow.step.confirm. "
                "Bind the exact waiting execution, plan revision, input digest and outputs. Request the Human to review "
                "the actual proof. Include the step as evidence in the interaction. "
                "After the authenticated Human signs, call workflow.step.accept with the exact accepted output. "
                "Then call workflow.complete "
                "with the current plan revision, control version and expected revision. Do not simulate tool calls."
            )}),
            source("capability", "editor-capability", {},
                   harness_ref="builtin-harness-codex-managed:2", model_ref="builtin-model-gpt-6-2-sol-high:1",
                   skills=[f"{identity}.title-review:1"], tools=[
                       "builtin-tool-codex-native:2", "builtin-tool-workflow-inspect:3",
                       "builtin-tool-workflow-step-dispatch:3", "builtin-tool-workflow-step-accept:3",
                       "builtin-tool-workflow-decision-request:5", "builtin-tool-workflow-complete:3",
                   ]),
            source("agent", "editor-agent", {}, role_ref=f"{identity}.editor:1",
                   capability_ref=f"{identity}.editor-capability:1"),
            source("workflow", "workflow", {
                "schema_version": 6, "orchestrator_role_ref": f"{identity}.editor:1",
                "inputs": {"manuscript": {"delivery_ref": f"{identity}.input:1"}},
                "stages": [{"key": "read", "name": "Read"}],
                "steps": [{
                    "key": "title", "name": "Read title", "stage": "read", "confirmation": "human",
                    "inputs": {"manuscript": {"type": "source", "source": {"step": "$input", "port": "manuscript"}}},
                    "executor": {"type": "tool", "tool_ref": tool, "arguments": {
                        "title": {"type": "source", "source": {"step": "$input", "port": "manuscript", "pointer": "/title"}},
                    }},
                    "outputs": {"observation": {"delivery_ref": delivery}},
                }],
                "outputs": {"observation": {"step": "title", "port": "observation"}},
            }),
        ]
        artifact_schema = {"type": "object", "required": ["kind", "title"], "additionalProperties": False,
                           "properties": {"kind": {"const": "artifact"}, "title": {"type": "string"}}}
        sources.extend([
            source("tool", "seal-material", {
                "qualified_name": f"{identity}.seal-material", "implementation": "domain",
                "handler": artifact_method,
                "input_schema": {"type": "object", "required": ["title"]},
                "output_schema": artifact_schema}),
            source("delivery_type", "sealed-material", {"format": "json", "content_schema": artifact_schema,
                   "producer": {"kind": "platform", "tools": [f"{identity}.seal-material"]}},
                   spec_refs=[f"{identity}.observation-spec:1"]),
            source("delivery_type", "read-material", {"format": "json",
                   "content_schema": {"type": "object", "required": ["title", "observedBy"]},
                   "producer": {"kind": "agent"}}, spec_refs=[f"{identity}.observation-spec:1"]),
            source("role", "proof-reader", {"instructions": (
                "Read exactly /work/artifact with the native shell tool using cat. "
                "This required input is a read-only material staged by the platform. "
                "Use its title and return the Workflow JSON result with status completed, a summary, "
                "and outputs.proof containing title and observedBy set to native-resource-reader. "
                "Do not write any files."
            )}),
            source("capability", "proof-reader-capability", {}, harness_ref="builtin-harness-codex-managed:2",
                   model_ref="builtin-model-gpt-6-2-sol-high:1", skills=[], tools=["builtin-tool-codex-native:2"]),
            source("agent", "proof-reader-agent", {}, role_ref=f"{identity}.proof-reader:1",
                   capability_ref=f"{identity}.proof-reader-capability:1"),
            source("workflow", "resource-workflow", {
                "schema_version": 6, "inputs": {"manuscript": {"delivery_ref": f"{identity}.input:1"}},
                "stages": [{"key": "workspace", "name": "Read sealed material"}],
                "steps": [{
                    "key": "seal", "name": "Seal material", "stage": "workspace",
                    "executor": {"type": "tool", "tool_ref": f"{identity}.seal-material:1", "arguments": {
                        "title": {"type": "source", "source": {"step": "$input", "port": "manuscript", "pointer": "/title"}},
                    }},
                    "outputs": {"sealed": {"delivery_ref": f"{identity}.sealed-material:1"}},
                }, {
                    "key": "read", "name": "Read frozen workspace", "stage": "workspace", "dependencies": ["seal"],
                    "task_instructions": "Read /work/artifact using the native shell tool and return the exact title in proof.",
                    "inputs": {"material": {"type": "source", "source": {"step": "seal", "port": "sealed"}}},
                    "access": {"workspace_input": "material"},
                    "harness_permission_ref": "builtin-permission-harness-read-only:2",
                    "executor": {"type": "agent", "role_ref": f"{identity}.proof-reader:1"},
                    "outputs": {"proof": {"delivery_ref": f"{identity}.read-material:1"}},
                }], "outputs": {"proof": {"step": "read", "port": "proof"}},
            }),
        ])
        # A tool step whose handler reads the kernel input domain.entities.
        sources.extend([
            source("tool", "read-inventory", {
                "qualified_name": f"{identity}.read-inventory", "implementation": "domain",
                "handler": entity_methods[1],
                "input_schema": {"type": "object", "additionalProperties": False},
                "output_schema": {"type": "object", "required": ["title", "observedBy"]}}),
            source("workflow", "inventory", {
                "schema_version": 6,
                "inputs": {"manuscript": {"delivery_ref": f"{identity}.input:1"}},
                "stages": [{"key": "read", "name": "Read"}],
                "steps": [{
                    "key": "title", "name": "List titles", "stage": "read", "confirmation": "human",
                    "inputs": {"manuscript": {"type": "source", "source": {"step": "$input", "port": "manuscript"}}},
                    "executor": {"type": "tool", "tool_ref": f"{identity}.read-inventory:1", "arguments": {}},
                    "outputs": {"observation": {"delivery_ref": delivery}},
                }],
                "outputs": {"observation": {"step": "title", "port": "observation"}},
            }),
        ])
        usage = runpy.run_path(str(ROOT.parents[2] / "scripts/check/configuration_usage.py"))["usage_index"]
        # A bundle manifest lists every source type, as the factory's does; unused ones stay empty.
        types = {kind: [] for kind in json.loads(
            (ROOT.parents[2] / "schemas/template-manifest.schema.json").read_text())["properties"]["types"]["required"]}
        for record in sources:
            path = f"{record['identity']}.json"
            files[f"configurations/{path}"] = json.dumps(record).encode()
            types.setdefault(record["type"], []).append(path)
        files["configurations/manifest.json"] = json.dumps({
            "schema_version": 1, "bundle_version": "manuscript-1", "types": types,
            "defaults": "defaults.json", "usage_index": "usage.json",
        }).encode()
        files["configurations/defaults.json"] = json.dumps({
            "role_agent_bindings": {f"{identity}.editor:1": f"{identity}.editor-agent:1",
                                    f"{identity}.proof-reader:1": f"{identity}.proof-reader-agent:1"},
        }).encode()
        files["configurations/usage.json"] = json.dumps({"references": usage(sources)}).encode()
    # Handlers the fixture reaches: the kernel tools its sources adopt. Its own tools are
    # domain tools and name its own handlers.
    handlers = platform_handlers(sources)
    entity_handlers = {name: {
        "core": "domain.entities",
        "entities": {"types": [f"{identity}.document:1"], "max_items": 50},
        "input_schema": {"type": "object", "additionalProperties": False},
        "output_schema": schema,
    } for name, schema in zip(entity_methods, (
        {"type": "object", "required": ["titles", "observedBy"]},
        {"type": "object", "required": ["title", "observedBy"], "additionalProperties": False,
         "properties": {"title": {"type": "string"}, "observedBy": {"type": "string"}}},
    ))}
    manifest = {
        "format": 2, "id": identity, "version": "1", "domain_protocol": 2,
        "requires": requires(handlers, ["engineering.git/1"] if core == GIT_HEAD else []),
        **({"entity_types": entity_types} if entity_types else {}),
        "files": {path: hashlib.sha256(value).hexdigest() for path, value in files.items()},
        "runtime": {
            "entry": "runtime/worker.mjs",
            "handlers": {
                method: {
                    **({"core": core} if core else {}),
                    "input_schema": {"type": "object", "required": ["path" if core else "title"]},
                    "output_schema": {"type": "object",
                                      "required": ["head" if core else "title", "observedBy"]},
                },
                **({artifact_method: {
                    "input_schema": {"type": "object", "required": ["title"]},
                    "output_schema": artifact_schema,
                }} if artifact_method else {}),
                **entity_handlers,
            },
        },
        "ui": {
            "contributions": "ui/contributions.json",
            "targets": [{"renderer": "web.react", "sdk": sdk, "bindings": [{
                "contribution": contribution, "entry": "ui/web/page.mjs", "export": "DomainPage",
            }]}],
        },
    }
    files["manifest.json"] = json.dumps(manifest).encode()
    with ZipFile(target / f"{identity}.zip", "w") as archive:
        for name, data in sorted(files.items()):
            archive.writestr(ZipInfo(name), data, compress_type=ZIP_DEFLATED)


if __name__ == "__main__":
    root = Path(sys.argv[1])
    root.mkdir(parents=True, exist_ok=True)
    package(root, "git-probe", "Git repository", "repository.head", GIT_HEAD)
    package(root, "manuscript", "Manuscript", "title.read", None)
    # Written for a Web SDK major this Core does not speak; it installs, but its pages do not run.
    package(root, "stray-kit", "Stray kit", "title.read", None, sdk=2)

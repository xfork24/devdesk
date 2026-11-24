"""Build deterministic ZIPs for the real Host/Docker and dual-client acceptance."""
import hashlib
import json
import runpy
from pathlib import Path
import sys
from zipfile import ZIP_DEFLATED, ZipFile, ZipInfo

ROOT = Path(__file__).parent


def package(target, identity, title, handler, core):
    method = f"{identity}.{handler}"
    contribution = f"{identity}.overview"
    worker = (ROOT / "worker.mjs").read_text().replace(
        '["PLACEHOLDER_HANDLER"]', json.dumps([method])
    )
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
    if identity == "manuscript":
        def source(kind, key, data, **refs):
            return {"type": kind, "identity": f"{identity}.{key}", "revision": 1,
                    "scope": "global", "name": key.replace("-", " ").title(), "data": data, **refs}

        tool = f"{identity}.read-title:1"
        delivery = f"{identity}.observation:1"
        sources = [
            source("tool", "read-title", {"qualified_name": "domain.read", "implementation": "platform",
                   "input_schema": {"type": "object", "required": ["handler", "input"]},
                   "output_schema": {"type": "object", "required": ["title", "observedBy"]}}),
            source("spec", "observation-spec", {
                "schema_version": 3, "body": "Record the title emitted by the worker.",
                "checks": [{"key": "title", "name": "Title", "instructions": "Record the observed title."}],
            }),
            source("delivery_type", "observation", {
                "format": "json",
                "content_schema": {"type": "object", "required": ["title", "observedBy"]},
                "producer": {"kind": "platform", "tools": ["domain.read"]},
            }, spec_refs=[f"{identity}.observation-spec:1"]),
            source("skill", "title-review", {"body": "Review the manuscript title.", "rules": []}),
            source("role", "editor", {"instructions": "Review manuscript wording."}),
            source("capability", "editor-capability", {},
                   harness_ref="builtin-harness-codex-managed:2", model_ref="builtin-model-gpt-6-2-sol-high:1",
                   skills=[f"{identity}.title-review:1"], tools=[]),
            source("agent", "editor-agent", {}, role_ref=f"{identity}.editor:1",
                   capability_ref=f"{identity}.editor-capability:1"),
            source("workflow", "workflow", {
                "schema_version": 6, "stages": [{"key": "read", "name": "Read"}],
                "steps": [{
                    "key": "title", "name": "Read title", "stage": "read",
                    "executor": {"type": "tool", "tool_ref": tool, "arguments": {
                        "handler": {"type": "constant", "value": method},
                        "input": {"type": "constant", "value": {"title": "  Local manuscript  "}},
                    }},
                    "outputs": {"observation": {"delivery_ref": delivery}},
                }],
                "outputs": {"observation": {"step": "title", "port": "observation"}},
            }),
        ]
        usage = runpy.run_path(str(ROOT.parents[2] / "scripts/py/configuration_usage.py"))["usage_index"]
        types = {}
        for record in sources:
            path = f"{record['identity']}.json"
            files[f"configurations/{path}"] = json.dumps(record).encode()
            types.setdefault(record["type"], []).append(path)
        files["configurations/manifest.json"] = json.dumps({
            "schema_version": 1, "bundle_version": "manuscript-1", "types": types,
            "defaults": "defaults.json", "usage_index": "usage.json",
        }).encode()
        files["configurations/defaults.json"] = b"{}"
        files["configurations/usage.json"] = json.dumps({"references": usage(sources)}).encode()
    manifest = {
        "format": 1, "id": identity, "version": "1",
        "files": {path: hashlib.sha256(value).hexdigest() for path, value in files.items()},
        "runtime": {
            "entry": "runtime/worker.mjs",
            "handlers": {method: {
                **({"core": core} if core else {}),
                "input_schema": {"type": "object", "required": ["path" if core else "title"]},
                "output_schema": {"type": "object", "required": ["head" if core else "title", "observedBy"]},
            }},
        },
        "ui": {
            "contributions": "ui/contributions.json",
            "targets": [{"renderer": "web.react", "api": 1, "bindings": [{
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
    package(root, "git-probe", "Git repository", "repository.head", "git_head")
    package(root, "manuscript", "Manuscript", "title.read", None)

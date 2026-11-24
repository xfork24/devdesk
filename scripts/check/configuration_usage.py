#!/usr/bin/env python3
"""Generate the packaged, descriptive reverse-reference index from published sources."""
import argparse
import json
from pathlib import Path

ROOT = Path(__file__).resolve().parents[2] / "plugins/core"


def usage_index(sources: list[dict]) -> dict:
    index = {
        f"{s['identity']}:{s['revision']}": []
        for s in sources
        if s["type"] in ("spec", "skill", "tool", "delivery_type")
    }
    for source in sources:
        def add(target, path, kind="reference", stage=None, step=None, port=None):
            row = {
                "reference": f"{source['identity']}:{source['revision']}",
                "name": source["name"], "type": source["type"],
                "version": source["revision"], "scope": source["scope"],
                "path": path, "kind": kind,
            }
            for key, value in (("stage", stage), ("step", step), ("port", port)):
                if value is not None:
                    row[key] = value
            index.setdefault(target, []).append(row)

        if source["type"] in ("capability", "delivery_type"):
            fields = ("spec_refs", "skills", "tools") if source["type"] == "capability" else ("spec_refs",)
            for field in fields:
                for i, target in enumerate(source.get(field, [])):
                    add(target, f"/{field}/{i}")
        elif source["type"] == "workflow":
            data = source["data"]
            if role := data.get("orchestrator_role_ref"):
                add(role, "/data/orchestrator_role_ref")
            inputs = data.get("inputs", {})
            steps = {s["key"]: s for s in data["steps"]}
            stages = {s["key"]: {"key": s["key"], "name": s["name"]} for s in data.get("stages", [])}

            def delivery(ref):
                ports = inputs if ref["step"] == "$input" else steps.get(ref["step"], {}).get("outputs", {})
                return ports.get(ref["port"], {}).get("delivery_ref")

            for name, value in sorted(inputs.items()):
                add(value["delivery_ref"], f"/data/inputs/{name}/delivery_ref", "workflow_input", port=name)
            for i, step in enumerate(data["steps"]):
                stage = stages.get(step["stage"])
                node = {"key": step["key"], "name": step["name"]}
                for j, spec_ref in enumerate(step.get("spec_refs", [])):
                    add(spec_ref, f"/data/steps/{i}/spec_refs/{j}", "step_spec", stage, node)
                for name, value in sorted(step.get("outputs", {}).items()):
                    add(value["delivery_ref"], f"/data/steps/{i}/outputs/{name}/delivery_ref", "step_output", stage, node, name)
                for name, binding in sorted(step.get("inputs", {}).items()):
                    if binding["type"] == "source" and (target := delivery(binding["source"])):
                        add(target, f"/data/steps/{i}/inputs/{name}", "step_input", stage, node, name)
                executor = step["executor"]
                if executor["type"] == "tool":
                    add(executor["tool_ref"], f"/data/steps/{i}/executor/tool_ref", "step_tool", stage, node)
                    for name, binding in sorted(executor.get("arguments", {}).items()):
                        if binding["type"] == "source" and (target := delivery(binding["source"])):
                            add(target, f"/data/steps/{i}/executor/arguments/{name}", "step_argument", stage, node, name)
                elif executor["type"] == "workflow" and executor.get("foreach"):
                    if target := delivery(executor["foreach"]["source"]):
                        add(target, f"/data/steps/{i}/executor/foreach/source", "step_collection", stage, node)
            for name, ref in sorted(data.get("outputs", {}).items()):
                if target := delivery(ref):
                    add(target, f"/data/outputs/{name}", "workflow_output", port=name)
    return {
        key: sorted(users, key=lambda r: (r["type"], r["reference"], r["path"]))
        for key, users in sorted(index.items())
    }


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--write", action="store_true", help="update the packaged index")
    args = parser.parse_args()
    manifest = json.loads((ROOT / "manifest.json").read_text())
    sources = [json.loads((ROOT / path).read_text()) for paths in manifest["types"].values() for path in paths]
    expected = {"schema_version": 1, "references": usage_index(sources)}
    path = ROOT / manifest["usage_index"]
    if args.write:
        path.write_text(json.dumps(expected, ensure_ascii=False, indent=2) + "\n")
    elif json.loads(path.read_text()) != expected:
        raise SystemExit("Packaged usage index is stale; run python3 scripts/check/configuration_usage.py --write")
    print(f"configuration-usage: OK — {len(expected['references'])} exact revisions")


if __name__ == "__main__":
    main()

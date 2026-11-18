#!/usr/bin/env python3
"""Measure a frozen Host test binary against an isolated SQLite backup (no execution)."""
import argparse
import hashlib
import json
import os
from pathlib import Path
import shutil
import sqlite3
import subprocess
import sys


def copy_dataset(source, destination):
    destination.mkdir(parents=True, exist_ok=False)
    files = [source / "global.sqlite3", *sorted(source.glob("projects/*/engineering.sqlite3"))]
    discussion = source / "conversations/global/engineering.sqlite3"
    if discussion.exists():
        files.append(discussion)
    for path in files:
        target = destination / path.relative_to(source)
        target.parent.mkdir(parents=True, exist_ok=True)
        original = sqlite3.connect(f"file:{path}?mode=ro", uri=True)
        copied = sqlite3.connect(target)
        try:
            original.backup(copied)
        finally:
            original.close()
            copied.close()
        shutil.copy2(path.parent / ".devdesk-dataset", target.parent / ".devdesk-dataset")
    (destination / ".performance-copy").write_text("offline performance fixture\n")


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--test-binary", type=Path, required=True)
    parser.add_argument("--dataset", type=Path, required=True)
    parser.add_argument("--output", type=Path, required=True)
    parser.add_argument("--project", help="Explicit project fixture ID; defaults to the backup selection or first project")
    parser.add_argument("--feature", help="Exact Feature for the execution-page cohort")
    parser.add_argument("--samples", type=int, default=30)
    parser.add_argument("--browser", action="store_true")
    parser.add_argument("--cold-browser", action="store_true", help="Reload JavaScript/configuration cache on every navigation")
    parser.add_argument("--model-conditions", action="store_true", help="Fresh browser with the same 1,200 ms slow/failed model fixture")
    parser.add_argument("--browser-page", choices=["configuration", "execution"], default="configuration")
    parser.add_argument("--assert-configuration-isolation", action="store_true", help="Fail RPC sampling if configuration/events restore full state or write transactions")
    args = parser.parse_args()
    if not 1 <= args.samples <= 1000:
        parser.error("samples must be 1..1000")
    if (args.browser or args.cold_browser or args.model_conditions) and args.assert_configuration_isolation:
        parser.error("configuration isolation assertions apply to the RPC cohort")
    args.output.mkdir(parents=True, exist_ok=False)
    tooling = {}
    for name in ["configuration_loading.py", "configuration-loading.mjs", "execution-loading.mjs", "model-loading.mjs"]:
        source = Path(__file__).with_name(name)
        tooling[name] = hashlib.sha256(source.read_bytes()).hexdigest()
        shutil.copy2(source, args.output / name)
    binary = args.output / "host-tests"
    shutil.copy2(args.test_binary, binary)
    binary.chmod(0o700)
    dataset = args.output / "dataset"
    copy_dataset(args.dataset.resolve(), dataset)
    with sqlite3.connect(dataset / "global.sqlite3") as conn:
        selected = conn.execute("SELECT CAST(value AS TEXT) FROM settings WHERE key='active_project'").fetchone()
        first = conn.execute("SELECT id FROM project_catalog WHERE project IS NOT NULL ORDER BY id LIMIT 1").fetchone()
        project = args.project or (json.loads(selected[0]) if selected else first[0] if first else None)
        if project and not conn.execute("SELECT 1 FROM project_catalog WHERE id=? AND project IS NOT NULL",(project,)).fetchone():
            raise RuntimeError("selected performance project is absent from the copied dataset")
    env = {**os.environ, "DEVDESK_PERF_DATASET": str(dataset.resolve()), "DEVDESK_PERF_OUTPUT": str((args.output / "rpc.jsonl").resolve()), "DEVDESK_PERF_SAMPLES": str(args.samples)}
    if project:
        env["DEVDESK_PERF_PROJECT"] = project
    env.pop("DEVDESK_ACCESS_TOKEN", None)
    metadata = {"binary_sha256": hashlib.sha256(binary.read_bytes()).hexdigest(), "source_dataset": str(args.dataset.resolve()), "samples": args.samples, "project_id": project, "offline": True, "scheduler": False, "node": subprocess.check_output(["node", "--version"], text=True).strip()}
    metadata["tooling_sha256"] = tooling
    metadata["copied_input_databases"] = {str(path.relative_to(dataset)): hashlib.sha256(path.read_bytes()).hexdigest() for path in sorted(dataset.rglob("*.sqlite3"))}
    metadata["feature_id"] = args.feature
    (args.output / "identity.json").write_text(json.dumps(metadata, indent=2))
    if not (args.browser or args.cold_browser or args.model_conditions):
        subprocess.run([str(binary.resolve()), "loading_performance::configuration_load_profile", "--ignored", "--exact", "--nocapture"], env=env, check=True)
        rows = [json.loads(line) for line in (args.output / "rpc.jsonl").read_text().splitlines()]
        groups = {}
        for row in rows:
            key = row["method"] + ":" + str(row.get("params", {}).get("offset", 0))
            if row.get("params", {}).get("view"): key += ":" + row["params"]["view"]
            if row["method"] == "query.configuration_index": key += ":" + ("project" if row["params"].get("project_id") else "global")
            groups.setdefault(key, []).append(row)
        summary = {}
        for key, group in groups.items():
            times = sorted(row["ms"] for row in group)
            summary[key] = {"samples": len(times), "p50_ms": times[len(times)//2], "p95_ms": times[max(0, (95*len(times)+99)//100-1)], "bytes": group[-1]["bytes"], "spans": group[-1]["measurement"]["spans"]}
        (args.output / "summary.json").write_text(json.dumps(summary, indent=2))
        if args.assert_configuration_isolation:
            forbidden = {"global.full_snapshot", "project.full_snapshot", "global.transaction", "project.transaction"}
            violations = [{"method": row["method"], "sample": row["sample"], "stages": sorted(forbidden.intersection(row["measurement"]["spans"]))} for row in rows if row["method"] != "query.workspace" and forbidden.intersection(row["measurement"]["spans"])]
            (args.output / "configuration-isolation.json").write_text(json.dumps({"samples": sum(row["method"] != "query.workspace" for row in rows), "violations": violations}, indent=2))
            if violations:
                raise RuntimeError(f"configuration isolation failed: {violations}")
        print(json.dumps({key: {k: v for k, v in value.items() if k != "spans"} for key, value in summary.items()}, indent=2))
        return
    env["DEVDESK_PERF_LOG"] = "1"
    with (args.output / "server.log").open("w") as log:
        server = subprocess.Popen([str(binary.resolve()), "loading_performance::configuration_load_server", "--ignored", "--exact", "--nocapture"], env=env, stdout=subprocess.PIPE, stderr=log, text=True)
        try:
            origin = None
            for line in server.stdout:
                log.write(line)
                if line.startswith("PERF_ORIGIN="):
                    origin = line.strip().split("=", 1)[1]
                    break
            if not origin:
                raise RuntimeError("benchmark server failed to start")
            script = "model-loading.mjs" if args.model_conditions else "execution-loading.mjs" if args.browser_page == "execution" else "configuration-loading.mjs"
            extras = [project or "", args.feature or ""] if args.browser_page == "execution" and not args.model_conditions else ["cold" if args.cold_browser else "warm", project or ""]
            subprocess.run(["node", str(Path(__file__).with_name(script)), origin, str(args.output.resolve()), str(args.samples), str(server.pid), *extras], check=True)
        finally:
            server.send_signal(__import__("signal").SIGINT)
            try:
                server.wait(timeout=10)
            except subprocess.TimeoutExpired:
                server.kill()
                server.wait()


if __name__ == "__main__":
    main()

#!/usr/bin/env python3
"""Sample a production Host and Web against a dedicated, quiescent dataset backup."""

import argparse
import contextlib
import concurrent.futures
import hashlib
import http.cookiejar
import json
import os
import platform
import re
import select
import shutil
import signal
import sqlite3
import subprocess
import sys
import tarfile
import tempfile
import threading
import time
import urllib.parse
import urllib.request
from pathlib import Path

from joint_frontends import stats


REPO = Path(__file__).resolve().parents[2]
ORIGIN = re.compile(rb"http://127\.0\.0\.1:\d+")


def sha(path):
    digest = hashlib.sha256()
    with path.open("rb") as stream:
        for block in iter(lambda: stream.read(1024 * 1024), b""):
            digest.update(block)
    return digest.hexdigest()


def write(path, value):
    path.write_text(json.dumps(value, indent=2, ensure_ascii=False) + "\n")


def append(path, row):
    with path.open("a") as stream:
        stream.write(json.dumps(row, ensure_ascii=False) + "\n")


def sqlite_paths(root):
    return sorted(root.rglob("*.sqlite3"))


def connect(path, immutable=False):
    suffix = "?mode=ro&immutable=1" if immutable else "?mode=ro"
    return contextlib.closing(sqlite3.connect(path.as_uri() + suffix, uri=True))


def dataset_manifest(root):
    files = {}
    databases = {}
    for path in sorted(root.rglob("*")):
        if path.is_symlink():
            raise ValueError("The benchmark backup must be self-contained without symlinks")
        if path.is_file():
            rel = str(path.relative_to(root))
            files[rel] = {"bytes": path.stat().st_size, "sha256": sha(path)}
    for path in sqlite_paths(root):
        if Path(str(path) + "-wal").is_file() or Path(str(path) + "-shm").is_file():
            raise ValueError("Use an offline SQLite backup without WAL/SHM sidecars")
        with connect(path, immutable=True) as db:
            integrity = db.execute("PRAGMA quick_check").fetchall()
            fks = db.execute("PRAGMA foreign_key_check").fetchone()
            if integrity != [("ok",)] or fks:
                raise ValueError(f"Invalid fixture database: {path.name}")
            version = db.execute("PRAGMA user_version").fetchone()[0]
            tables = {
                name: db.execute('SELECT COUNT(*) FROM "' + name.replace('"', '""') + '"').fetchone()[0]
                for (name,) in db.execute("SELECT name FROM sqlite_schema WHERE type='table' AND name NOT LIKE 'sqlite_%'")
            }
            kinds = (dict(db.execute("SELECT type,COUNT(*) FROM governance_records GROUP BY type"))
                     if "governance_records" in tables else {})
            databases[str(path.relative_to(root))] = {"version": version, "tables": tables, "governance": kinds}
    return {"files": files, "databases": databases}


def clone(source, dest):
    # A private copy for every reset/startup prevents writes or recovery from
    # drifting the seed. APFS clonefile accelerates this without sharing inodes.
    if sys.platform == "darwin":
        subprocess.run(["cp", "-cR", str(source), str(dest)], check=True)
    else:
        shutil.copytree(source, dest)


def checkpoint_fixture(root):
    """Only call for a generated copy after its sole Host has exited."""
    for path in sqlite_paths(root):
        with contextlib.closing(sqlite3.connect(path)) as db:
            busy, _, _ = db.execute("PRAGMA wal_checkpoint(TRUNCATE)").fetchone()
        wal = Path(str(path) + "-wal")
        if busy or (wal.exists() and wal.stat().st_size):
            raise RuntimeError("Could not checkpoint the generated fixture")
        wal.unlink(missing_ok=True)
        Path(str(path) + "-shm").unlink(missing_ok=True)


def identity(args):
    def git(*parts):
        return subprocess.check_output(["git", "-C", str(REPO), *parts])
    paths = sorted(set(git("ls-files", "-z", "--cached", "--others", "--exclude-standard").split(b"\0")) - {b""})
    source = {}
    for name in map(os.fsdecode, paths):
        file = REPO / name
        if not file.is_relative_to(args.output):
            source[name] = sha(file) if file.is_file() else None
    web = {str(p.relative_to(args.web)): sha(p) for p in sorted(args.web.rglob("*")) if p.is_file()}
    return {
        "git_head": git("rev-parse", "HEAD").decode().strip(),
        "working_tree": source,
        "working_tree_diff_sha256": hashlib.sha256(git("diff", "--binary", "HEAD")).hexdigest(),
        "host_sha256": sha(args.host),
        "web": web,
        "machine": {
            "os": platform.platform(),
            "logical_cpu": os.cpu_count(),
            "allowed_cpus": sorted(os.sched_getaffinity(0)) if hasattr(os, "sched_getaffinity") else None,
            "memory_bytes": (
                int(subprocess.check_output(["sysctl", "-n", "hw.memsize"]))
                if sys.platform == "darwin"
                else os.sysconf("SC_PHYS_PAGES") * os.sysconf("SC_PAGE_SIZE")
            ),
            "network": "loopback",
            "filesystem_cache": "OS cache retained",
        },
        "python": platform.python_version(),
        "node": subprocess.check_output(["node", "--version"], text=True).strip(),
        "samples": args.samples,
        "project_id": args.project,
        "feature_id": args.feature,
        "root": args.root,
        "cohort": args.cohort,
        "mode": args.run,
        "rpc_cases": args.rpc_case,
        "reset_readers": args.reset_readers,
        "reset_full_only": args.reset_full_only,
        "track_reset_progress": args.track_reset_progress,
    }


@contextlib.contextmanager
def host(binary, dataset, run_dir, name, *, expose_process=False):
    env = {k: v for k, v in os.environ.items() if not k.startswith(("DEVDESK_", "GH_", "GITHUB_"))}
    env["DEVDESK_PERF_LOG"] = "1"
    log = run_dir / f"{name}.host.log"
    with log.open("wb") as stderr:
        begin = time.perf_counter()
        process = subprocess.Popen([binary, "--data", dataset, "--port", "0"], cwd=run_dir,
                                   env=env, stderr=stderr, stdout=subprocess.PIPE,
                                   bufsize=0)
        try:
            buf = b""
            until = time.monotonic() + 60
            while time.monotonic() < until and process.poll() is None:
                if select.select([process.stdout], [], [], 0.2)[0]:
                    buf += os.read(process.stdout.fileno(), 65536)
                    match = ORIGIN.search(buf)
                    if match:
                        ready = match[0].decode(), (time.perf_counter() - begin) * 1000, log
                        yield (*ready, process) if expose_process else ready
                        return
            raise RuntimeError(f"Host startup failed. See {log.name}")
        finally:
            if process.poll() is None:
                process.send_signal(signal.SIGTERM)
            try:
                process.wait(timeout=10)
            except subprocess.TimeoutExpired:
                process.kill()
                process.wait()
            process.stdout.close()


def client(origin):
    opener = urllib.request.build_opener(
        urllib.request.ProxyHandler({}),
        urllib.request.HTTPCookieProcessor(http.cookiejar.CookieJar()),
    )
    request = urllib.request.Request(origin + "/session", data=b"{}",
                                     headers={"Content-Type": "application/json"})
    opener.open(request, timeout=15).close()
    return opener


def rpc(opener, origin, method, params):
    request = urllib.request.Request(origin + "/rpc", json.dumps({
        "jsonrpc": "2.0", "id": 1, "method": method, "params": params,
    }).encode(), {"Content-Type": "application/json"})
    row = {"method": method, "error": None, "bytes": 0}
    value = None
    begin = time.perf_counter()
    try:
        with opener.open(request, timeout=60) as response:
            raw = response.read()
            row.update(ms=(time.perf_counter() - begin) * 1000,
                       bytes=len(raw), request_id=response.headers.get("x-devdesk-request-id"))
        document = json.loads(raw)
        if not isinstance(document, dict):
            row["error"] = "invalid-json-rpc-response"
        elif "error" in document:
            error = document["error"]
            row["error"] = str((error.get("data") or {}).get("code") or error.get("code"))
        elif "result" in document:
            value = document["result"]
            row["result_sha256"] = hashlib.sha256(json.dumps(value, sort_keys=True).encode()).hexdigest()
        else:
            row["error"] = "invalid-json-rpc-response"
    except (OSError, ValueError) as exc:
        row["error"] = type(exc).__name__
    row.setdefault("ms", (time.perf_counter() - begin) * 1000)
    return row, value


def required(opener, origin, method, params):
    sample, value = rpc(opener, origin, method, params)
    if sample["error"]:
        raise RuntimeError(f'{method}: {sample["error"]}')
    return value


def verify_web(args, origin, opener):
    for asset in sorted(args.web.rglob("*")):
        if not asset.is_file() or asset.name.endswith(".map"):
            continue
        href = "/" if asset.name == "index.html" else "/" + asset.relative_to(args.web).as_posix()
        with opener.open(origin + href, timeout=10) as response:
            actual = hashlib.sha256(response.read()).hexdigest()
        if sha(asset) != actual:
            raise RuntimeError(f"Release Host does not serve the supplied Web build: {href}")


def cases(args, opener, origin):
    project = {"project_id": args.project}
    root = {**project, "id": args.root}
    owner = {**project, "owner_id": args.feature, "limit": 50}
    feature = {**project, "feature_id": args.feature, "github_only": True, "limit": 50}
    materials = required(opener, origin, "query.workflow_materials", owner)
    decisions = required(opener, origin, "query.orchestration", {**project, "root_instance_id": args.root})
    result = [
        ("overview", "query.workspace", {**project, "github_only": True, "limit": 50}),
        ("work-queue", "query.execution_workspace", {**project, "github_only": True, "limit": 50}),
        ("selected-feature", "query.execution_workspace", feature),
        ("selected-feature-editors", "query.workspace", feature),
        ("manager-summary", "query.workflow_management_summary", root),
        ("manager-progress", "query.workflow_progress", root),
        ("manager-driver", "query.workflow_driver", {**root, "view": "summary"}),
        ("manager-driver-full", "query.workflow_driver", {**project, "id": args.approval_root or args.root}),
        ("manager-orchestration", "query.orchestration", {**project, "root_instance_id": args.root}),
        ("manager-index", "query.orchestration_summary", {**project, "root_instance_id": args.root}),
        ("workflow-status", "query.workflow_instance", {**root, "view": "status"}),
        ("materials", "query.workflow_materials", owner),
        ("contexts", "query.workflow_contexts", owner),
        ("records", "query.workflow_executions", owner),
        ("catalog-index", "query.configuration_index", {**project, "local": True, "limit": 1000}),
        ("components-bootstrap", "query.ui_bootstrap", project),
        ("project-directory", "query.project_directory", {"limit": 100, "project_offset": 0}),
        ("settings", "query.settings", {}),
        ("history", "query.history", {**project, "scope": "project", "local": True, "include_tool_calls": True,
                                     "view": "summary", "limit": 50}),
        ("discussion-agents", "query.discussion_agents", {**project, "view": "index", "limit": 50}),
        ("reset-preview", "query.template_reset_preview", {}),
    ]
    for kind in ["model", "permission_profile", "agent", "workflow"]:
        result.append((kind + "-configurations", "query.configurations", {**project, "kind": kind, "limit": 50}))
    for name, method in [("materials", "query.workflow_materials"), ("contexts", "query.workflow_contexts"),
                         ("records", "query.workflow_executions")]:
        result.append((name + "-deep", method, {**owner, "offset": 50}))
    result.append(("materials-filtered", "query.workflow_materials", {**owner, "search": args.filter}))
    if materials.get("records"):
        result.append(("material-body", "query.workflow_materials", {**owner, "output_id": materials["records"][0]["id"]}))
    else:
        append(args.output / "coverage.jsonl", {"case": "material-body", "reason": "no material in scope"})
    resolved = next((x for x in decisions.get("decisions", []) if x["data"]["status"] != "pending"), None)
    if resolved:
        result.append(("manager-history-exact", "query.orchestration_decision", {**project, "id": resolved["id"]}))
    else:
        append(args.output / "coverage.jsonl", {"case": "manager-history-exact", "reason": "no signed history in scope"})
    if args.approval_root:
        result.append(("manager-pending-evidence", "query.orchestration",
                       {**project, "root_instance_id": args.approval_root}))
    else:
        append(args.output / "coverage.jsonl", {"case": "manager-pending-evidence", "reason": "approval root not supplied"})
    return result


def sse(opener, origin, project, last_event=None):
    query = urllib.parse.urlencode({"project_id": project, "tail": "true", "configuration": "true"})
    headers = {"Accept": "text/event-stream"}
    if last_event is not None:
        headers["Last-Event-ID"] = last_event
    request = urllib.request.Request(origin + "/events?" + query, headers=headers)
    with opener.open(request, timeout=10) as response:
        for line in response:
            if line.startswith(b"data: "):
                value = json.loads(line[6:])
                if value.get("project_id") != project:
                    raise ValueError("stream scope changed")
                if value.get("kind") in ("stream.ready", "stream.reset"):
                    return f'{project}:{value["cursor"]}'
    raise OSError("no ready/reset event")


def read_samples(args, origin, opener):
    samples = args.output / "samples.jsonl"
    if args.run == "growth":
        owner = {"project_id": args.project, "owner_id": args.feature, "limit": 50}
        selected = [(kind + suffix, "query.workflow_" + kind, {**owner, **options})
                    for kind in ("materials", "contexts", "executions")
                    for suffix, options in (("", {}), ("-deep", {"offset": 50}))]
        selected.append(("materials-filtered", "query.workflow_materials", {**owner, "search": args.filter}))
    else:
        selected = cases(args, opener, origin)
    if args.rpc_case:
        known = {name for name, _, _ in selected}
        if not set(args.rpc_case) <= known:
            raise ValueError("Unknown RPC benchmark case")
        selected = [case for case in selected if case[0] in args.rpc_case]
    for name, method, params in selected:
        rpc(opener, origin, method, params)
        for cohort in ("warm-spaced", "warm-repeat"):
            for index in range(args.samples):
                if cohort == "warm-spaced":
                    time.sleep(.12)
                row, _ = rpc(opener, origin, method, params)
                append(samples, dict(row, case=name, cohort=cohort, sample=index, host="read"))
    if args.run == "growth":
        return
    cursor = sse(opener, origin, args.project)
    for index in range(args.samples):
        begin = time.perf_counter()
        error = None
        try:
            cursor = sse(opener, origin, args.project, cursor)
        except (OSError, ValueError) as exc:
            error = type(exc).__name__
        append(samples, {"case": "event-stream-reconnect", "cohort": "warm", "sample": index,
                         "ms": (time.perf_counter() - begin) * 1000, "error": error})


def reset_command(preview, preserve, index):
    return {"command_id": f"workbench-{index}-{time.time_ns()}", "payload": {
        "expected_catalog_generation": preview["generation"], "catalog_fingerprint": preview["catalog_fingerprint"],
        "bundle_hash": preview["bundle_hash"], "preserve_custom": preserve,
    }}


def project_generations(dataset):
    generations = {}
    for path in sqlite_paths(dataset):
        with connect(path) as db:
            generation = db.execute(
                "SELECT json_extract(data,'$.generation') FROM governance_records WHERE id='template-catalog-state'"
            ).fetchone()
            generations[str(path.relative_to(dataset))] = generation[0] if generation else None
    return generations


def reset_seed(args, dest):
    clone(args.dataset, dest)
    with host(args.host, dest, args.output, "prepare-reset-seed") as (origin, _, _):
        opener = client(origin)
        preview = required(opener, origin, "query.template_reset_preview", {})
        custom = required(opener, origin, "delivery_type.create", {
            "command_id": "workbench-reset-custom",
            "expected_catalog_generation": preview["generation"],
            "payload": {
                "spec_refs": ["builtin-spec-stage-prd:4"],
                "name": "Workbench benchmark custom delivery",
                "data": {"format": "markdown", "body": "Isolated benchmark fixture"},
            },
        })
        if required(opener, origin, "query.template_reset_preview", {})["pristine"]:
            raise RuntimeError("The reset fixture did not register the custom definition")
    checkpoint_fixture(dest)
    write(args.output / "reset-seed.json", dataset_manifest(dest))
    return custom["id"]


def fg_reset_read(args, start, done, origin, index, reader, run):
    try:
        opener = client(origin)
    except OSError:
        start.abort()
        raise
    start.wait(timeout=20)
    iteration = 0
    while not done.is_set():
        method = ["query.workflow_contexts", "query.workflow_materials",
                  "query.workflow_executions", "query.workflow_management_summary"][reader % 4]
        scope = ({"id": args.root} if reader % 4 == 3 else {"owner_id": args.feature, "limit": 50})
        row, _ = rpc(opener, origin, method, {"project_id": args.project, **scope})
        append(args.output / "samples.jsonl", dict(
            row, case="read-during-reset:" + method, cohort="full", sample=index,
            reader=reader, iteration=iteration, host=run,
        ))
        iteration += 1


def reset_progress(args, start, done, origin, command, index, run):
    try:
        opener = client(origin)
    except OSError:
        start.abort()
        raise
    start.wait(timeout=20)
    begin = time.perf_counter()
    first = None
    while not done.is_set():
        sample, value = rpc(opener, origin, "query.template_reset_progress", {
            "command_id": command["command_id"],
        })
        if sample["error"]:
            raise RuntimeError(f'reset progress failed: {sample["error"]}')
        if value["phase"] != "idle":
            first = dict(sample, case="template-reset-first-progress", cohort="full",
                         ms=(time.perf_counter() - begin) * 1000, phase=value["phase"],
                         sample=index, host=run)
            break
        done.wait(timeout=0.02)
    if first is None:
        raise RuntimeError("No progress became visible before the complete reset response")
    append(args.output / "samples.jsonl", first)


def measured_reset(args, opener, origin, command, scenario, index, run):
    workers = args.reset_readers + int(args.track_reset_progress)
    if scenario != "full" or not workers:
        return rpc(opener, origin, "configuration.templates.reset", command)
    start = threading.Barrier(workers + 1)
    done = threading.Event()
    with concurrent.futures.ThreadPoolExecutor(max_workers=workers) as pool:
        tasks = [pool.submit(fg_reset_read, args, start, done, origin, index, reader, run)
                 for reader in range(args.reset_readers)]
        if args.track_reset_progress:
            tasks.append(pool.submit(reset_progress, args, start, done, origin, command, index, run))
        start.wait(timeout=20)
        try:
            return rpc(opener, origin, "configuration.templates.reset", command)
        finally:
            done.set()
            for task in tasks:
                task.result()


def reset_samples(args, seed, custom_id):
    samples = args.output / "samples.jsonl"
    for index in range(args.samples):
        for preserve in ((False,) if args.reset_full_only else (True, False)):
            name = "preserve-custom" if preserve else "full"
            run = f"reset-{name}-{index:03d}"
            with tempfile.TemporaryDirectory(dir=args.output) as temp:
                dataset = Path(temp) / "fixture"
                clone(seed, dataset)
                with host(args.host, dataset, args.output, run) as (origin, _, _):
                    opener = client(origin)
                    preview = required(opener, origin, "query.template_reset_preview", {})
                    params = reset_command(preview, preserve, index)
                    if preserve:
                        requests = [(name, params)]
                    else:
                        requests = [(name, params), ("replay", params), ("unchanged", None)]
                    original = None
                    for scenario, command in requests:
                        if command is None:
                            latest = required(opener, origin, "query.template_reset_preview", {})
                            command = reset_command(latest, False, index)
                        row, result = measured_reset(args, opener, origin, command, scenario, index, run)
                        if scenario in ("full", "replay") and args.track_reset_progress and result:
                            job = required(opener, origin, "query.template_reset_progress",
                                           {"command_id": command["command_id"]})
                            if (job["phase"] != "completed" or job["generation"] != result["generation"]
                                    or job["total"] != job["applied"]):
                                row["error"] = "reset-progress-incomplete"
                            row["job_target_count"] = job["total"]
                        generations = project_generations(dataset)
                        with connect(dataset / "global.sqlite3") as db:
                            custom_exists = db.execute(
                                "SELECT EXISTS(SELECT 1 FROM source_definitions WHERE reference=?)",
                                [custom_id],
                            ).fetchone()[0] == 1
                        if result:
                            row.update(custom_exists=custom_exists, generation=result.get("generation"), changed=result.get("changed"),
                                       database_generations=generations)
                            if scenario == "preserve-custom" and not custom_exists:
                                row["error"] = "preserve-custom-lost-definition"
                            if scenario == "full" and (custom_exists or not result.get("changed") or
                                any(n != result["generation"] for n in generations.values())):
                                row["error"] = "full-reset-generation"
                            if scenario == "replay" and result != original:
                                row["error"] = "replay-result-changed"
                            if scenario == "unchanged" and result.get("changed") is not False:
                                row["error"] = "reset-reported-changes"
                        if scenario == "full":
                            original = result
                        append(samples, dict(row, case="template-reset", cohort=scenario, sample=index, host=run))


def approval_samples(args, origin, opener):
    samples = args.output / "samples.jsonl"
    scope = {"project_id": args.project, "root_instance_id": args.approval_root}
    for index in range(args.samples):
        time.sleep(.12)
        row, value = rpc(opener, origin, "query.orchestration", scope)
        if not row["error"]:
            active = [d for d in value.get("decisions", [])
                      if d["current"] and d["data"]["status"] == "pending"]
            row["current_decisions"] = len(active)
            if not active:
                row["error"] = "approval-is-not-current"
        append(samples, dict(row, case="manager-pending-decision",
              cohort="warm-spaced", sample=index, host="read"))

        row, driver = rpc(opener, origin, "query.workflow_driver",
                          {"project_id": args.project, "id": args.approval_root})
        if not row["error"]:
            approvals = [item for item in driver.get("approvals", []) if item["human_required"]]
            row["approval_count"] = len(approvals)
            if not approvals or not all(
                isinstance(item.get("interaction", {}).get("materials"), list)
                and isinstance(item.get("interaction", {}).get("evidence"), list)
                for item in approvals
            ):
                row["error"] = "approval-evidence-missing"
        append(samples, dict(row, case="manager-pending-evidence",
              cohort="warm-spaced", sample=index, host="read"))
    subprocess.run(["node", str(Path(__file__).with_name("workbench-browser.mjs")),
                    origin, str(args.output), str(args.samples), args.project, args.feature,
                    "", args.approval_root], check=True)

def collect(args):
    traces = {}
    for log in sorted(args.output.glob("*.host.log")):
        name = log.name.removesuffix(".host.log")
        with log.open(errors="replace") as stream:
            for line in stream:
                if not line.startswith("{"):
                    continue
                try:
                    value = json.loads(line)
                except ValueError:
                    continue
                if value.get("type") not in ("performance", "sqlite_configuration"):
                    continue
                value["host"] = name
                append(args.output / "server.jsonl", value)
                if "request_id" in value:
                    traces[name, str(value["request_id"])] = value["measurement"]
    groups = {}
    for samples in [args.output / "samples.jsonl", args.output / "browser.jsonl"]:
        if not samples.exists():
            continue
        for line in samples.read_text().splitlines():
            row = json.loads(line)
            key = (row["case"], row["cohort"])
            measurement = traces.get((row.get("host"), row.get("request_id")), {})
            spans = measurement.get("spans", {})
            if "runtime.display_cache_hit" in spans:
                cache = "display-hit"
            elif "runtime.display_cache_miss" in spans:
                cache = "display-miss"
            else:
                cache = "display-ineligible"
            groups.setdefault(key, []).append((row, measurement, cache))
    report = {}
    for (case, cohort), rows in groups.items():
        report.setdefault(case, {})[cohort] = {
            **stats([row["ms"] for row, _, _ in rows]),
            "failures": sum(bool(row.get("error")) for row, _, _ in rows),
            "cache": {
                cache: stats([row["ms"] for row, _, state in rows if state == cache])
                for cache in sorted({state for _, _, state in rows})
            },
            "stages": {
                "/".join(path): stats([
                    item["total_ms"] for _, m, _ in rows for item in m.get("paths", [])
                    if tuple(item["path"]) == path
                ])
                for path in sorted({tuple(item["path"]) for _, m, _ in rows for item in m.get("paths", [])})
            },
        }
    write(args.output / "summary.json", report)
    return any(group["failures"] for case in report.values() for group in case.values())


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    for arg in ("host", "web", "dataset", "output"):
        parser.add_argument(f"--{arg}", type=lambda path: Path(path).resolve(), required=True)
    for arg in ("project", "feature", "root"):
        parser.add_argument(f"--{arg}", required=True)
    parser.add_argument("--switch-feature")
    parser.add_argument("--approval-root")
    parser.add_argument("--filter", default="workflow")
    parser.add_argument("--cohort", default="1x-reference")
    parser.add_argument("--browser-case", help="Repeat one named browser navigation")
    parser.add_argument("--rpc-case", action="append", help="Repeat selected named RPC cases")
    parser.add_argument("--freeze-source", action="store_true", help="Archive the source checkout in the private evidence directory")
    parser.add_argument("--samples", type=int, default=30)
    parser.add_argument("--reset-readers", type=int, choices=(0, 1, 4, 16), default=0)
    parser.add_argument("--track-reset-progress", action="store_true")
    parser.add_argument("--reset-full-only", action="store_true", help="Skip the preserve-custom cohort")
    parser.add_argument("--run", choices=("all", "read", "browser", "cold-host", "reset", "growth", "approval"), default="all")
    args = parser.parse_args()
    if args.run == "approval" and not args.approval_root:
        parser.error("--run approval requires --approval-root")
    if not 1 <= args.samples <= 1000:
        parser.error("--samples must be 1..1000. Use at least 30 for baseline acceptance")
    if args.output.is_relative_to(args.dataset) or args.dataset == (REPO / "data").resolve():
        parser.error("Use an isolated backup and a separate output directory")
    if not (args.dataset / ".devdesk-dataset").is_file():
        parser.error("The backup is missing its dataset marker")
    if any(path.is_file() or path.is_symlink()
           for name in ("credentials", "shared-cache") for path in (args.dataset / name).rglob("*")):
        parser.error("Provide a backup stripped of runtime credentials and shared caches")
    args.output.mkdir(mode=0o700)
    binary = args.output / "devdesk-host"
    shutil.copy2(args.host, binary)
    with contextlib.ExitStack() as stack:
        meta = identity(args)
        # Keep the exact checkout restorable after the optimization changes it.
        if args.freeze_source:
            snapshot = args.output / "source.tar"
            with tarfile.open(snapshot, "w", dereference=False) as bundle:
                for name, digest in meta["working_tree"].items():
                    if digest is not None:
                        bundle.add(REPO / name, arcname=name, recursive=False)
            meta["source_archive_sha256"] = sha(snapshot)
        meta["dataset"] = dataset_manifest(args.dataset)
        meta["binary_copy_sha256"] = sha(binary)
        write(args.output / "identity.json", meta)
        args.host = binary
        stack.callback(collect, args)
        if args.run in ("all", "read", "browser", "growth", "approval"):
            with tempfile.TemporaryDirectory(dir=args.output) as name:
                dataset = Path(name) / "fixture"
                clone(args.dataset, dataset)
                with host(binary, dataset, args.output, "read") as (origin, _, _):
                    opener = client(origin)
                    verify_web(args, origin, opener)
                    if args.run in ("all", "read", "growth"):
                        read_samples(args, origin, opener)
                    if args.run == "approval":
                        approval_samples(args, origin, opener)
                    if args.run in ("all", "browser"):
                        subprocess.run([
                            "node", str(Path(__file__).with_name("workbench-browser.mjs")),
                            origin, str(args.output), str(args.samples), args.project, args.feature,
                            args.switch_feature or "", "", args.browser_case or "",
                        ], check=True)
        if args.run in ("all", "cold-host"):
            for index in range(args.samples):
                name = f"cold-{index:03d}"
                with tempfile.TemporaryDirectory(dir=args.output) as temp:
                    try:
                        dataset = Path(temp) / "fixture"
                        clone(args.dataset, dataset)
                        with host(binary, dataset, args.output, name) as (origin, startup, _):
                            append(args.output / "samples.jsonl", {"case": "host-startup", "cohort": "cold-process",
                                   "sample": index, "ms": startup, "error": None})
                            row, _ = rpc(client(origin), origin, "query.workflow_instance",
                                         {"project_id": args.project, "id": args.root, "view": "status"})
                            append(args.output / "samples.jsonl", dict(row, case="workflow-status", cohort="cold-process",
                                   sample=index, host=name))
                    except (OSError, RuntimeError) as exc:
                        append(args.output / "samples.jsonl", {"case": "host-startup", "cohort": "cold-process",
                               "sample": index, "ms": 60000, "error": type(exc).__name__})
        if args.run in ("all", "reset"):
            with tempfile.TemporaryDirectory(dir=args.output) as temp:
                seed = Path(temp) / "reset-seed"
                custom = reset_seed(args, seed)
                reset_samples(args, seed, custom)
    report = json.loads((args.output / "summary.json").read_text())
    if any(group["failures"] for case in report.values() for group in case.values()):
        raise SystemExit("At least one benchmark sample failed. See summary.json")


if __name__ == "__main__":
    main()

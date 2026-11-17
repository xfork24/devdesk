#!/usr/bin/env python3
"""Opt-in local HTTP/SSE regression with 100 simulated clients and four control roots.

Starts only the opt-in Host test fixture in a new empty directory. No user data,
models, GitHub or containers are accessed. This is not browser or formal P6 evidence.
"""
import argparse
from concurrent.futures import ThreadPoolExecutor
import hashlib
import json
import math
import os
from pathlib import Path
import select
import shutil
import signal
import sqlite3
import subprocess
import threading
import time
import urllib.error
import urllib.parse
import urllib.request


def percentile(values, fraction):
    return sorted(values)[max(0, math.ceil(len(values) * fraction) - 1)] if values else None


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--test-binary", type=Path, required=True)
    parser.add_argument("--output", type=Path, required=True)
    parser.add_argument("--seconds", type=int, default=30)
    args = parser.parse_args()
    if not 10 <= args.seconds <= 120:
        parser.error("seconds must be 10..120")
    args.output.mkdir(parents=True, exist_ok=False)
    output = args.output.resolve()
    binary = output / "host-tests"
    shutil.copy2(args.test_binary, binary)
    binary.chmod(0o700)
    dataset = output / "dataset"
    dataset.mkdir()
    (output / ".active-frontends-fixture").touch()
    environment = {**os.environ, "DEVDESK_ACTIVE_FRONTEND_DATASET": str(dataset)}
    environment.pop("DEVDESK_ACCESS_TOKEN", None)
    rows, events, failures = [], [], []
    stopping = threading.Event()
    streams = []
    with (output / "server.log").open("w") as log:
        server = subprocess.Popen(
            [str(binary), "active_frontend_performance::active_frontend_server", "--exact", "--ignored", "--nocapture"],
            env=environment, stdout=subprocess.PIPE, stderr=log,
        )
        try:
            fixture = None
            deadline = time.monotonic() + 90
            pending = ""
            while time.monotonic() < deadline and server.poll() is None:
                if select.select([server.stdout], [], [], 0.1)[0]:
                    chunk = os.read(server.stdout.fileno(), 65536).decode()
                    log.write(chunk)
                    log.flush()
                    pending += chunk
                    while "\n" in pending:
                        line, pending = pending.split("\n", 1)
                        if line.startswith("ACTIVE_FRONTEND_FIXTURE="):
                            fixture = json.loads(line.split("=", 1)[1])
                            break
                    if fixture:
                        break
            if not fixture:
                raise RuntimeError("isolated fixture failed to start; inspect server.log")
            origin = fixture["origin"]
            with urllib.request.urlopen(origin, timeout=10) as response:
                cookie = response.headers["Set-Cookie"].split(";", 1)[0]

            def rpc(method, params, label, client=-1):
                began = time.monotonic()
                request = urllib.request.Request(origin + "/rpc", json.dumps({"jsonrpc": "2.0", "id": client, "method": method, "params": params}).encode(), {"Content-Type": "application/json", "Cookie": cookie})
                try:
                    with urllib.request.urlopen(request, timeout=10) as response:
                        body = response.read()
                        value = json.loads(body)
                    error = value.get("error")
                    row = {"label": label, "client": client, "method": method, "ms": (time.monotonic() - began) * 1000, "bytes": len(body), "error": error}
                    rows.append(row)
                    if error and error.get("data", {}).get("code") != "revision_conflict":
                        failures.append(row)
                    return value
                except Exception as error:
                    row = {"label": label, "method": method, "client": client, "exception": repr(error), "ms": (time.monotonic() - began) * 1000}
                    rows.append(row)
                    failures.append(row)
                    raise

            def progress():
                return [rpc("query.workflow_progress", root, "progress")["result"] for root in fixture["roots"]]

            def event_client(client):
                project = fixture["contexts"][client]["project_id"]
                request = urllib.request.Request(origin + "/events?" + urllib.parse.urlencode({"project_id": project, "tail": "true"}), headers={"Cookie": cookie})
                try:
                    response = urllib.request.urlopen(request, timeout=5)
                    streams.append(response)
                    while not stopping.is_set():
                        line = response.readline().decode()
                        if not line:
                            break
                        if line.startswith("id:"):
                            event_id = line[3:].strip()
                            if not event_id.startswith(project + ":"):
                                failures.append({"client": client, "foreign_event": event_id})
                            events.append({"client": client, "time": time.monotonic(), "id": event_id})
                except Exception as error:
                    if not stopping.is_set():
                        failures.append({"client": client, "sse_error": repr(error)})

            subscribers = [threading.Thread(target=event_client, args=(client,), daemon=True) for client in range(100)]
            for subscriber in subscribers:
                subscriber.start()
            deadline = time.monotonic() + 10
            while len(streams) < 100 and time.monotonic() < deadline:
                time.sleep(0.05)
            if len(streams) != 100:
                raise RuntimeError("100 SSE clients did not subscribe")
            before = progress()
            start = time.monotonic()
            barrier = threading.Barrier(100)

            def active_client(client):
                context = fixture["contexts"][client]
                revision = context["revision"]
                barrier.wait()
                iteration = 0
                while time.monotonic() - start < args.seconds:
                    # Every fifth read switches this client's project; writes
                    # retain the original context and its independent revision.
                    project_index = (client // 50 + iteration // 5) % 2
                    root = fixture["roots"][project_index * 2 + client % 2]
                    method = ["query.execution_workspace", "query.workflow_progress", "query.workspace"][iteration % 3]
                    params = root if method == "query.workflow_progress" else {"project_id": root["project_id"]}
                    value = rpc(method, params, "initial_read" if iteration == 0 else "steady_read", client)
                    if "result" not in value:
                        failures.append({"client": client, "read_error": value})
                    elif method == "query.workflow_progress":
                        if value["result"]["record"]["id"] != root["id"]:
                            failures.append({"client": client, "wrong_root": value})
                    elif value["result"]["active_project"]["id"] != root["project_id"]:
                        failures.append({"client": client, "wrong_project": value})
                    if iteration % 10 == client % 10:
                        result = rpc("context.update", {"project_id": context["project_id"], "command_id": f"client-{client}-write-{iteration}", "target": context["id"], "expected_revision": revision, "payload": {"content": f"Client {client}, iteration {iteration}"}}, "steady_write", client)
                        if "error" in result:
                            failures.append({"client": client, "independent_write_error": result})
                        else:
                            revision += 1
                    iteration += 1
                    stopping.wait(max(0, start + iteration + client / 100 - time.monotonic()))
                return revision

            with ThreadPoolExecutor(max_workers=100) as pool:
                revisions = list(pool.map(active_client, range(100)))
            after = progress()
            with ThreadPoolExecutor(max_workers=100) as pool:
                burst_start = time.monotonic()
                reads = list(pool.map(lambda client: rpc("query.workflow_progress", fixture["roots"][0], "read_burst", client), range(100)))
                read_burst_ms = (time.monotonic() - burst_start) * 1000
                burst_start = time.monotonic()
                writes = list(pool.map(lambda client: rpc("context.update", {"project_id": fixture["contexts"][client]["project_id"], "command_id": f"independent-burst-{client}", "target": fixture["contexts"][client]["id"], "expected_revision": revisions[client], "payload": {"content": f"Independent final {client}"}}, "independent_write_burst", client), range(100)))
                write_burst_ms = (time.monotonic() - burst_start) * 1000
                context = fixture["contexts"][0]
                burst_start = time.monotonic()
                conflicts = list(pool.map(lambda client: rpc("context.update", {"project_id": context["project_id"], "command_id": f"same-version-burst-{client}", "target": context["id"], "expected_revision": revisions[0] + 1, "payload": {"content": f"Original version contender {client}"}}, "conflict_burst", client), range(100)))
            conflict_burst_ms = (time.monotonic() - burst_start) * 1000
            wins = sum("result" in item for item in conflicts)
            stale = sum(item.get("error", {}).get("data", {}).get("code") == "revision_conflict" for item in conflicts)
            if wins != 1 or stale != 99:
                failures.append({"conflict_winners": wins, "stale": stale})
            if any("error" in value for value in reads + writes):
                failures.append({"burst_error": True})
            advances = [end["counts"]["completed"] - begin["counts"]["completed"] for begin, end in zip(before, after)]
            if any(count <= 0 for count in advances):
                failures.append({"background_did_not_advance": advances})
            if len({event["client"] for event in events}) != 100:
                failures.append({"clients_with_events": len({event["client"] for event in events})})
            summary = {"clients": 100, "sse_clients": len(streams), "seconds": args.seconds, "scheduler": True, "workflow_completed_step_advances": advances, "read_burst_ms": read_burst_ms, "independent_write_burst_ms": write_burst_ms, "conflict_winners": wins, "conflict_rejections": stale, "failures": failures, "formal_p6": False, "limitations": ["Same host generator and server", "Control-only workflows; no provider/W2 effects", "HTTP/SSE simulated clients; no 100-browser rendering/navigation or final joint acceptance"]}
            for label in ["initial_read", "steady_read", "steady_write", "read_burst", "independent_write_burst", "conflict_burst"]:
                values = [row["ms"] for row in rows if row["label"] == label]
                summary[label] = {"count": len(values), "p95_ms": percentile(values, .95), "p99_ms": percentile(values, .99), "max_ms": max(values) if values else None}
            summary["latency_gates"] = {
                "steady_read_p95_200ms": summary["steady_read"]["p95_ms"] <= 200,
                "steady_read_p99_500ms": summary["steady_read"]["p99_ms"] <= 500,
                "steady_write_p95_300ms": summary["steady_write"]["p95_ms"] <= 300,
                "steady_write_p99_1000ms": summary["steady_write"]["p99_ms"] <= 1000,
                "read_burst_1000ms": read_burst_ms <= 1000,
                "independent_write_burst_2000ms": write_burst_ms <= 2000,
                "conflict_burst_2000ms": conflict_burst_ms <= 2000,
            }
            for project in fixture["projects"]:
                db = dataset / "projects" / project / "engineering.sqlite3"
                with sqlite3.connect(db.as_uri() + "?mode=ro", uri=True) as connection:
                    actual = dict(connection.execute("SELECT id,revision FROM contexts"))
                for client, context in enumerate(fixture["contexts"]):
                    if context["project_id"] == project:
                        expected = revisions[client] + 1 + (1 if client == 0 else 0)
                        if actual.get(context["id"]) != expected:
                            failures.append({"client": client, "expected_revision": expected, "actual_revision": actual.get(context["id"])})
                    elif context["id"] in actual:
                        failures.append({"client": client, "foreign_context_in_project": project})
            if any(item["record"]["data"]["status"] in ("completed", "cancelled") for item in after):
                failures.append({"workload_drained_before_measurement_end": True})
            summary["functional_pass"] = not failures
            summary["latency_pass"] = all(summary["latency_gates"].values())
            summary["workflows_still_active"] = all(item["record"]["data"]["status"] not in ("completed", "cancelled") for item in after)
            summary["identity"] = {"binary_sha256": hashlib.sha256(binary.read_bytes()).hexdigest(), "dataset": str(dataset), "fixture": fixture}
            (output / "summary.json").write_text(json.dumps(summary, indent=2))
            (output / "rpc.jsonl").write_text("".join(json.dumps(row) + "\n" for row in rows))
            (output / "events.jsonl").write_text("".join(json.dumps(event) + "\n" for event in events))
            print(json.dumps({key: value for key, value in summary.items() if key != "identity"}, indent=2))
            if failures or not summary["latency_pass"]:
                raise RuntimeError("local regression failed; evidence retained")
        finally:
            stopping.set()
            for response in streams:
                response.close()
            if server.poll() is None:
                server.send_signal(signal.SIGINT)
                try:
                    server.wait(timeout=15)
                except subprocess.TimeoutExpired:
                    server.kill()
                    server.wait()


if __name__ == "__main__":
    main()

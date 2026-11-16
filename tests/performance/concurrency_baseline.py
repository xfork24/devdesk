#!/usr/bin/env python3
"""Profile concurrent RPC reads on a marked backup without starting real execution."""
import argparse
from concurrent.futures import ThreadPoolExecutor
import hashlib
import http.cookiejar
import json
import os
from pathlib import Path
import re
import select
import shutil
import signal
import sqlite3
import subprocess
import time
import urllib.request

from configuration_loading import copy_dataset


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--test-binary", type=Path, required=True)
    parser.add_argument("--dataset", type=Path, required=True)
    parser.add_argument("--output", type=Path, required=True)
    parser.add_argument("--clients", default="1,2,10")
    parser.add_argument("--samples", type=int, default=3)
    args = parser.parse_args()
    clients = [int(item) for item in args.clients.split(",")]
    if not clients or any(n < 1 or n > 100 for n in clients) or not 1 <= args.samples <= 100:
        parser.error("clients must be 1..100 and samples 1..100")
    args.output.mkdir(parents=True, exist_ok=False)
    output = args.output.resolve()
    binary = output / "host-tests"
    shutil.copy2(args.test_binary, binary)
    binary.chmod(0o700)
    dataset = output / "dataset"
    copy_dataset(args.dataset.resolve(), dataset)
    metadata = {
        "binary_sha256": hashlib.sha256(binary.read_bytes()).hexdigest(),
        "source_dataset": str(args.dataset.resolve()),
        "offline": True,
        "scheduler": False,
        "formal_100_frontend_acceptance": False,
        "clients": clients,
        "samples": args.samples,
    }
    (output / "identity.json").write_text(json.dumps(metadata, indent=2))
    environment = {**os.environ, "DEVDESK_PERF_DATASET": str(dataset), "DEVDESK_PERF_LOG": "1"}
    environment.pop("DEVDESK_ACCESS_TOKEN", None)
    with (output / "server.log").open("w") as log:
        server = subprocess.Popen(
            [str(binary), "loading_performance::configuration_load_server", "--ignored", "--exact", "--nocapture"],
            env=environment, stdout=subprocess.PIPE, stderr=log,
        )
        try:
            origin = None
            deadline = time.monotonic() + 60
            while time.monotonic() < deadline:
                ready, _, _ = select.select([server.stdout], [], [], 0.1)
                if ready:
                    chunk = os.read(server.stdout.fileno(), 65536).decode(errors="replace")
                    log.write(chunk)
                    log.flush()
                    match = re.search(r"PERF_ORIGIN=(\S+)", chunk)
                    if match:
                        origin = match.group(1)
                        break
                if server.poll() is not None:
                    break
            if not origin:
                raise RuntimeError("offline benchmark server did not start")
            jar = http.cookiejar.CookieJar()
            client = urllib.request.build_opener(urllib.request.HTTPCookieProcessor(jar))
            client.open(origin + "/", timeout=10).read()
            cookie = "; ".join(f"{item.name}={item.value}" for item in jar)
            with sqlite3.connect(dataset / "global.sqlite3") as conn:
                row = conn.execute("SELECT value FROM settings WHERE key='active_project'").fetchone()
                if not row:
                    raise RuntimeError("baseline dataset needs a selected project")
                project = json.loads(row[0])
            with sqlite3.connect(dataset / "projects" / project / "engineering.sqlite3") as conn:
                feature = conn.execute("SELECT id FROM features ORDER BY rowid DESC LIMIT 1").fetchone()
            params = {"project_id": project, "limit": 50}
            if feature:
                params["feature_id"] = feature[0]

            def request(number, submitted):
                started = time.monotonic()
                body = {"jsonrpc": "2.0", "id": number, "method": "query.workspace", "params": params}
                req = urllib.request.Request(origin + "/rpc", json.dumps(body).encode(), {"Content-Type": "application/json", "Cookie": cookie})
                with urllib.request.urlopen(req, timeout=600) as response:
                    raw = response.read()
                    value = json.loads(raw)
                    return {
                        "scheduled_to_finished_ms": (time.monotonic() - submitted) * 1000,
                        "request_ms": (time.monotonic() - started) * 1000,
                        "queue_ms": (started - submitted) * 1000,
                        "bytes": len(raw),
                        "server_timing": response.headers.get("Server-Timing"),
                        "error": value.get("error"),
                    }

            rows = []
            with (output / "rpc.jsonl").open("w") as samples:
                for count in clients:
                    for sample in range(args.samples):
                        submitted = time.monotonic()
                        with ThreadPoolExecutor(max_workers=count) as pool:
                            futures = [pool.submit(request, number, submitted) for number in range(count)]
                            cohort = [future.result() for future in futures]
                        row = {"clients": count, "sample": sample, "wall_ms": (time.monotonic() - submitted) * 1000, "requests": cohort}
                        rows.append(row)
                        samples.write(json.dumps(row) + "\n")
                        samples.flush()
                        print(json.dumps({"clients": count, "sample": sample, "wall_ms": row["wall_ms"], "errors": sum(bool(item["error"]) for item in cohort)}), flush=True)
            if any(item["error"] for row in rows for item in row["requests"]):
                raise RuntimeError("RPC errors were recorded")
        finally:
            if server.poll() is None:
                server.send_signal(signal.SIGINT)
                try:
                    server.wait(timeout=10)
                except subprocess.TimeoutExpired:
                    server.kill()
                    server.wait()


if __name__ == "__main__":
    main()

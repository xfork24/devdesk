#!/usr/bin/env python3
"""Real HTTP lifecycle CAS, replay and old-instance rejection on disposable data."""
from concurrent.futures import ThreadPoolExecutor
import http.client
import http.cookiejar
import json
from pathlib import Path
import os
import subprocess
import sys
import tempfile
import time
import urllib.error
import urllib.request
import urllib.parse
import uuid


def run():
    binary = Path(sys.argv[1]).resolve()
    environment = dict(os.environ)
    environment.pop("DEVDESK_ACCESS_TOKEN", None)
    with tempfile.TemporaryDirectory(prefix="devdesk-lifecycle-") as directory:
        root = Path(directory).resolve()

        def start(dataset=None):
            args = [str(binary), "--port", "0"]
            if dataset:
                args += ["--data", str(dataset)]
            child = subprocess.Popen(args, cwd=root, env=environment, stdout=subprocess.PIPE,
                                     stderr=subprocess.PIPE, text=True)
            child.stdout.readline()
            origin = child.stdout.readline().strip()
            assert origin.startswith("http://127.0.0.1:"), origin
            cookies = http.cookiejar.CookieJar()
            opener = urllib.request.build_opener(urllib.request.ProxyHandler({}),
                urllib.request.HTTPCookieProcessor(cookies))
            opener.open(urllib.request.Request(origin + "/session", b"{}",
                {"Content-Type": "application/json"}), timeout=10).read()
            cookie = "; ".join(f"{item.name}={item.value}" for item in cookies)

            def request(path, body=None):
                req = urllib.request.Request(origin + path,
                    json.dumps(body).encode() if body is not None else None,
                    {"Content-Type": "application/json", "Cookie": cookie})
                try:
                    response = urllib.request.build_opener(urllib.request.ProxyHandler({})).open(req, timeout=30)
                except urllib.error.HTTPError as error:
                    response = error
                with response:
                    return response.status, json.load(response)
            request.origin = origin
            request.cookie = cookie
            return child, request

        def command(snapshot):
            return {"command_id": str(uuid.uuid4()), "instance_id": snapshot["instance_id"],
                    "expected_revision": snapshot["revision"]}

        child, request = start()
        try:
            initial = request("/capabilities")[1]["lifecycle"]
            status, missing = request("/setup", {"command_id": "missing", "path": str(root / "data")})
            assert status == 400 and missing["code"] == "precondition_required", missing
            commands = [{**command(initial), "path": str(root / "data")} for _ in range(100)]
            read_samples = []
            with ThreadPoolExecutor(max_workers=100) as pool:
                futures = [pool.submit(request, "/setup", body) for body in commands]
                while any(not future.done() for future in futures):
                    started = time.monotonic()
                    status, view = request("/capabilities")
                    read_samples.append((time.monotonic() - started) * 1000)
                    assert status == 200 and view["lifecycle"]["status"] in ("uninitialized", "initializing", "ready")
                    time.sleep(.01)
                results = [future.result() for future in futures]
            winners = [i for i, (status, body) in enumerate(results) if status == 200 and "root" in body]
            assert len(winners) == 1, results
            assert all(status == 409 and body["code"] == "revision_conflict"
                       for i, (status, body) in enumerate(results) if i not in winners), results
            winner = winners[0]
            assert request("/setup", commands[winner]) == results[winner]
            changed = {**commands[winner], "path": str(root / "different")}
            assert request("/setup", changed)[1]["code"] == "invalid_params"
            assert not (root / "different").exists()
            ready = request("/capabilities")[1]
            assert ready["setup_required"] is False and ready["lifecycle"]["status"] == "ready"
            # The capability catalog answers what a domain package may require.
            assert ready["domain_protocol"] == 2 and ready["package_format"] == 2
            groups = {f"{group['id']}/{group['version']}": group for group in ready["extensions"]}
            assert set(groups) == {"engineering.git/1", "engineering.tests/1"}
            assert "engineering.git.snapshot" in groups["engineering.git/1"]["core_inputs"]
            assert groups["engineering.tests/1"]["hooks"] == ["test_discovery"]
            assert {"workflow.start", "tests.run"} <= set(ready["platform_handlers"])
            assert "query.domain.active" in ready["rpc"]
            stale = command(initial)
            assert request("/shutdown", stale)[1]["code"] == "revision_conflict"
            assert request("/capabilities")[0] == 200
            old_shutdown = command(ready["lifecycle"])
            assert request("/shutdown", old_shutdown)[1]["ok"]
            assert child.wait(timeout=15) == 0
            child.stdout.close()
            child.stderr.close()
            child, request = start(root / "data")
            assert request("/shutdown", old_shutdown)[1]["code"] == "revision_conflict"
            assert request("/setup", commands[winner])[1]["code"] == "revision_conflict"
            assert request("/capabilities")[0] == 200
            current = request("/capabilities")[1]["lifecycle"]
            assert current["instance_id"] != initial["instance_id"]
            assert request("/shutdown", command(current))[1]["ok"]
            assert child.wait(timeout=15) == 0
            child.stdout.close()
            child.stderr.close()
            child, request = start()
            foreign = root / "foreign"
            foreign.mkdir()
            (foreign / "retained.txt").write_text("preserve this directory")
            failed_command = {**command(request("/capabilities")[1]["lifecycle"]), "path":str(foreign)}
            failed = request("/setup", failed_command)
            assert failed[0] == 200 and "error" in failed[1], failed
            assert failed[1]["lifecycle"]["status"] == "uninitialized"
            assert request("/setup", failed_command) == failed
            assert (foreign / "retained.txt").read_text() == "preserve this directory"
            initial_disconnect = request("/capabilities")[1]["lifecycle"]
            disconnected_command = {**command(initial_disconnect), "path": str(root / "disconnect")}
            connection = http.client.HTTPConnection(urllib.parse.urlsplit(request.origin).netloc, timeout=30)
            connection.request("POST", "/setup", json.dumps(disconnected_command),
                {"Content-Type":"application/json", "Cookie":request.cookie})
            deadline = time.monotonic() + 30
            while request("/capabilities")[1]["lifecycle"]["revision"] == initial_disconnect["revision"]:
                assert time.monotonic() < deadline, "setup intent was not admitted"
                time.sleep(.01)
            # Lose the response after admission. Completion belongs to the Host task.
            connection.close()
            while request("/capabilities")[1]["setup_required"]:
                assert time.monotonic() < deadline, "disconnected initialization did not complete"
                time.sleep(.01)
            status, recovered = request("/setup", disconnected_command)
            assert status == 200 and recovered["root"] == str(root / "disconnect"), recovered
            assert request("/shutdown", command(request("/capabilities")[1]["lifecycle"]))[1]["ok"]
            assert child.wait(timeout=15) == 0
            print(json.dumps({"lifecycle_winners": len(winners), "expected_conflicts": 99,
                "concurrent_capability_reads": len(read_samples),
                "max_capability_read_ms": max(read_samples, default=0),
                "old_instance_rejected": True, "disconnected_setup_recovered": True,
                "formal_100_frontend_acceptance": False}))
        finally:
            if child.poll() is None:
                child.terminate()
                child.wait(timeout=15)


if __name__ == "__main__":
    run()

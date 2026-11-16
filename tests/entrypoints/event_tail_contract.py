#!/usr/bin/env python3
"""Isolated HTTP tail fanout/reconnect checks; not workflow capacity acceptance."""
from concurrent.futures import ThreadPoolExecutor
import http.client
import http.cookiejar
import json
import os
from pathlib import Path
import subprocess
import sys
import tempfile
import time
import urllib.request
import uuid


def run():
    binary = Path(sys.argv[1]).resolve()
    with tempfile.TemporaryDirectory(prefix="devdesk-event-tail-") as directory:
        root = Path(directory)
        commands = root / "commands"
        commands.mkdir()
        for name in ["docker", "gh"]:
            command = commands / name
            command.write_text("#!/bin/sh\nexit 1\n")
            command.chmod(0o700)
        environment = {**os.environ, "PATH": str(commands) + os.pathsep + os.environ["PATH"]}
        environment.pop("DEVDESK_ACCESS_TOKEN", None)
        child = subprocess.Popen([str(binary), "--data", str(root / "dataset"), "--port", "0"],
            cwd=root, env=environment, stdout=subprocess.PIPE, stderr=subprocess.PIPE, text=True)
        streams = []
        try:
            child.stdout.readline()
            origin = child.stdout.readline().strip()
            assert origin.startswith("http://127.0.0.1:"), origin
            cookies = http.cookiejar.CookieJar()
            client = urllib.request.build_opener(urllib.request.ProxyHandler({}), urllib.request.HTTPCookieProcessor(cookies))
            client.open(urllib.request.Request(origin + "/session", b"{}", {"Content-Type": "application/json"}), timeout=10).read()
            cookie = "; ".join(f"{item.name}={item.value}" for item in cookies)

            def request(path, body=None):
                req = urllib.request.Request(origin + path, json.dumps(body).encode() if body is not None else None,
                    {"Content-Type": "application/json", "Cookie": cookie})
                with client.open(req, timeout=10) as response:
                    return json.load(response)

            def rpc(method, params):
                result = request("/rpc", {"jsonrpc": "2.0", "id": 1, "method": method, "params": params})
                assert "error" not in result, result
                return result

            def frame(response):
                value = {}
                while True:
                    raw = response.readline()
                    if not raw:
                        raise EOFError("event tail closed before expected frame")
                    line = raw.decode().strip()
                    if not line:
                        if value:
                            return value
                        continue
                    if line.startswith("id:"):
                        value["id"] = line[3:].strip()
                    elif line.startswith("data:"):
                        value["data"] = json.loads(line[5:].strip())

            def connect(project=None, cursor=None):
                path = "/events?tail=true&configuration=true"
                if project:
                    path += "&project_id=" + project
                headers = {"Cookie": cookie}
                if cursor:
                    headers["Last-Event-ID"] = cursor
                connection = http.client.HTTPConnection(origin.removeprefix("http://"), timeout=5)
                connection.request("GET", path, headers=headers)
                response = connection.getresponse()
                assert response.status == 200
                streams.append((connection, response))
                return response

            global_streams = [connect() for _ in range(100)]
            for response in global_streams:
                ready = frame(response)
                assert ready["data"]["kind"] == "stream.ready" and ready["data"]["project_id"] == "global", ready
            guard = next(v for v in rpc("query.settings", {})["result"]["versions"] if v["resource"]["id"] == "global_concurrency")

            def save(number):
                nonlocal guard
                result = rpc("settings.update", {"command_id": str(uuid.uuid4()), "preconditions": [guard], "payload": {"global_concurrency": 2 + number % 2}})
                guard = result["meta"]["commit"]["versions"][0]
                return result["meta"]["commit"]["cursor"]

            cursor = save(0)
            def observed(response):
                while True:
                    value = frame(response)
                    if value.get("id") == f"global:{cursor}":
                        assert value["data"]["target"] == "global_concurrency", value
                        return value
            with ThreadPoolExecutor(max_workers=100) as pool:
                results = list(pool.map(observed, global_streams))
            assert len(results) == 100
            resumed = connect(cursor=f"global:{cursor}")
            assert frame(resumed)["data"]["kind"] == "stream.ready"
            next_cursor = save(1)
            value = frame(resumed)
            assert value["id"] == f"global:{next_cursor}", value
            # Force an explicit missed-window recovery, without a historical SQL
            # replay for each client. Canonical state is read after the reset.
            for number in range(520):
                next_cursor = save(number)
            while frame(resumed).get("id") != f"global:{next_cursor}":
                pass
            reset = frame(connect(cursor=f"global:{cursor}"))
            assert reset["data"]["kind"] == "stream.reset" and reset["id"] == f"global:{next_cursor}", reset
            assert rpc("query.settings", {})["result"]["global_concurrency"] == 3
            future = frame(connect(cursor="global:18446744073709551615"))
            assert future["data"]["kind"] == "stream.reset", future
            project_a = rpc("project.create", {"command_id": "tail-project-a", "payload": {"title": "A"}})["result"]
            project_b = rpc("project.create", {"command_id": "tail-project-b", "payload": {"title": "B"}})["result"]
            a = connect(project_a["id"], f'{project_b["id"]}:999999')
            b = connect(project_b["id"])
            assert frame(a)["data"]["kind"] == "stream.ready"
            assert frame(b)["data"]["kind"] == "stream.ready"
            project_b = rpc("query.execution_workspace", {"project_id": project_b["id"]})["result"]["active_project"]
            rpc("project.update", {"project_id": project_b["id"], "target": project_b["id"], "command_id": "tail-update-b", "expected_revision": project_b["revision"], "payload": {"title": "B updated"}})
            while True:
                value = frame(b)
                if value.get("id"):
                    assert value["id"].startswith(project_b["id"] + ":")
                    break
            # Cross-project configuration notifications are allowed; persisted
            # project events must keep the pinned scope.
            next(connection for connection, response in streams if response is a).sock.settimeout(.3)
            try:
                while True:
                    value = frame(a)
                    assert "id" not in value, value
                    assert value["data"]["project_id"] == project_a["id"], value
            except TimeoutError:
                pass
            lifecycle = request("/capabilities")["lifecycle"]
            started = time.monotonic()
            request("/shutdown", {"command_id": "tail-shutdown", "instance_id": lifecycle["instance_id"], "expected_revision": lifecycle["revision"]})
            # Close client sockets so transport backpressure cannot retain them.
            for connection, response in streams:
                response.close()
                connection.close()
            streams.clear()
            assert child.wait(timeout=4) == 0
            print(json.dumps({"passed": True, "http_subscribers": 100, "burst_commits": 520,
                "shutdown_ms": (time.monotonic() - started) * 1000,
                "workflow_capacity_acceptance": False, "formal_capacity_acceptance": False}))
        finally:
            for connection, response in streams:
                response.close()
                connection.close()
            if child.poll() is None:
                child.terminate()
                child.wait(timeout=10)
            stderr = child.stderr.read()
            if child.returncode:
                print(stderr, file=sys.stderr)
            child.stdout.close()
            child.stderr.close()


if __name__ == "__main__":
    run()

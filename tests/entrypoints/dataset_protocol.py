#!/usr/bin/env python3
"""Empty datasets initialize; incompatible actual data is rejected without conversion."""
from pathlib import Path
import hashlib
import http.cookiejar
import json
import os
import sqlite3
import subprocess
import sys
import tempfile
import urllib.request

binary = Path(sys.argv[1] if len(sys.argv) > 1 else "target/debug/devdesk-host").resolve()
environment = dict(os.environ)
environment.pop("DEVDESK_ACCESS_TOKEN", None)

with tempfile.TemporaryDirectory(prefix="devdesk-protocol-") as temp:
    root = Path(temp)
    for index, (marker, version) in enumerate([
        ("devdesk-schema-v1\n", 14), ("devdesk-source-v2\n", 15), ("devdesk-source-flat-v1\n", 16), ("devdesk-source-v2-standard-v1\n", 16),
    ]):
        data = root / f"incompatible-{index}"
        data.mkdir()
        (data / ".devdesk-dataset").write_text(marker)
        db = data / "global.sqlite3"
        with sqlite3.connect(db) as conn:
            conn.executescript(f"CREATE TABLE sentinel(value TEXT); INSERT INTO sentinel VALUES ('retained'); PRAGMA user_version={version};")
        before = hashlib.sha256(db.read_bytes()).hexdigest()
        run = subprocess.run([str(binary), "--data", str(data), "--port", "0"],
                             env=environment, capture_output=True, text=True, timeout=15)
        assert run.returncode != 0, run.stdout
        expected = "数据集标记不受支持" if marker != "devdesk-source-flat-v1\n" else "数据集协议不兼容"
        assert expected in run.stdout + run.stderr, (run.stdout, run.stderr)
        assert hashlib.sha256(db.read_bytes()).hexdigest() == before, "unsupported data was changed"
        assert (data / ".devdesk-dataset").read_text() == marker

    for index, leftovers in enumerate([False, True]):
        data = root / f"empty-{index}"
        data.mkdir()
        if leftovers:
            (data / ".DS_Store").write_bytes(b"Finder metadata")
            (data / ".devdesk-dataset").write_text("devdesk-schema-v1\\n")
            (data / ".devdesk.lock").touch()
        for reopening in [False, True]:
            args = [str(binary), "--port", "0"]
            if reopening:
                args += ["--data", str(data)]
            child = subprocess.Popen(args, cwd=root, env=environment, stdout=subprocess.PIPE,
                                     stderr=subprocess.PIPE, text=True)
            try:
                child.stdout.readline()
                url = child.stdout.readline().strip()
                assert url.startswith("http://127.0.0.1:"), url
                client = urllib.request.build_opener(
                    urllib.request.ProxyHandler({}),
                    urllib.request.HTTPCookieProcessor(http.cookiejar.CookieJar()),
                )
                def post(path, body):
                    request = urllib.request.Request(url + path, json.dumps(body).encode(),
                                                     {"Content-Type": "application/json"})
                    return json.load(client.open(request, timeout=20))
                post("/session", {})
                if not reopening:
                    result = post("/setup", {"path": str(data)})
                    assert result == {"root": str(data.resolve())}, result
                capabilities = json.load(client.open(url + "/capabilities", timeout=10))
                assert capabilities["setup_required"] is False
                assert (data / ".devdesk-dataset").read_text() == "devdesk-source-flat-v1\n"
                with sqlite3.connect(data / "global.sqlite3") as conn:
                    assert conn.execute("PRAGMA user_version").fetchone()[0] == 17
                    assert conn.execute("SELECT count(*) FROM source_definitions").fetchone()[0] > 0
                post("/shutdown", {})
                assert child.wait(timeout=15) == 0
            finally:
                if child.poll() is None:
                    child.terminate()
                    child.wait(timeout=15)

print("PASS dataset initialization: empty directory, hidden remnants, Web setup, restart; incompatible actual data retained")

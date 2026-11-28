#!/usr/bin/env python3
"""Verify empty setup, incompatible-data preservation, and explicit global upgrades."""
from pathlib import Path
import fcntl
import hashlib
import http.cookiejar
import json
import os
import sqlite3
import subprocess
import sys
import tempfile
import urllib.request
import uuid

binary = Path(sys.argv[1] if len(sys.argv) > 1 else "target/debug/devdesk-host").resolve()
environment = dict(os.environ)
environment.pop("DEVDESK_ACCESS_TOKEN", None)

with tempfile.TemporaryDirectory(prefix="devdesk-protocol-") as temp:
    root = Path(temp)
    for index, (marker, version) in enumerate([
        ("devdesk-schema-v1\n", 14), ("devdesk-source-v2\n", 15), ("devdesk-source-flat-v1\n", 18), ("devdesk-profile-runtime-v2\n", 16), ("devdesk-profile-runtime-v2\n", 18), ("devdesk-profile-runtime-v2\n", 19), ("devdesk-profile-runtime-v2\n", 20), ("devdesk-profile-runtime-v2\n", 21), ("devdesk-source-v2-standard-v1\n", 16),
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
        expected = "数据集标记不受支持" if marker != "devdesk-profile-runtime-v2\n" else "数据集协议不兼容"
        assert expected in run.stdout + run.stderr, (run.stdout, run.stderr)
        assert hashlib.sha256(db.read_bytes()).hexdigest() == before, "unsupported data was changed"
        assert (data / ".devdesk-dataset").read_text() == marker

    def upgrade_global(data):
        return subprocess.run([str(binary), "storage", "upgrade-global", "--data", str(data)],
                              env=environment, capture_output=True, text=True, timeout=15)

    current_sql = (Path(__file__).resolve().parents[2] / "sql/global/schema.sql").read_text()
    old_sql = current_sql.replace("('codex','dsh','pi','mcode','zcode','opencode')",
                                  "('codex','dsh')").replace("user_version=25", "user_version=24")
    with sqlite3.connect(":memory:") as baseline:
        baseline.executescript(current_sql)
        expected_schema = baseline.execute(
            "SELECT name,sql FROM sqlite_schema WHERE tbl_name='runtime_installations' ORDER BY name"
        ).fetchall()

    for number in (0, 2):
        data = root / f"global-24-with-{number}-installations"
        data.mkdir()
        (data / ".devdesk-dataset").write_text("devdesk-profile-runtime-v2\n")
        db = data / "global.sqlite3"
        rows = [(f"runtime-{provider}", provider, "1", "linux", "x86_64", '{}', "fixture", "MIT", None)
                for provider in ("codex", "dsh")][:number]
        with sqlite3.connect(db) as conn:
            conn.execute("PRAGMA journal_mode=WAL")
            conn.executescript(old_sql)
            conn.executemany("INSERT INTO runtime_installations VALUES (?,?,?,?,?,?,?,?,?)", rows)
            conn.execute("INSERT INTO settings VALUES (?,?)", ("existing-setting", '"before-upgrade"'))
        original = subprocess.run([str(binary), "--port", "0", "--data", str(data)],
                                  env=environment, capture_output=True, text=True, timeout=15)
        assert original.returncode != 0 and "storage upgrade-global" in original.stderr + original.stdout
        with (data / ".devdesk.lock").open("a") as lock:
            fcntl.flock(lock, fcntl.LOCK_EX | fcntl.LOCK_NB)
            locked = upgrade_global(data)
            assert locked.returncode != 0 and "另一个 host" in locked.stderr + locked.stdout
        run = upgrade_global(data)
        assert run.returncode == 0, run.stderr
        report = json.loads(run.stdout)
        assert (report["source_version"], report["target_version"], report["runtime_installations"]) == (24, 25, number)
        backup = Path(report["backup_path"])
        assert not backup.is_relative_to(data) and (backup.stat().st_mode & 0o777) == 0o600
        with sqlite3.connect(backup) as saved:
            assert saved.execute("PRAGMA user_version").fetchone()[0] == 24
            assert saved.execute("SELECT * FROM runtime_installations ORDER BY id").fetchall() == rows
        with sqlite3.connect(db) as conn:
            assert conn.execute("PRAGMA user_version").fetchone()[0] == 25
            assert conn.execute("PRAGMA integrity_check").fetchone()[0] == "ok"
            assert conn.execute("SELECT * FROM runtime_installations ORDER BY id").fetchall() == rows
            assert conn.execute("SELECT value FROM settings WHERE key='existing-setting'").fetchone()[0] == '"before-upgrade"'
            assert conn.execute(
                "SELECT name,sql FROM sqlite_schema WHERE tbl_name='runtime_installations' ORDER BY name"
            ).fetchall() == expected_schema
        repeated = upgrade_global(data)
        assert repeated.returncode == 0 and json.loads(repeated.stdout) == {
            "changed": False, "source_version": 25, "target_version": 25,
        }
        if old_binary := os.environ.get("DEVDESK_V24_HOST"):
            old_run = subprocess.run([old_binary, "--data", str(data), "--port", "0"],
                                     env=environment, capture_output=True, text=True, timeout=15)
            assert old_run.returncode != 0 and "数据集协议不兼容" in old_run.stderr + old_run.stdout

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
                    if path in ('/setup','/shutdown'):
                        state = json.load(client.open(url+'/capabilities', timeout=10))['lifecycle']
                        body = {**body, 'command_id':str(uuid.uuid4()), 'instance_id':state['instance_id'], 'expected_revision':state['revision']}
                    request = urllib.request.Request(url + path, json.dumps(body).encode(),
                                                     {"Content-Type": "application/json"})
                    return json.load(client.open(request, timeout=20))
                post("/session", {})
                if not reopening:
                    result = post("/setup", {"path": str(data)})
                    assert result["root"] == str(data.resolve()), result
                capabilities = json.load(client.open(url + "/capabilities", timeout=10))
                assert capabilities["setup_required"] is False
                assert (data / ".devdesk-dataset").read_text() == "devdesk-profile-runtime-v2\n"
                with sqlite3.connect(data / "global.sqlite3") as conn:
                    assert conn.execute("PRAGMA user_version").fetchone()[0] == 25
                    assert conn.execute("SELECT count(*) FROM source_definitions").fetchone()[0] > 0
                post("/shutdown", {})
                assert child.wait(timeout=15) == 0
            finally:
                if child.poll() is None:
                    child.terminate()
                    child.wait(timeout=15)

print("PASS dataset initialization: empty directory, hidden remnants, Web setup, restart; incompatible data retained; explicit global 24 to 25 upgrade and idempotent replay")

#!/usr/bin/env python3
"""Unsupported dataset protocols are rejected without conversion or data loss."""
from pathlib import Path
import hashlib, sqlite3, subprocess, sys, tempfile
binary=Path(sys.argv[1] if len(sys.argv)>1 else "target/debug/devdesk-host").resolve()
with tempfile.TemporaryDirectory(prefix="devdesk-protocol-") as temp:
    root=Path(temp)
    for index,(marker,version) in enumerate([("devdesk-schema-v1\n",14),("devdesk-source-v2\n",14)]):
        data=root/f"dataset-{index}";data.mkdir()
        (data/".devdesk-dataset").write_text(marker)
        db=data/"global.sqlite3"
        with sqlite3.connect(db) as conn:conn.executescript(f"CREATE TABLE sentinel(value TEXT); INSERT INTO sentinel VALUES ('retained'); PRAGMA user_version={version};")
        before=hashlib.sha256(db.read_bytes()).hexdigest()
        run=subprocess.run([str(binary),"--data",str(data),"--port","0"],capture_output=True,text=True,timeout=15)
        assert run.returncode!=0,run.stdout
        expected="数据集标记不受支持" if marker=="devdesk-schema-v1\n" else "数据集协议不兼容"
        assert expected in run.stdout+run.stderr,(run.stdout,run.stderr)
        assert hashlib.sha256(db.read_bytes()).hexdigest()==before,"unsupported data was changed"
        assert not (data/"harness-v5-upgrade.json").exists()
print("PASS unsupported dataset protocols rejected without conversion")

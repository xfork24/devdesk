#!/usr/bin/env python3
"""Measure existing engineering data and extrapolate bounded request costs.

Both engineering databases are opened read-only. Write probes use a disposable
SQLite database and a private file on the same volume.
"""
import argparse
import hashlib
import json
import math
import os
from pathlib import Path
import sqlite3
import statistics
import tempfile
import time


def connection(path):
    return sqlite3.connect(path.resolve().as_uri() + "?mode=ro", uri=True)


def p95(samples):
    return round(sorted(samples)[math.ceil(len(samples) * .95) - 1], 3)


def inventory(db, path):
    kinds = dict(db.execute(
        "SELECT s.type,sum(d.pgsize) FROM dbstat d JOIN sqlite_schema s USING(name) GROUP BY 1"
    ))
    objects = path.parent / "payloads"
    version, = db.execute("PRAGMA user_version").fetchone()
    files = [p for p in objects.rglob("*") if p.is_file()] if version >= 61 else []
    return {
        "sqlite_bytes": path.stat().st_size,
        "table_bytes": kinds.get("table", 0),
        "index_bytes": kinds.get("index", 0),
        "wal_bytes": path.with_name(path.name + "-wal").stat().st_size
        if path.with_name(path.name + "-wal").exists() else 0,
        "payload_bytes": sum(p.stat().st_size for p in files),
        "payload_allocated_bytes": sum(512 * p.stat().st_blocks for p in files),
        "payload_files": len(files),
        "sqlite_page_bytes": db.execute("PRAGMA page_size").fetchone()[0],
        "artifact_allocated_bytes": sum(
            512 * p.stat().st_blocks for p in (path.parent / "artifacts").rglob("*") if p.is_file()
        ),
    }


def body(db, root, pointer):
    hash_value = pointer["sha256"]
    assert len(hash_value) == 64 and all(c in "0123456789abcdef" for c in hash_value)
    if "artifact" in pointer:
        relative, digest = db.execute(
            "SELECT relative_path,sha256 FROM artifacts WHERE id=?", (pointer["artifact"],)
        ).fetchone()
        path = (root / relative).resolve()
        assert path.is_relative_to(root.resolve() / "artifacts") and digest == hash_value
        raw = path.read_bytes()
    else:
        loose = root / "payloads" / "objects" / hash_value[:2] / hash_value[2:4] / hash_value
        if loose.is_file():
            raw = loose.read_bytes()
        else:
            name, offset, size = db.execute(
                "SELECT pack,offset,bytes FROM payload_pack_objects WHERE sha256=?", (hash_value,)
            ).fetchone()
            assert all(c in "0123456789abcdef-" for c in name)
            with (root / "payloads" / "packs" / (name + ".pack")).open("rb") as file:
                file.seek(offset)
                raw = file.read(size)
    assert len(raw) == pointer["bytes"] and hashlib.sha256(raw).hexdigest() == hash_value
    return raw


def decode(db, root, value, io):
    prefix = "\x1ddevdesk-payload/v1:"
    key = "$devdesk_project_payload_v1"
    if isinstance(value, dict) and key in value:
        raw = body(db, root, value[key])
        io[0] += 1
        io[1] += len(raw)
        return json.loads(raw)
    if isinstance(value, str) and value.startswith(prefix):
        size, sha = value[len(prefix):].split(":", 1)
        if size == "json":
            raw = decode(db, root, json.loads(sha), io)
            return json.dumps(raw, ensure_ascii=False, separators=(",", ":"), sort_keys=True)
        raw = body(db, root, {"sha256": sha, "bytes": int(size)})
        io[0] += 1
        io[1] += len(raw)
        return raw.decode()
    if isinstance(value, list):
        return [decode(db, root, item, io) for item in value]
    if isinstance(value, dict):
        return {k: decode(db, root, item, io) for k, item in value.items()}
    return value


def request_samples(path, queries, rounds):
    report = {}
    with connection(path) as warm:
        for name, (sql, params, shape) in queries.items():
            plans = [row[3] for row in warm.execute("EXPLAIN QUERY PLAN " + sql, params)]
            samples = {"warm_ms": [], "sqlite_cold_ms": []}
            for mode, samples_of_mode in samples.items():
                for repeat in range(rounds + 1):
                    db = warm if mode == "warm_ms" else connection(path)
                    io = [0, 0]
                    start = time.perf_counter_ns()
                    rows = db.execute(sql, params).fetchall()
                    if shape:
                        for (value,) in rows:
                            decode(db, path.parent, json.loads(value) if shape == "json" else value, io)
                    elapsed = (time.perf_counter_ns() - start) / 1e6
                    if repeat:
                        samples_of_mode.append(elapsed)
                    if db is not warm:
                        db.close()
            report[name] = {
                "p95": {k: p95(samples_of_mode) for k, samples_of_mode in samples.items()},
                "rows_returned": len(rows), "body_reads": io[0], "body_bytes": io[1],
                "query_plan": plans,
            }
    return report


def write_probe(parent, sample, rounds, page_size):
    with tempfile.TemporaryDirectory(prefix=".storage-io-", dir=parent) as directory:
        root = Path(directory)
        db_path = root / "probe.sqlite3"
        db = sqlite3.connect(db_path)
        db.execute(f"PRAGMA page_size={page_size}")
        db.executescript("""
            PRAGMA journal_mode=WAL;
            PRAGMA synchronous=FULL;
            PRAGMA wal_autocheckpoint=0;
            CREATE TABLE sample(id INTEGER PRIMARY KEY, state TEXT, body TEXT);
            CREATE INDEX by_state ON sample(state,id);
        """)
        start_wal = db_path.with_name("probe.sqlite3-wal").stat().st_size
        sync, commit = [], []
        for i in range(rounds):
            started = time.perf_counter_ns()
            destination = root / f"{i}.body"
            pending = root / f"{i}.pending"
            with pending.open("wb") as f:
                f.write(sample)
                f.flush()
                os.fsync(f.fileno())
            pending.rename(destination)
            directory_fd = os.open(root, os.O_RDONLY)
            try:
                os.fsync(directory_fd)
            finally:
                os.close(directory_fd)
            sync.append((time.perf_counter_ns() - started) / 1e6)
            started = time.perf_counter_ns()
            db.execute("INSERT INTO sample VALUES (?,?,?)", (i, "completed", "0" * 64))
            db.commit()
            commit.append((time.perf_counter_ns() - started) / 1e6)
        wal = db_path.with_name("probe.sqlite3-wal").stat().st_size - start_wal
        started = time.perf_counter_ns()
        db.execute("PRAGMA wal_checkpoint(TRUNCATE)").fetchone()
        checkpoint_ms = (time.perf_counter_ns() - started) / 1e6
        db.close()
    return {"body_bytes": len(sample), "publish_ms_p95": p95(sync),
            "sqlite_commit_ms_p95": p95(commit), "wal_bytes_per_commit": round(wal / rounds),
            "checkpoint_ms_for_probe": round(checkpoint_ms, 3)}


def index_model(db, multiplier):
    indexes = {}
    for index in ("sqlite_autoindex_features_1", "sqlite_autoindex_workflow_display_steps_1",
                  "idx_runtime_work_seek", "sqlite_autoindex_operations_1",
                  "idx_conversation_session_kind"):
        leaves, internal, keys, cells, payload = db.execute("""
            SELECT sum(pagetype='leaf'),sum(pagetype='internal'),
                   sum(CASE WHEN pagetype='internal' AND path!='/' THEN ncell+1 ELSE 0 END),
                   sum(CASE WHEN pagetype='leaf' THEN ncell ELSE 0 END),
                   sum(CASE WHEN pagetype='leaf' THEN payload ELSE 0 END)
            FROM dbstat WHERE name=?
        """, (index,)).fetchone()
        if not leaves:
            continue
        fanout = keys / (internal - 1) if internal > 1 else None
        page, = db.execute("PRAGMA page_size").fetchone()
        capacity = page / ((payload / max(1, cells)) + 8)
        fanouts = [max(2, capacity * occupancy) for occupancy in (.55, .8)]
        indexes[index] = {
            "measured_leaf_pages": leaves,
            "measured_non_root_fanout": round(fanout, 1) if fanout else None,
            "estimated_point_read_pages_fully_uncached": sorted(
                1 + math.ceil(math.log(max(1, leaves * multiplier), factor)) for factor in fanouts
            ),
        }
    return indexes


def verify(old, new, root):
    assert new.execute("PRAGMA integrity_check").fetchall() == [("ok",)]
    assert new.execute("PRAGMA foreign_key_check").fetchone() is None
    verified = {}
    for table, field in (("governance_records","data"),("operations","result"),
                         ("conversation_events","content"),("events","detail")):
        matched = 0
        for key, before in old.execute(f"SELECT id,{field} FROM {table}"):
            after, = new.execute(f"SELECT {field} FROM {table} WHERE id=?", (key,)).fetchone()
            if after != before:
                if table in ("governance_records","operations"):
                    assert decode(new, root, json.loads(after), [0,0]) == json.loads(before), (table,key)
                elif table == "events":
                    assert [{"resource":{"kind":kind,"id":rid},"revision":rev} for kind,rid,rev in json.loads(after)] == json.loads(before), (table,key)
                else:
                    assert decode(new, root, after, [0,0]) == before, (table,key)
            matched += 1
        assert new.execute(f"SELECT count(*) FROM {table}").fetchone() == (matched,)
        verified[table] = matched
    return verified


def main():
    parser = argparse.ArgumentParser()
    parser.add_argument("before", type=Path)
    parser.add_argument("after", type=Path)
    parser.add_argument("--rounds", type=int, default=40)
    parser.add_argument("--verify", action="store_true")
    args = parser.parse_args()
    args.before, args.after = args.before.resolve(), args.after.resolve()
    with connection(args.before) as old, connection(args.after) as new:
        before = inventory(old, args.before)
        after = inventory(new, args.after)
        fixed_tables = ("sqlite_schema","dataset_layout","source_definitions","source_heads","settings","projects")
        fixed = new.execute(
            "SELECT coalesce(sum(pgsize),0) FROM dbstat WHERE name IN (" + ",".join("?" for _ in fixed_tables) + ")",
            fixed_tables,
        ).fetchone()[0]
        workloads = old.execute("""
            SELECT f.id, count(DISTINCT s.id),count(DISTINCT e.turn_id),
                   count(e.id),coalesce(sum(length(CAST(e.content AS BLOB))),0)
            FROM features f LEFT JOIN sessions s ON s.feature_id=f.id
            LEFT JOIN conversation_events e ON e.session_id=s.id GROUP BY f.id
            ORDER BY 5
        """).fetchall()
        assert workloads and any(w[4] for w in workloads)
        rid = old.execute("SELECT instance_id FROM workflow_display_headers ORDER BY instance_id LIMIT 1").fetchone()[0]
        tool = old.execute("SELECT id FROM governance_records WHERE type='platform_call' ORDER BY length(data) DESC LIMIT 1").fetchone()[0]
        sid = old.execute("SELECT id FROM sessions ORDER BY rowid DESC LIMIT 1").fetchone()[0]
        receipt = old.execute("SELECT id FROM operations ORDER BY length(result) DESC LIMIT 1").fetchone()[0]
        queries = {
            "feature_page": ("SELECT id,status FROM features WHERE id>? ORDER BY id LIMIT 20", ("",), None),
            "workflow_status": ("SELECT data FROM workflow_display_steps WHERE instance_id=? ORDER BY execution_id LIMIT 20", (rid,), None),
            "ready_work": ("SELECT id FROM governance_records INDEXED BY idx_runtime_work_seek WHERE type='platform_call' AND json_extract(data,'$.status')='prepared' AND id>? ORDER BY id LIMIT 17", ("",), None),
            "tool_detail": ("SELECT data FROM governance_records WHERE id=?", (tool,), "json"),
            "conversation_page": ("SELECT content FROM conversation_events WHERE session_id=? ORDER BY sequence DESC LIMIT 20", (sid,), "text"),
            "replay_result": ("SELECT result FROM operations WHERE id=?", (receipt,), "json"),
        }
        fields = [w[4] for w in workloads]
        # Observed conversation activity is a sensitivity parameter, not a claim
        # that all project-scoped workflow bytes belong to that Feature.
        activity = [max(.25, x / statistics.mean(fields)) for x in
                    (statistics.median(fields), statistics.mean(fields), fields[-1])]
        capacity = []
        for n in (100_000, 1_000_000, 5_000_000):
            unit = n / len(workloads) / 1e12
            capacity.append({"features": n, "baseline_sqlite_TB": round(before["sqlite_bytes"] * unit, 3),
                "sqlite_TB_median_average_heaviest_activity": [round(fixed/1e12 + (after["sqlite_bytes"]-fixed)*unit*k,3) for k in activity],
                "payload_TB_median_average_heaviest_activity": [round(after["payload_bytes"] * unit * k, 3) for k in activity],
                "artifact_TB_average_activity": round(after["artifact_allocated_bytes"]*unit,3)})
        sample = (old.execute("SELECT CAST(content AS BLOB) FROM conversation_events WHERE kind='tool' AND length(content)>4096 ORDER BY length(content) LIMIT 1").fetchone() or (b"x"*4096,))[0]
        print(json.dumps({"before":before,"after":after,"feature_count":len(workloads),
            "features_sessions_turns_events_body_bytes":[w[1:] for w in workloads],
            "capacity":capacity,"estimated_project_fixed_sqlite_bytes":fixed,
            "million_feature_index_model":index_model(new, 1_000_000/len(workloads)),
            "before_queries":request_samples(args.before, queries,args.rounds),
            "after_queries":request_samples(args.after,queries,args.rounds),
            "small_write_probe_before":write_probe(args.after.parent, sample,args.rounds,before["sqlite_page_bytes"]),
            "small_write_probe_after":write_probe(args.after.parent, sample,args.rounds,after["sqlite_page_bytes"]),
            "verified_rows":verify(old, new, args.after.parent) if args.verify else None}, indent=2))


if __name__ == "__main__":
    main()

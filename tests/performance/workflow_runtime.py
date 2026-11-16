#!/usr/bin/env python3
"""Explicit actual Codex/Host workflow run; separates native transport and durable receipt timings.

Uses a new dataset, not a copy of a business Feature. Requires an explicitly supplied
authentication source for this real-model scene; it is never a pressure-test credential fixture.
"""
import argparse
import hashlib
import json
import math
import os
from pathlib import Path
import sqlite3
import subprocess
import sys


def percentile(values, quantile):
    values = sorted(values)
    return values[max(0, math.ceil(len(values) * quantile) - 1)] if values else None


def summarize(values):
    return {"count": len(values), "p50_ms": percentile(values, .5),
            "p95_ms": percentile(values, .95), "p99_ms": percentile(values, .99),
            "max_ms": max(values) if values else None}


def read_timings(directory):
    events = []
    for line in (directory / "host.log").read_text().splitlines():
        if line.startswith("{"):
            item = json.loads(line)
            if item.get("type") == "native_protocol":
                events.append(item)
    requests = {(e["operation_id"], e.get("sequence", e["request_key"])): e for e in events
                if e["phase"] == "request_observed"}
    replies = {(e["operation_id"], e.get("sequence", e["request_key"])): e for e in events
               if e["phase"] == "reply_written"}
    methods = {}
    for journal in (directory / "data").rglob("*.jsonl"):
        if journal.stem not in {key[0] for key in requests}:
            continue
        for line in journal.read_text().splitlines():
            entry = json.loads(line)
            frame = entry.get("frame", {})
            if entry.get("direction") != "in" or frame.get("method") not in ("item/tool/call", "devdesk/tool/call"):
                continue
            key = hashlib.sha256(json.dumps(frame["id"], ensure_ascii=False, separators=(",", ":")).encode()).hexdigest()
            params = frame.get("params", {})
            methods[(journal.stem, key)] = params.get("tool", params.get("name", "unknown"))
    transport = [{**reply, "method": methods.get((reply["operation_id"], reply["request_key"]), "unknown")}
                 for key, reply in replies.items()]
    durable = []
    actions = []
    for database in (directory / "data/projects").glob("*/engineering.sqlite3"):
        with sqlite3.connect(database.as_uri() + "?mode=ro", uri=True) as db:
            for call_id, body, created in db.execute("SELECT id,data,created_at FROM governance_records WHERE type='controller_tool_call'"):
                data = json.loads(body)
                row = db.execute("SELECT created_at FROM governance_records WHERE type='controller_tool_reply' AND definition_id=?", (call_id,)).fetchone()
                request_key = hashlib.sha256(json.dumps(data['request_id'], ensure_ascii=False, separators=(',', ':')).encode()).hexdigest()
                durable.append({"call_id": call_id, "method": data["method"], "status": data["status"],
                                "operation_id": data['worker_operation_id'], "request_key": request_key,
                                "registered_at_ms": created, "result_record_at_ms": data.get('finished_at'),
                                "registered_to_result_ms": data.get("finished_at", created) - created,
                                "registered_to_reply_record_ms": row[0] - created if row else None})
            active = db.execute("SELECT count(*) FROM turns WHERE status IN ('queued','dispatching','running','waiting_interaction','needs_reconciliation')").fetchone()[0]
            writers = db.execute("SELECT count(*) FROM executions WHERE writer_revoked=0").fetchone()[0]
            assert active == writers == 0, (active, writers)
            for (body,) in db.execute("SELECT data FROM governance_records WHERE type='workflow_decision'"):
                decision=json.loads(body)
                if decision['operation'] in ('dispatch','accept','complete'):
                    actions.append({'operation':decision['operation'],'actor':decision['actor']['kind']})
    assert actions and all(item['actor']=='controller' for item in actions), 'Business advancement bypassed the actual manager'
    assert requests and requests.keys() == replies.keys(), "Missing actual protocol reply-write boundaries"
    assert all(item["status"] == "succeeded" for item in durable), "Native call failures are preserved"
    for sample in transport:
        matches = [item for item in durable if item['operation_id'] == sample['operation_id']
                   and item['request_key'] == sample['request_key']
                   and item['registered_at_ms'] >= sample['observed_at_ms']
                   and item['registered_at_ms'] <= sample['observed_at_ms'] + sample['roundtrip_ms'] + 1]
        if len(matches) == 1:
            item = matches[0]
            sample['observed_to_registered_ms'] = item['registered_at_ms'] - sample['observed_at_ms']
            sample['registered_to_result_record_ms'] = item['registered_to_result_ms']
            sample['result_record_to_written_ms'] = sample['roundtrip_ms'] - (item['result_record_at_ms'] - sample['observed_at_ms'])
    return {"transport": transport, "durable": durable, "business_actions":actions,
            "transport_summary": summarize([x["roundtrip_ms"] for x in transport]),
            "durable_summary": summarize([x["registered_to_reply_record_ms"] for x in durable]),
            "correlation_boundary": "registration/result timestamps are wall-clock fields before commit; transport total uses a monotonic clock; correlated segments have millisecond rounding and are not exact SQL commit spans",
            "scope": "actual single-workflow exploration, zero browsers plus one observational HTTP poller; not a 1000-sample SLA or 100-client acceptance"}


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--binary", type=Path, required=True)
    parser.add_argument("--codex-auth-source", type=Path, required=True)
    parser.add_argument("--output", type=Path, required=True, help="Must not exist")
    args = parser.parse_args()
    output = args.output.resolve()
    output.mkdir()
    root = Path(__file__).resolve().parents[2]
    probe = root / "tests/live/probe_configured_workflow.py"
    identity = {"binary_sha256": hashlib.sha256(args.binary.read_bytes()).hexdigest(),
                "probe_sha256": hashlib.sha256(probe.read_bytes()).hexdigest(),
                "runner_sha256": hashlib.sha256(Path(__file__).read_bytes()).hexdigest(),
                "source_commit": subprocess.check_output(["git", "rev-parse", "HEAD"], cwd=root, text=True).strip()}
    (output / "identity.json").write_text(json.dumps(identity, indent=2) + "\n")
    (output / "source.patch").write_bytes(subprocess.check_output(["git", "diff", "HEAD"], cwd=root))
    with (output / "probe.log").open("w") as log:
        result = subprocess.run([sys.executable, str(probe), "--binary", str(args.binary.resolve()),
                                 "--codex-auth-source", str(args.codex_auth_source.resolve()),
                                 "--report-directory", str(output / "run")],
                                env={**os.environ, "DEVDESK_PERF_LOG": "1"}, stdout=log, stderr=subprocess.STDOUT)
    if result.returncode:
        raise RuntimeError(f"Actual workflow failed; original evidence retained: {output / 'probe.log'}")
    timings = read_timings(output / "run")
    (output / "timings.json").write_text(json.dumps(timings, ensure_ascii=False, indent=2) + "\n")
    print(json.dumps({"transport": timings["transport_summary"], "durable": timings["durable_summary"], "scope": timings["scope"]}))


if __name__ == "__main__":
    main()

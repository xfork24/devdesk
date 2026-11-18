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
import shutil
import subprocess
import sys


def percentile(values, quantile):
    values = sorted(values)
    return values[max(0, math.ceil(len(values) * quantile) - 1)] if values else None


def summarize(values):
    return {"count": len(values), "p50_ms": percentile(values, .5),
            "p95_ms": percentile(values, .95), "p99_ms": percentile(values, .99),
            "max_ms": max(values) if values else None}


def read_timings(directory, allow_incomplete=False):
    events = []
    phases = []
    startups = []
    incomplete_log_tails = []

    def json_records(path, mixed=False):
        with path.open() as stream:
            for number, line in enumerate(stream, 1):
                if allow_incomplete and not line.endswith("\n"):
                    incomplete_log_tails.append({"file": str(path.relative_to(directory)), "line": number})
                    break
                if not mixed or line.startswith("{"):
                    yield json.loads(line)

    for item in json_records(directory / "host.log", mixed=True):
        if item.get("type") == "native_protocol":
            events.append(item)
        elif item.get("type") == "native_phase":
            phases.append(item)
        elif item.get("type") == "native_startup":
            startups.append(item)
    requests = {(e["operation_id"], e.get("sequence", e["request_key"])): e for e in events
                if e["phase"] == "request_observed"}
    replies = {(e["operation_id"], e.get("sequence", e["request_key"])): e for e in events
               if e["phase"] == "reply_written"}
    methods = {}
    protocol_results = {}
    for journal in (directory / "data").rglob("*.jsonl"):
        if journal.stem not in {key[0] for key in requests}:
            continue
        for entry in json_records(journal):
            frame = entry.get("frame", {})
            if entry.get("direction") == "out" and "id" in frame:
                key = hashlib.sha256(json.dumps(frame["id"], ensure_ascii=False, separators=(",", ":")).encode()).hexdigest()
                result = frame.get("result", {})
                success = result.get("success")
                if success is None and "isError" in result:
                    success = not result["isError"]
                if "error" in frame:
                    success = False
                if success is not None:
                    observation = {"protocol_success": success}
                    if not success:
                        observation["protocol_error_code"] = frame.get("error", {}).get("code")
                        for content in result.get("contentItems", result.get("content", [])):
                            try:
                                failure = json.loads(content.get("text", ""))
                            except (ValueError, TypeError):
                                continue
                            if isinstance(failure, dict) and "code" in failure:
                                observation["protocol_error_code"] = failure["code"]
                                break
                    protocol_results[(journal.stem, key)] = observation
            if entry.get("direction") != "in" or frame.get("method") not in ("item/tool/call", "devdesk/tool/call"):
                continue
            key = hashlib.sha256(json.dumps(frame["id"], ensure_ascii=False, separators=(",", ":")).encode()).hexdigest()
            params = frame.get("params", {})
            methods[(journal.stem, key)] = params.get("tool", params.get("name", "unknown"))
    transport = [{**reply, "method": methods.get((reply["operation_id"], reply["request_key"]), "unknown"),
                  **protocol_results.get((reply["operation_id"], reply["request_key"]), {})}
                 for key, reply in replies.items()]
    durable = []
    agent_durable = []
    actions = []
    open_execution_state = []
    for database in (directory / "data/projects").glob("*/engineering.sqlite3"):
        with sqlite3.connect(database.as_uri() + "?mode=ro", uri=True) as db:
            for call_id, body, created in db.execute("SELECT id,data,created_at FROM governance_records WHERE type='controller_tool_call'"):
                data = json.loads(body)
                row = db.execute("SELECT created_at FROM governance_records WHERE type='controller_tool_reply' AND definition_id=?", (call_id,)).fetchone()
                observation=db.execute("SELECT created_at FROM governance_records WHERE id=?",(data.get('observation_id',''),)).fetchone()
                finished=data.get('finished_at',observation[0] if observation else None)
                request_key = hashlib.sha256(json.dumps(data['request_id'], ensure_ascii=False, separators=(',', ':')).encode()).hexdigest()
                durable.append({"call_id": call_id, "method": data["method"], "status": data["status"],
                                "operation_id": data['worker_operation_id'], "request_key": request_key,
                                "registered_at_ms": created, "result_record_at_ms": finished,
                                "registered_to_result_ms": finished-created if finished is not None else None,
                                "registered_to_reply_record_ms": row[0] - created if row else None})
            sources={}
            for body, created in db.execute("SELECT data,created_at FROM governance_records WHERE type='agent_platform_request'"):
                request=json.loads(body)['request']
                sources[request['execution_id']]=created
            for call_id, body, created in db.execute("SELECT id,data,created_at FROM governance_records WHERE type='platform_call'"):
                data=json.loads(body)
                caller=data['request'].get('caller')
                if not caller:
                    continue
                row=db.execute("SELECT created_at FROM governance_records WHERE type='platform_tool_reply' AND definition_id=?",(call_id,)).fetchone()
                registered=sources[call_id]
                key=hashlib.sha256(json.dumps(caller['request_id'],ensure_ascii=False,separators=(',',':')).encode()).hexdigest()
                definition=data['request']['definition']
                if definition.get('$source')=='tool_definition':
                    source=db.execute('SELECT data FROM source_definitions WHERE reference=?',(definition['ref'],)).fetchone()
                    assert source, 'Missing exact native tool source: '+definition['ref']
                    definition=json.loads(source[0])
                finished=data.get('finished')
                agent_durable.append({'call_id':call_id,'method':definition['qualified_name'],'status':data['status'],
                    'operation_id':caller['operation_id'],'request_key':key,'registered_at_ms':registered,'result_record_at_ms':finished,
                    'registered_to_enqueue_ms':created-registered,'enqueue_to_result_record_ms':finished-created if finished is not None else None,
                    'registered_to_result_ms':finished-registered if finished is not None else None,
                    'registered_to_reply_record_ms':row[0]-registered if row else None})
            active = db.execute("SELECT count(*) FROM turns WHERE status IN ('queued','dispatching','running','waiting_interaction','needs_reconciliation')").fetchone()[0]
            writers = db.execute("SELECT count(*) FROM executions WHERE writer_revoked=0").fetchone()[0]
            open_execution_state.append({'project':database.parent.name,'active_or_unknown_turns':active,'unrevoked_writers':writers})
            if not allow_incomplete: assert active == writers == 0, (active, writers)
            for (body,) in db.execute("SELECT data FROM governance_records WHERE type='workflow_decision'"):
                decision=json.loads(body)
                if decision['operation'] in ('dispatch','accept','complete'):
                    actor=decision['actor']
                    if actor['kind']=='segment':
                        source=db.execute("SELECT type,data FROM governance_records WHERE id=?",(actor['authorization_id'],)).fetchone()
                        assert source and source[0]=='workflow_segment', 'Segment lacks its durable authorization'
                        segment=json.loads(source[1]);actor=segment['actor']
                        assert segment['plan_revision_id']==decision['plan_revision_id'], 'Segment changed its authorized plan'
                    assert actor['kind']=='controller', 'Business advancement bypassed the actual manager'
                    controller=db.execute("SELECT type,data FROM governance_records WHERE id=?",(actor['controller_id'],)).fetchone()
                    assert controller and controller[0]=='workflow_controller', 'Advancement lacks its actual manager'
                    owner=json.loads(controller[1])
                    assert owner['root_instance_id']==decision['root_instance_id'] and owner['epoch']==actor['epoch'], 'Foreign manager advancement'
                    actions.append({'operation':decision['operation'],'actor':decision['actor']['kind'],'authorized_actor':actor['kind']})
    assert actions, 'No actual manager business advancement'
    assert requests, "No actual native protocol requests"
    if not allow_incomplete:
        assert requests.keys() == replies.keys(), "Missing actual protocol reply-write boundaries"
        assert all(item["status"] in ("succeeded", "failed") and item["registered_to_reply_record_ms"] is not None for item in durable+agent_durable), "Native calls did not receive a durable terminal reply"
    for sample in transport:
        matches = [item for item in durable+agent_durable if item['operation_id'] == sample['operation_id']
                   and item['request_key'] == sample['request_key']
                   and item['registered_at_ms'] >= sample['observed_at_ms']
                   and item['registered_at_ms'] <= sample['observed_at_ms'] + sample['roundtrip_ms'] + 1]
        if len(matches) == 1:
            item = matches[0]
            sample['observed_to_registered_ms'] = item['registered_at_ms'] - sample['observed_at_ms']
            sample['registered_to_result_record_ms'] = item['registered_to_result_ms']
            sample['result_record_to_written_ms'] = sample['roundtrip_ms'] - (item['result_record_at_ms'] - sample['observed_at_ms']) if item['result_record_at_ms'] is not None else None
            sample['call_id']=item['call_id']
            sample['status']=item['status']
            sample['phases']=[phase for phase in phases if phase['call_id']==item['call_id']]
            if 'registered_to_enqueue_ms' in item:
                sample['registered_to_enqueue_ms']=item['registered_to_enqueue_ms']
                sample['enqueue_to_result_record_ms']=item['enqueue_to_result_record_ms']
    by_method={method:summarize([x['roundtrip_ms'] for x in transport if x['method']==method]) for method in sorted({x['method'] for x in transport})}
    return {"transport": transport, "durable": durable, "agent_durable":agent_durable, "business_actions":actions, "startups":startups,
            "partial_failed_scene":allow_incomplete,"open_execution_state":open_execution_state,
            "incomplete_log_tails":incomplete_log_tails,
            "observed_native_requests":len(requests),"actual_written_replies":len(replies),
            "requests_without_written_reply":[request for key,request in requests.items() if key not in replies],
            "protocol_failed_replies":[x for x in transport if x.get('protocol_success') is False],
            "protocol_replies_without_classification":[x for x in transport if 'protocol_success' not in x],
            "native_failed_calls":[x for x in durable+agent_durable if x['status']=='failed'],"by_method":by_method,
            "transport_summary": summarize([x["roundtrip_ms"] for x in transport]),
            "durable_summary": summarize([x["registered_to_reply_record_ms"] for x in durable if x['registered_to_reply_record_ms'] is not None]),
            "correlation_boundary": "registration/result timestamps are wall-clock fields before commit; transport total uses a monotonic clock; correlated segments have millisecond rounding and are not exact SQL commit spans",
            "scope": "actual single-workflow exploration, zero browsers plus one observational HTTP poller; not a 1000-sample SLA or 100-client acceptance"}


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--binary", type=Path, required=True)
    parser.add_argument("--codex-auth-source", type=Path, required=True)
    parser.add_argument("--frontends", type=int, choices=[0,100], default=0)
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
    helpers=[Path(__file__).resolve(),probe,root / "tests/performance/native_frontend_load.py",root / "tests/live/workflow_observer.py"]
    identity["tooling_sha256"]={str(path.relative_to(root)):hashlib.sha256(path.read_bytes()).hexdigest() for path in helpers}
    for path in helpers:
        destination=output / "tooling" / path.relative_to(root)
        destination.parent.mkdir(parents=True,exist_ok=True)
        shutil.copy2(path,destination)
    (output / "identity.json").write_text(json.dumps(identity, indent=2) + "\n")
    (output / "source.patch").write_bytes(subprocess.check_output(["git", "diff", "HEAD"], cwd=root))
    with (output / "probe.log").open("w") as log:
        result = subprocess.run([sys.executable, str(probe), "--binary", str(args.binary.resolve()),
                                 "--codex-auth-source", str(args.codex_auth_source.resolve()),
                                 "--report-directory", str(output / "run"), "--frontends", str(args.frontends), "--prepare-frontend-fixture"],
                                env={**os.environ, "DEVDESK_PERF_LOG": "1"}, stdout=log, stderr=subprocess.STDOUT)
    if result.returncode:
        raise RuntimeError(f"Actual workflow failed; original evidence retained: {output / 'probe.log'}")
    timings = read_timings(output / "run")
    timings["frontend_clients"] = args.frontends
    timings["prepared_independent_frontend_targets"] = 100
    if args.frontends:
        timings["frontend_load"] = json.loads((output / "run/frontend-load/summary.json").read_text())
        timings["scope"] = "Actual native two-step workflow under 100 HTTP/SSE clients, open 100 reads/s and 10 independent durable writes/s, same local Host; small history, no browser or 1000-sample SLA claim"
    (output / "timings.json").write_text(json.dumps(timings, ensure_ascii=False, indent=2) + "\n")
    print(json.dumps({"transport": timings["transport_summary"], "durable": timings["durable_summary"], "scope": timings["scope"]}))


if __name__ == "__main__":
    main()

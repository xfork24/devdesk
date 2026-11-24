#!/usr/bin/env python3
"""Local open-arrival HTTP/SSE load with durable writes and ongoing workflow work.

All scheduled requests, generator queuing, failures and raw replies are counted.
The opt-in fixture owns a fresh dataset. This is local W1 evidence, not formal P6.
"""
import argparse
from collections import defaultdict
from concurrent.futures import ThreadPoolExecutor, wait
import hashlib
import gzip
import json
import math
import os
from pathlib import Path
import select
import shutil
import signal
import sqlite3
import subprocess
import sys
import threading
import time
import urllib.error
import urllib.parse
import urllib.request
import uuid


def stats(values):
    values = sorted(values)
    def p(f):
        return values[max(0, math.ceil(len(values) * f) - 1)] if values else None
    return {"count": len(values), "p50_ms": p(.5), "p95_ms": p(.95), "p99_ms": p(.99), "max_ms": values[-1] if values else None}


def dispatch_latency(dataset, start_ms, end_ms):
    rows=[]
    for database in dataset.glob('projects/*/engineering.sqlite3'):
        with sqlite3.connect(database.as_uri()+'?mode=ro',uri=True) as connection:
            connection.execute('BEGIN')
            accepts={}
            for body,created in connection.execute("SELECT data,created_at FROM governance_records WHERE type='workflow_decision'"):
                decision=json.loads(body)
                if decision.get('operation')=='accept':accepts[decision.get('execution_id')]=created
            for identity,body,created in connection.execute("SELECT id,data,created_at FROM governance_records WHERE type='workflow_instance'"):
                instance=json.loads(body)
                source=connection.execute("SELECT data FROM source_definitions WHERE type='workflow' AND reference=?",(instance['workflow_ref'],)).fetchone()
                if source is None:continue
                definitions={s['key']:s for s in json.loads(source[0])['steps']}
                steps={s['step']:s for s in instance['steps']}
                for key,step in steps.items():
                    started=step.get('started')
                    if started is None or not start_ms<=started<=end_ms:continue
                    dependencies=definitions[key].get('dependencies',[])
                    # These benchmark graphs are fixed serial control/Docker
                    # chains with no Human/conditional/business-input gates.
                    if not dependencies or not all(steps[d]['id'] in accepts for d in dependencies):continue
                    ready=max(accepts[steps[d]['id']] for d in dependencies)
                    rows.append({'project':database.parent.name,'instance':identity,'execution':step['id'],'dependency_accept_field_ms':ready,'started_field_ms':started,'ms':started-ready})
    return rows


def validate_stopped_fixture(dataset, fixture, real_runtime):
    result={'databases':[],'errors':[],'running_containers':[],'boundary':'After private Host shutdown; read-only integrity, registered Artifact hashes and writer/worker checks. No state repair or checkpoint command.'}
    for database in sorted(dataset.rglob('*.sqlite3')):
        item={'database':str(database.relative_to(dataset)),'wal_bytes_after_shutdown':Path(str(database)+'-wal').stat().st_size if Path(str(database)+'-wal').exists() else 0}
        try:
            with sqlite3.connect(database.as_uri()+'?mode=ro',uri=True,timeout=5) as connection:
                item['quick_check']=[row[0] for row in connection.execute('PRAGMA quick_check')]
                item['foreign_key_errors']=connection.execute('PRAGMA foreign_key_check').fetchall()
                tables={row[0] for row in connection.execute("SELECT name FROM sqlite_master WHERE type='table'")}
                if 'executions' in tables:item['unrevoked_writers']=connection.execute('SELECT count(*) FROM executions WHERE writer_revoked=0').fetchone()[0]
                if 'orchestration_leases' in tables:item['retained_leases']=connection.execute('SELECT id,epoch,expires_at FROM orchestration_leases').fetchall()
                checked=0
                if 'artifacts' in tables:
                    for identity,relative,digest in connection.execute('SELECT id,relative_path,sha256 FROM artifacts'):
                        path=database.parent/relative
                        if not path.is_file() or not path.resolve().is_relative_to(database.parent.resolve()) or hashlib.sha256(path.read_bytes()).hexdigest()!=digest:
                            result['errors'].append({'artifact_integrity':identity,'database':item['database']})
                        checked+=1
                item['registered_artifacts_checked']=checked
                if item['quick_check']!=['ok'] or item['foreign_key_errors'] or item.get('unrevoked_writers',0):result['errors'].append({'database_integrity_or_writer':item})
        except Exception as error:
            item['error']=repr(error);result['errors'].append({'database_observation':item})
        result['databases'].append(item)
    if real_runtime and fixture:
        for project in sorted({root['project_id'] for root in fixture['roots']}):
            try:
                process=subprocess.run(['docker','ps','-q','--filter','label=devdesk.project='+project],capture_output=True,text=True,timeout=10,check=True)
                result['running_containers'].extend(process.stdout.split())
            except Exception as error:result['errors'].append({'docker_cleanup_observation':repr(error),'project':project})
        if result['running_containers']:result['errors'].append({'running_containers_after_shutdown':result['running_containers']})
    return result


def storage_trace(output, rpc_rows, browser):
    if browser:
        samples = output / "browser-rpc.jsonl"
        if not samples.is_file():
            return {}
        with samples.open() as stream:
            rpc_rows = [row for line in stream if (row := json.loads(line))["phase"] == "load"]
    else:
        rpc_rows = [row for row in rpc_rows if row["label"] in {"read", "write"}]
    requests = {row.get("host_request_id"): row for row in rpc_rows
                if row.get("host_request_id") is not None}
    inventory = Path(__file__).resolve().parents[2] / "crates/core/src/rpc_contracts.json"
    read_methods = {entry["method"] for entry in json.loads(inventory.read_text())["entries"]
                    if entry["access"] == "read"}
    spans = ["sqlite.pool_wait", "sqlite.read_pool_wait", "sqlite.busy_wait",
             "resource.sql_transaction", "sqlite.table_read_decode"]
    times = defaultdict(list)
    busy = 0
    matched = 0
    checkout_calls = 0
    checkout_bounds = None
    checkout_buckets = []
    with (output / "server.log").open() as stream:
        for line in stream:
            if not line.startswith("{"):
                continue
            try:
                trace = json.loads(line)
            except json.JSONDecodeError:
                continue
            if str(trace.get("request_id")) not in requests:
                continue
            matched += 1
            busy += trace.get("error_code") == "storage_busy"
            kind = "read" if trace["method"] in read_methods else "write"
            observed = trace.get("measurement", {})
            histogram = observed.get("read_pool_checkout_histogram")
            if histogram:
                if checkout_bounds is None:
                    checkout_bounds = histogram["bounds_ms"]
                    checkout_buckets = [0] * len(histogram["counts"])
                if checkout_bounds != histogram["bounds_ms"]:
                    raise ValueError("Read pool histogram changed within one build")
                checkout_buckets = [a + b for a, b in zip(checkout_buckets, histogram["counts"])]
            measurement = observed.get("spans", {})
            checkout_calls += measurement.get("sqlite.pool_wait", {}).get("calls", 0)
            for stage in spans:
                if stage in measurement:
                    times[(kind, stage)].append(measurement[stage]["total_ms"])
    checkout_count = sum(checkout_buckets)
    cumulative = 0
    upper_p95 = None
    if checkout_count and checkout_bounds:
        for bound, count in zip(checkout_bounds, checkout_buckets):
            cumulative += count
            if cumulative >= math.ceil(checkout_count * 0.95):
                upper_p95 = bound
                break
    return {
        "matched_offered_requests": matched, "offered_request_ids_with_responses": len(requests),
        "read_pool_checkouts": {
            "count": checkout_count, "p95_upper_bound_ms": upper_p95,
            "bounds_ms": checkout_bounds, "counts": checkout_buckets,
        },
        "observed_checkouts": checkout_calls, "storage_busy": busy,
        "per_request_stage_totals": {
            kind: {stage: stats(times[kind, stage]) for stage in spans}
            for kind in ["read", "write"]
        },
    }


def checkpoint_after_load(dataset):
    """Measure one passive checkpoint after the workload window on its private databases."""
    observations = []
    for database in sorted(dataset.rglob("*.sqlite3")):
        started = time.monotonic()
        wal = database.with_name(database.name + "-wal")
        bytes_before = wal.stat().st_size if wal.exists() else 0
        with sqlite3.connect(database.as_uri() + "?mode=rw", uri=True, timeout=0.1) as connection:
            busy, log_frames, checkpointed = connection.execute(
                "PRAGMA wal_checkpoint(PASSIVE)"
            ).fetchone()
        observations.append({
            "database": str(database.relative_to(dataset)),
            "busy": busy,
            "log_frames": log_frames,
            "checkpointed_frames": checkpointed,
            "wal_bytes_before": bytes_before,
            "ms": (time.monotonic() - started) * 1000,
        })
    return observations


def pending_root_supply(dataset, slot_roots, start_ms, end_ms):
    intervals=[];missing=[]
    projects={root['project_id']for roots in slot_roots for root in roots}
    for project in projects:
        roots=[root for group in slot_roots for root in group if root['project_id']==project]
        with sqlite3.connect((dataset/'projects'/project/'engineering.sqlite3').as_uri()+'?mode=ro',uri=True) as connection:
            connection.execute('BEGIN')
            for root in roots:
                selected=connection.execute("SELECT time FROM events WHERE kind='workflow.driver.select' AND target=? ORDER BY id LIMIT 1",(root['id'],)).fetchone()
                saved=connection.execute("SELECT data FROM governance_records WHERE type='workflow_instance' AND id=?",(root['id'],)).fetchone()
                if not selected or not saved:
                    missing.append(root);continue
                steps=json.loads(saved[0])['steps']
                last=connection.execute("SELECT created_at FROM governance_records WHERE type='workflow_decision' AND json_extract(data,'$.operation')='accept' AND json_extract(data,'$.execution_id')=? ORDER BY rowid LIMIT 1",(steps[-1]['id'],)).fetchone()
                began=max(start_ms,selected[0]);ended=min(end_ms,last[0] if last else end_ms)
                if began<ended:intervals.append({'project_id':project,'root_id':root['id'],'selected_field_ms':selected[0],'last_step_accept_field_ms':last[0]if last else None,'start_ms':began,'end_ms':ended})
    points=defaultdict(int);points[start_ms]=0;points[end_ms]=0
    for row in intervals:points[row['start_ms']]+=1;points[row['end_ms']]-=1
    timeline=sorted(points);count=0;minimum=None;gaps=[]
    for at,following in zip(timeline,timeline[1:]):
        count+=points[at]
        if following<=at:continue
        minimum=count if minimum is None else min(minimum,count)
        if count<4:gaps.append({'start_ms':at,'end_ms':following,'pending_roots':count})
    return {'minimum_pending_roots':minimum,'intervals':intervals,'below_four_intervals':gaps,'unobserved_roots':missing,'scope':'Fixed serial benchmark graphs only: driver-select Event.time through last-step accept.created_at, clipped to the offered window. Both are pre-COMMIT fields, not exact readiness timestamps. A pending root alone does not prove execution; completion counters and actual Docker receipts are checked separately.'}


def run(args):
    args.output.mkdir(parents=True, exist_ok=False)
    output = args.output.resolve()
    tooling = {}
    for name in ['joint_frontends.py','joint_browser.mjs']:
        source=Path(__file__).with_name(name)
        tooling[name]=hashlib.sha256(source.read_bytes()).hexdigest()
        shutil.copy2(source,output/name)
    (output/'invocation.json').write_text(json.dumps({'arguments':{key:str(value) if isinstance(value,Path) else value for key,value in vars(args).items()},'tooling_sha256':tooling},indent=2))
    dataset = output / 'dataset'
    dataset.mkdir()
    (output / '.active-frontends-fixture').touch()
    binary = output / 'host-tests'
    shutil.copy2(args.test_binary, binary)
    binary.chmod(0o700)
    environment = {**os.environ, 'DEVDESK_ACTIVE_FRONTEND_DATASET': str(dataset),
                   'DEVDESK_PERFORMANCE_HISTORY_SCALE': str(args.scale), 'DEVDESK_PERFORMANCE_PROJECTS': str(args.projects)}
    if args.real_runtime:
        if not args.host_binary or not args.test_image: raise ValueError('Actual runtime needs a production Host and pinned test image')
        environment.update(DEVDESK_PERFORMANCE_REAL_RUNTIME='1',DEVDESK_PERFORMANCE_TEST_IMAGE=args.test_image)
    if args.hot_project: environment['DEVDESK_PERFORMANCE_HOT_PROJECT']='1'
    environment.pop('DEVDESK_ACCESS_TOKEN', None)
    if args.trace: environment['DEVDESK_PERF_LOG']='1'
    rows, samples, failures, streams, reconnects, sse_threads = [], [], [], [], [], []
    rpc_log_lock=threading.Lock()
    def record_rpc(row):
        encoded=json.dumps(row)+'\n'
        with rpc_log_lock:
            rows.append(row)
            with (output/'rpc.jsonl').open('a') as stream:stream.write(encoded)
    event_lock=threading.Lock()
    event_sink=gzip.open(output/'events.jsonl.gz','wt',compresslevel=1)
    event_counts={'all':0,'timed':0,'max_ms':None,'negative_clock_samples':0,'overflow_samples':0}
    event_hist={}
    def record_event(event):
        encoded=json.dumps(event)+'\n'
        with event_lock:
            if stop.is_set():return
            event_sink.write(encoded)
            event_counts['all']+=1
            if 'commit_field_to_receive_ms' in event:
                delay=event['commit_field_to_receive_ms']
                event_counts['timed']+=1
                event_counts['max_ms']=max(event_counts['max_ms'] or delay,delay)
                if delay<0:event_counts['negative_clock_samples']+=1
                if delay>60000:event_counts['overflow_samples']+=1
                bucket=max(-1,min(60000,math.ceil(delay)))
                event_hist[bucket]=event_hist.get(bucket,0)+1
    def event_stats():
        with event_lock: counts=event_counts.copy();hist=sorted(event_hist.items())
        def percentile(q):
            target=math.ceil(counts['timed']*q);seen=0
            for bucket,count in hist:
                seen+=count
                if seen>=target:return bucket
            return None
        return {**counts,'p50_ms':percentile(.5),'p95_ms':percentile(.95),'p99_ms':percentile(.99),'histogram_resolution_ms':1,'histogram_overflow_ms':60000,'raw':'events.jsonl.gz'}
    stop = threading.Event()
    renew_stop = threading.Event()
    log = (output / 'server.log').open('w')
    if args.scale or args.real_runtime:
        with (output / 'seed.log').open('w') as seed_log:
            seed_result = subprocess.run([str(binary), 'active_frontend_performance::active_frontend_server', '--exact', '--ignored', '--nocapture'], env={**environment, 'DEVDESK_PERFORMANCE_PREPARE_ONLY':'1'}, stdout=seed_log, stderr=seed_log, timeout=900)
        if seed_result.returncode: raise RuntimeError('Dataset factory failed; retained seed.log')
        environment['DEVDESK_PERFORMANCE_OPEN_SEED']='1'
    command=[str(binary), 'active_frontend_performance::active_frontend_server', '--exact', '--ignored', '--nocapture']
    if args.real_runtime:
        production=output/'devdesk-host';shutil.copy2(args.host_binary,production);command=[str(production),'--data',str(dataset),'--port','0']
    server = subprocess.Popen(command, env=environment, stdout=subprocess.PIPE, stderr=log)
    try:
        pending, fixture = '', None
        deadline = time.monotonic() + 600
        while time.monotonic() < deadline and server.poll() is None:
            if not select.select([server.stdout], [], [], .1)[0]:
                continue
            chunk = os.read(server.stdout.fileno(), 65536).decode()
            log.write(chunk); log.flush(); pending += chunk
            while '\n' in pending:
                line, pending = pending.split('\n', 1)
                if line.startswith('ACTIVE_FRONTEND_FIXTURE='):
                    fixture = json.loads(line.split('=', 1)[1]); break
                if args.real_runtime and line.startswith('http://'):
                    fixture=json.loads((output/'performance-seed.json').read_text());fixture['origin']=line.strip();break
            if fixture:
                break
        if not fixture:
            raise RuntimeError('Fixture did not initialize; retained server.log')
        (output / 'fixture.json').write_text(json.dumps(fixture, indent=2))
        origin = fixture['origin']
        with urllib.request.urlopen(origin, timeout=30) as response:
            cookie = response.headers['Set-Cookie'].split(';', 1)[0]
        def rpc(method, params, label='observe', client=-1, scheduled=None, expected_errors=()):
            params={key:value for key,value in params.items() if key!='write_owner_id'}
            if method in ['query.workflow_progress','query.workflow_instance','query.workflow_management_summary','query.workflow_driver']:
                params.pop('feature_id',None)
            sent = time.monotonic()
            scheduled = sent if scheduled is None else scheduled
            row = {'method':method, 'label':label, 'client':client, 'project_id':params.get('project_id'), 'scheduled':scheduled, 'sent':sent, 'generator_queue_ms':(sent-scheduled)*1000}
            try:
                request = urllib.request.Request(origin+'/rpc', json.dumps({'jsonrpc':'2.0','id':str(uuid.uuid4()),'method':method,'params':params}).encode(), {'Cookie':cookie,'Content-Type':'application/json'})
                with urllib.request.urlopen(request, timeout=30) as response:
                    body = response.read()
                    row['host_request_id'] = response.headers.get('x-devdesk-request-id')
                    value = json.loads(body)
                row.update(bytes=len(body), error=value.get('error'))
                row['expected_error'] = bool(row['error'] and row['error'].get('data',{}).get('code') in expected_errors)
                if row['error'] and not row['expected_error']:
                    failures.append(row)
                return value.get('result')
            except Exception as error:
                row['exception'] = repr(error); failures.append(row)
                return None
            finally:
                row['done']=time.monotonic(); row['ms']=(row['done']-sent)*1000; row['scheduled_ms']=(row['done']-scheduled)*1000
                record_rpc(row)
        if len(fixture['projects'])>2:
            # D2 has ten historical projects, while only two need to run the
            # four continuing work slots. Frontends must nevertheless read and
            # write all ten projects. Extra Drafts are created through ordinary
            # commands before sampling; no workflow or model is started.
            setup_started=time.monotonic()
            frontend_roots=[]
            for project in fixture['projects']:
                active=next((root for root in fixture['roots'] if root['project_id']==project),None)
                if active:
                    frontend_roots.append(active.copy())
                else:
                    database=dataset/'projects'/project/'engineering.sqlite3'
                    with sqlite3.connect(database.as_uri()+'?mode=ro',uri=True) as connection:
                        row=connection.execute("SELECT id,json_extract(data,'$.owner_id') FROM governance_records WHERE type='workflow_instance' AND json_extract(data,'$.parent_execution_id') IS NULL ORDER BY rowid DESC LIMIT 1").fetchone()
                    if row is None:raise RuntimeError('D2 project lacks a real frozen workflow to observe')
                    frontend_roots.append({'project_id':project,'id':row[0],'feature_id':row[1]})
            independent=[context for batch in zip(*fixture.get('d2_contexts', []), strict=True) for context in batch]
            prepared_targets=bool(independent)
            for n in range(0 if prepared_targets else 100):
                project=fixture['projects'][n%len(fixture['projects'])]
                feature=rpc('feature.create',{'project_id':project,'command_id':str(uuid.uuid4()),'payload':{'project_id':project,'title':f'D2 independent frontend {n}','initial_requirement':'Performance context only; no workflow or model may start','start_workflow':False}},'setup',n)
                if not feature:raise RuntimeError('D2 independent frontend Feature creation failed')
                detail=rpc('query.workspace',{'project_id':project,'feature_id':feature['id']},'setup',n)
                if not detail:raise RuntimeError('D2 frontend context query failed')
                context=next(row for row in detail['detail']['contexts'] if row['owner_id']==feature['id'])
                independent.append({'project_id':project,'owner_id':feature['id'],'id':context['id'],'revision':context['revision']})
            fixture['original_contexts']=fixture['contexts']
            fixture['contexts']=independent
            fixture['frontend_roots']=frontend_roots
            (output/'fixture.json').write_text(json.dumps(fixture,indent=2))
            (output/'frontend-scope-setup.json').write_text(json.dumps({'seconds':time.monotonic()-setup_started,'write_targets_prepared_offline':prepared_targets,'write_targets_by_project':{project:sum(c['project_id']==project for c in independent)for project in fixture['projects']},'read_targets':frontend_roots,'scope':'Prepared editable contexts before the offered window. Four continuing roots remain on two projects; dormant frozen workflows in other projects are observed without fabricating execution.'},indent=2))
        completed_by_root = {}
        slot_roots = [[root.copy()] for root in fixture['roots']]
        retired = set()
        progress_lock=threading.Lock()
        def record_progress(root,value):
            with progress_lock, (output/'workflow-progress.jsonl').open('a') as stream:
                stream.write(json.dumps({'time':time.monotonic(),'project_id':root['project_id'],'id':root['id'],'status':value['record']['data']['status'],'counts':value['counts']})+'\n')
        def progress():
            values = []
            for roots in slot_roots:
                last = None
                for root in roots.copy():
                    if root['id'] in retired: continue
                    value = rpc('query.workflow_progress', root)
                    if value is None: raise RuntimeError('Workflow progress unavailable')
                    record_progress(root,value)
                    completed_by_root[root['id']] = value['counts']['completed']
                    if value['record']['data']['status'] == 'completed': retired.add(root['id'])
                    last = value
                if last is None: raise RuntimeError('No active logical workflow slot')
                last['counts']['completed'] = sum(completed_by_root.get(root['id'],0) for root in roots)
                values.append(last)
            return values
        def renew():
            # Only the deterministic Rust-driver fixture is replenished. This
            # never controls a native PM or writes fabricated terminal facts.
            while not renew_stop.wait(2):
                try:
                    for slot, root in enumerate(fixture['roots'].copy()):
                        value=rpc('query.workflow_progress',root,'renew_observe')
                        if value is None: raise RuntimeError('Renewal observation failed')
                        record_progress(root,value)
                        completed_by_root[root['id']]=value['counts']['completed']
                        if value['counts']['total']-value['counts']['completed']>8: continue
                        project=root['project_id']
                        def command(method,target=None,revision=None,payload=None):
                            result=rpc(method,{'project_id':project,'command_id':str(uuid.uuid4()),'target':target,'expected_revision':revision,'payload':payload or {}},'renew')
                            if result is None: raise RuntimeError('Fixture renewal failed: '+method)
                            return result
                        feature=command('feature.create',payload={'project_id':project,'title':'Continuous deterministic load fixture','initial_requirement':'Control-only performance fixture; no native PM','start_workflow':False})
                        detail=rpc('query.workspace',{'project_id':project,'feature_id':feature['id']},'renew_observe')
                        run=command('workflow.start',feature['id'],detail['detail']['feature']['revision'])
                        header=rpc('query.workflow_management_summary',{'project_id':project,'id':run['id']},'renew_observe')['record']
                        command('workflow.driver.select',run['id'],header['revision'],{'mode':'rust','expected_driver_epoch':0,'plan_revision_id':header['data']['active_plan_revision_id'],'control_version':header['data']['control_version']})
                        next_root={'project_id':project,'id':run['id'],'feature_id':feature['id'],'write_owner_id':root['write_owner_id']}
                        fixture['roots'][slot]=next_root
                        for index,target in enumerate(fixture.get('frontend_roots',[])):
                            if target['id']==root['id']:fixture['frontend_roots'][index]=next_root.copy()
                        slot_roots[slot].append(next_root)
                        temporary=output/'fixture-next.json';temporary.write_text(json.dumps(fixture));temporary.replace(output/'fixture.json')
                        with (output/'renewals.jsonl').open('a') as stream:stream.write(json.dumps({'time':time.monotonic(),'slot':slot,'old':root,'new':next_root})+'\n')
                except Exception as error:
                    failures.append({'renewal_error':repr(error)});return
        reconnect_clients = (args.reconnecting_clients or args.clients) if args.sse_reconnect else 0

        def sse(client):
            project = fixture["contexts"][client]["project_id"]
            cursor = None
            cycles = 0
            try:
                while not stop.is_set():
                    headers = {"Cookie": cookie}
                    if cursor:
                        headers["Last-Event-ID"] = cursor
                    began = time.monotonic()
                    url = origin + "/events?" + urllib.parse.urlencode({
                        "project_id": project, "tail": "true",
                        "configuration": str(args.sse_configuration).lower(),
                    })
                    request = urllib.request.Request(url, headers=headers)
                    reconnect_next = False
                    with urllib.request.urlopen(request, timeout=15) as response:
                        streams.append(response)
                        reconnect = {
                            "client": client, "cycle": cycles, "resume_cursor": cursor,
                            "connected_ms": (time.monotonic() - began) * 1000,
                        } if cycles else None
                        pending_id = None
                        until = time.monotonic() + 10
                        while not stop.is_set():
                            line = response.readline().decode()
                            if not line:
                                break
                            if line.startswith("id:"):
                                pending_id = line[3:].strip()
                                if not pending_id.startswith(project + ":"):
                                    failures.append({"client": client, "foreign_event": pending_id})
                            elif line.startswith("data:"):
                                value = json.loads(line[5:].strip())
                                event = {"client": client, "time": time.monotonic(),
                                         "id": pending_id, "kind": value.get("kind")}
                                if isinstance(value.get("time"), (int, float)):
                                    event["commit_field_to_receive_ms"] = time.time() * 1000 - value["time"]
                                record_event(event)
                                if reconnect is not None:
                                    reconnect["first_event_ms"] = (time.monotonic() - began) * 1000
                                    roots = fixture.get("frontend_roots", fixture["roots"])
                                    root = next(target for target in roots if target["project_id"] == project)
                                    result = rpc("query.workflow_progress", root, "reconnect_verify", client)
                                    reconnect["state_read_ms"] = (time.monotonic() - began) * 1000
                                    if not result or result["record"]["id"] != root["id"]:
                                        failures.append({"client": client, "reconnect_state_unavailable": cycles})
                                    reconnects.append(reconnect)
                                    reconnect = None
                            elif not line.strip():
                                if pending_id:
                                    cursor = pending_id
                                pending_id = None
                                if client < reconnect_clients and cycles < 10 and time.monotonic() >= until:
                                    cycles += 1
                                    reconnect_next = True
                                    break
                    if not reconnect_next:
                        if not stop.is_set():
                            failures.append({"client": client, "unexpected_stream_end": cycles})
                        return
            except Exception as error:
                if not stop.is_set():
                    failures.append({"client": client, "sse_error": repr(error)})

        def committed_work():
            totals={'workflow_step_updates':0,'workflow_state_commits':0,'completed_docker_tests':0,'workflow_events':0}
            for database in dataset.glob('projects/*/engineering.sqlite3'):
                with sqlite3.connect(database.as_uri()+'?mode=ro',uri=True,timeout=1) as connection:
                    totals['workflow_step_updates']+=connection.execute("SELECT coalesce(sum(revision-1),0) FROM resource_versions WHERE kind='workflow_step'").fetchone()[0]
                    totals['workflow_events']+=connection.execute("SELECT count(*) FROM events WHERE kind='workflow.runtime'").fetchone()[0]
                    # WorkflowCommit emits exactly one method event in the
                    # same transaction. No-op observations return beforehand.
                    totals['workflow_state_commits']+=connection.execute("SELECT count(*) FROM events WHERE kind IN ('workflow.step.dispatch','workflow.step.accept','runtime.workflow.observe','workflow.complete')").fetchone()[0]
                    if args.real_runtime:
                        totals['completed_docker_tests']+=connection.execute("SELECT count(*) FROM governance_records c JOIN source_definitions s ON s.reference=json_extract(c.data,'$.request.definition.ref') WHERE c.type='platform_call' AND json_extract(c.data,'$.status')='succeeded' AND json_extract(s.data,'$.qualified_name')='tests.run'").fetchone()[0]
            return {'time':time.monotonic(),**totals}
        def monitor():
            while not stop.wait(1):
                process=subprocess.run(['ps','-p',str(server.pid),'-o','%cpu=,rss=,time='],capture_output=True,text=True)
                fields=process.stdout.split()
                generator=subprocess.run(['ps','-p',str(os.getpid()),'-o','%cpu=,rss=,time='],capture_output=True,text=True).stdout.split()
                wal=[f.stat().st_size for f in dataset.rglob('*-wal') if f.exists()]
                sample={'time':time.monotonic(),'host_ps':fields,'generator_ps':generator,'max_wal_bytes':max(wal,default=0),'total_wal_bytes':sum(wal)}
                if len(samples)%5==0:
                    try: sample['committed_work']=committed_work()
                    except Exception as error: sample['work_observation_error']=repr(error)
                samples.append(sample)
                with (output/'resources.jsonl').open('a') as stream:stream.write(json.dumps(sample)+'\n')
        threading.Thread(target=monitor,daemon=True).start()
        def monitor_workers():
            # Read only this fixture's project-labelled containers. Docker VM
            # and generator resources are not attributed to the Host process.
            projects=sorted({root['project_id'] for root in fixture['roots']})
            while not stop.wait(5):
                observed={'time':time.monotonic(),'containers':[]}
                try:
                    ids=[]
                    for project in projects:
                        result=subprocess.run(['docker','ps','-q','--filter','label=devdesk.project='+project],capture_output=True,text=True,timeout=5,check=True)
                        ids.extend(result.stdout.split())
                    if ids:
                        result=subprocess.run(['docker','stats','--no-stream','--format','{{json .}}',*ids],capture_output=True,text=True,timeout=8,check=True)
                        observed['containers']=[json.loads(line) for line in result.stdout.splitlines() if line]
                except Exception as error:
                    observed['observation_error']=repr(error)
                with (output/'worker-resources.jsonl').open('a') as stream: stream.write(json.dumps(observed)+'\n')
        if args.real_runtime: threading.Thread(target=monitor_workers,daemon=True).start()
        if not args.browser:
            for client in range(args.clients):
                thread=threading.Thread(target=sse,args=(client,),daemon=True);sse_threads.append(thread);thread.start()
            deadline=time.monotonic()+30
            while len(streams)<args.clients and time.monotonic()<deadline: time.sleep(.05)
            if len(streams)!=args.clients: raise RuntimeError('SSE subscriptions incomplete')
        before=progress()
        work_before=committed_work()
        (output/'progress-before.json').write_text(json.dumps(before))
        renewal_thread=threading.Thread(target=renew,daemon=True) if args.renew else None
        if renewal_thread: renewal_thread.start()
        start=time.monotonic()
        start_wall_ms=time.time()*1000
        scheduled_count=0
        browser_result=None
        burst_results=[]
        work_offer_end=None
        if args.browser:
            browser_result=subprocess.run(['node',str(Path(__file__).with_name('joint_browser.mjs')),str(output/'fixture.json'),str(output),str(args.clients),str(args.seconds)],capture_output=True,text=True)
            (output/'browser-process.log').write_text(browser_result.stdout+'\n'+browser_result.stderr)
            if browser_result.returncode: failures.append({'browser_exit':browser_result.returncode})
        elif args.burst_cycles:
            versions=[context.get('revision',1) for context in fixture['contexts']]
            with ThreadPoolExecutor(max_workers=128) as pool:
                def group(label, requests, expected_errors=()):
                    nonlocal scheduled_count
                    barrier=threading.Barrier(len(requests)+1)
                    scheduled=[None]
                    def issue(client,method,params):
                        barrier.wait(timeout=30)
                        return rpc(method,params,label,client,scheduled[0],expected_errors)
                    futures=[pool.submit(issue,n,*request) for n,request in enumerate(requests)]
                    scheduled_count+=len(requests)
                    scheduled[0]=time.monotonic()
                    barrier.wait(timeout=30)
                    results=[future.result() for future in futures]
                    elapsed=(time.monotonic()-scheduled[0])*1000
                    return results,elapsed
                def write_request(n,revision,command_id=None):
                    context=fixture['contexts'][n]
                    return 'context.update',{'project_id':context['project_id'],'command_id':command_id or str(uuid.uuid4()),'target':context['id'],'expected_revision':revision,'payload':{'content':f'Burst cycle {cycle}, client {n}'}}
                for cycle in range(args.burst_cycles):
                    requests=[('query.workflow_progress',fixture['roots'][n%len(fixture['roots'])]) for n in range(args.clients)]
                    reads,read_ms=group(f'burst-read-{cycle}',requests)
                    writes,write_ms=group(f'burst-write-{cycle}',[write_request(n,versions[n]) for n in range(args.clients)])
                    for n,result in enumerate(writes):
                        if result:versions[n]+=1
                    # One original resource version, distinct commands: exactly
                    # one succeeds. Replay the winning command byte for byte.
                    competing=[write_request(0,versions[0]) for _ in range(args.clients)]
                    outcomes,compete_ms=group(f'burst-compete-{cycle}',competing,('revision_conflict',))
                    winners=[n for n,result in enumerate(outcomes) if result]
                    replay_ok=False
                    if len(winners)==1:
                        winner=winners[0];versions[0]+=1
                        replay=rpc(*competing[winner],f'burst-replay-{cycle}')
                        replay_ok=replay==outcomes[winner]
                    result={'cycle':cycle,'clients':args.clients,'reads_completed':sum(r is not None for r in reads),'writes_completed':sum(r is not None for r in writes),'compete_winners':len(winners),'replay_identical':replay_ok,'read_group_ms':read_ms,'write_group_ms':write_ms,'compete_group_ms':compete_ms}
                    burst_results.append(result)
                    if result['reads_completed']!=args.clients or result['writes_completed']!=args.clients or len(winners)!=1 or not replay_ok:failures.append({'burst_correctness':result})
                    # Read all independent revisions after each round: an HTTP
                    # success or replay cannot hide a lost/duplicate update.
                    for n,context in enumerate(fixture['contexts'][:args.clients]):
                        detail=rpc('query.workflow_contexts',{'project_id':context['project_id'],'owner_id':context.get('owner_id',context['project_id']),'context_id':context['id']},'burst-verify',n)
                        if not detail or detail.get('context',{}).get('revision')!=versions[n]:failures.append({'burst_read_back':cycle,'client':n,'expected_revision':versions[n]})
            if time.monotonic()-start<args.seconds:time.sleep(args.seconds-(time.monotonic()-start))
        else:
            locks=[threading.Lock() for _ in range(max(1,args.clients))]
            versions=[context.get('revision',1)for context in fixture['contexts'][:args.clients]] if args.clients else [1]
            last_written=[None]*max(1,args.clients)
            read_count=int(args.seconds*args.read_rps) if args.clients else 0
            write_count=int(args.seconds*args.write_rps) if args.clients else 0
            tasks=sorted([(n/args.read_rps,'read',n) for n in range(read_count)]+[(n/args.write_rps,'write',n) for n in range(write_count)])
            scheduled_count=len(tasks)
            methods=['query.execution_workspace','query.workflow_progress','query.workflow_instance','query.history','query.tool_calls','query.workspace','query.workflow_management_summary','query.orchestration','query.workflow_driver']
            (output/'offered.json').write_text(json.dumps({'started':start,'seconds':args.seconds,'read_requests':read_count,'write_requests':write_count,'clients':args.clients,'drain_deadline_seconds':45,'all_scheduled_count':scheduled_count},indent=2))
            def execute(kind, n, scheduled):
                client=n%args.clients
                context=fixture['contexts'][client]
                if kind=='write':
                    with locks[client]:
                        result=rpc('context.update',{'project_id':context['project_id'],'command_id':str(uuid.uuid4()),'target':context['id'],'expected_revision':versions[client],'payload':{'content':f'Open-arrival client {client} write {n}'}},'write',client,scheduled)
                        if result:
                            versions[client]+=1
                            last_written[client]=f'Open-arrival client {client} write {n}'
                else:
                    frontend_roots=fixture.get('frontend_roots',fixture['roots'])
                    root=frontend_roots[(client+n//max(1,args.clients))%len(frontend_roots)]
                    method=methods[n%len(methods)]
                    params={**root,'view':'status'} if method=='query.workflow_instance' else {**root,'view':'summary'} if method=='query.workflow_driver' else root if method in ['query.workflow_progress','query.workflow_management_summary'] else {'project_id':root['project_id'],'root_instance_id':root['id']} if method=='query.orchestration' else {'project_id':root['project_id'],'limit':20,'view':'summary'} if method in ['query.history','query.tool_calls'] else {'project_id':root['project_id']}
                    result=rpc(method,params,'read',client,scheduled)
                    if result and method in ['query.workspace','query.execution_workspace'] and result['active_project']['id']!=root['project_id']:
                        failures.append({'client':client,'wrong_project':method})
            pool=ThreadPoolExecutor(max_workers=128)
            try:
                futures=[]
                for at,kind,n in tasks:
                    remaining=start+at-time.monotonic()
                    if remaining>0: time.sleep(remaining)
                    futures.append(pool.submit(execute,kind,n,start+at))
                if not tasks: time.sleep(args.seconds)
                work_offer_end=committed_work()
                done,pending=wait(futures,timeout=45)
                if pending:
                    failures.append({'generator_drain_deadline':True,'unfinished_at_cutoff':len(pending)})
                for future,(at,kind,n) in zip(futures,tasks):
                    if future in pending and future.cancel():
                        cutoff=time.monotonic()
                        row={'method':'context.update' if kind=='write' else methods[n%len(methods)],'label':kind,'client':n%args.clients,'scheduled':start+at,'done':cutoff,'not_sent':True,'error':{'code':'generator_drain_deadline'},'ms':None,'scheduled_ms':(cutoff-start-at)*1000,'generator_queue_ms':(cutoff-start-at)*1000,'censored_at_cutoff':True}
                        failures.append(row);record_rpc(row)
                for future in futures:
                    if future.cancelled():continue
                    future.result()
            finally:
                pool.shutdown(wait=True,cancel_futures=True)
            for client,version in enumerate(versions[:args.clients]):
                # A committed read validates every independent edit, not just its HTTP receipt.
                context=fixture['contexts'][client]
                detail=rpc('query.workflow_contexts',{'project_id':context['project_id'],'owner_id':context.get('owner_id',context['project_id']),'context_id':context['id']},'verify',client)
                actual=detail.get('context',{}) if detail else {}
                valid=actual.get('id')==context['id'] and actual.get('revision')==version and (last_written[client] is None or actual.get('content')==last_written[client])
                with (output/'committed-context-readbacks.jsonl').open('a') as stream:stream.write(json.dumps({'client':client,'project_id':context['project_id'],'context_id':context['id'],'expected_revision':version,'actual_revision':actual.get('revision'),'last_content_matches':last_written[client] is None or actual.get('content')==last_written[client],'passed':valid})+'\n')
                if not valid:
                    failures.append({'client':client,'expected_revision':version,'read_back':detail})
        if len(reconnects) != reconnect_clients * 10:
            failures.append({'incomplete_reconnections':len(reconnects),'expected':reconnect_clients * 10})
        renew_stop.set()
        if renewal_thread:
            renewal_thread.join(timeout=35)
            if renewal_thread.is_alive(): failures.append({'renewal_did_not_stop':True})
        after=progress()
        work_after=committed_work()
        stop.set()
        (output/'progress-after.json').write_text(json.dumps(after))
        advances=[e['counts']['completed']-b['counts']['completed'] for b,e in zip(before,after)]
        if any(n<=0 for n in advances): failures.append({'background_stalled':advances})
        if any(e['record']['data']['status'] in ['completed','cancelled'] for e in after): failures.append({'work_drained':True})
        if any(e['counts']['failed'] or e['record']['data']['status'] in ['failed','needs_reconciliation'] for e in after): failures.append({'workflow_failure':after})
        summary={'local_only':True,'formal_p6':False,'workload':'W1 test ports, real HTTP/core/SQLite/scheduler','clients':args.clients,'seconds':args.seconds,'scale':args.scale,'projects':args.projects,'read_rps':args.read_rps,'write_rps':args.write_rps,'scheduled':scheduled_count,'workflow_advances':advances,'failure_count':len(failures),'failures':failures,'host_rss_peak_kib':max((int(s['host_ps'][1]) for s in samples if len(s['host_ps'])>1),default=0),'max_wal_bytes':max((s['max_wal_bytes'] for s in samples),default=0),'identity':{'binary_sha256':hashlib.sha256(binary.read_bytes()).hexdigest(),'dataset':str(dataset)}}
        summary['arrival_mode']='synchronized_burst' if args.burst_cycles else 'browser_actions' if args.browser else 'open_arrival'
        summary['write_targets_by_project']={project:sum(c['project_id']==project for c in fixture['contexts'][:args.clients])for project in fixture['projects']}
        summary['read_requests_by_project']={project:sum(r.get('project_id')==project for r in rows if r['label']=='read')for project in fixture['projects']}
        if args.burst_cycles:summary['read_rps']=summary['write_rps']=None
        for label in ['read','write']:
            selected=[r for r in rows if r['label']==label]
            attempted=[r for r in selected if not r.get('not_sent')]
            summary[label]=stats([r['ms'] for r in attempted]);summary[label]['scheduled_latency']=stats([r['scheduled_ms'] for r in attempted]);summary[label]['generator_queue']=stats([r['generator_queue_ms'] for r in attempted]);summary[label]['bytes_max']=max((r.get('bytes',0) for r in attempted),default=0);summary[label]['not_sent']=sum(bool(r.get('not_sent')) for r in selected);summary[label]['failed_attempts']=sum(bool(r.get('error') or r.get('exception')) for r in attempted)
        summary['committed_work_before']=work_before
        summary['committed_work_after']=work_after
        if work_offer_end:
            summary['committed_work_offer_end']=work_offer_end
            summary['offered_window_work_per_second']={key:(work_offer_end[key]-value)/(work_offer_end['time']-work_before['time']) for key,value in work_before.items() if key!='time'}
        summary['committed_work_delta']={key:work_after[key]-value for key,value in work_before.items() if key!='time'}
        summary['committed_work_per_second']={key:value/(work_after['time']-work_before['time']) for key,value in summary['committed_work_delta'].items()}
        summary['work_telemetry_scope']='Separate read-only SQLite snapshots; workflow_step sum(revision-1) counts durable logical step changes; state_commits counts the one same-transaction WorkflowCommit method event for dispatch/accept/changed observation/completion, not empty ticks. New roots start at revision 1. Docker count requires succeeded actual tests.run platform calls.'
        summary['event_commit_field_to_receive']=event_stats()
        summary['event_timing_boundary']='Same-machine wall clock from persisted pre-COMMIT Event.time to complete SSE data line; conservative, not exact COMMIT or browser rendering latency.'
        summary['sse_reconnections']={'enabled':args.sse_reconnect,'completed':len(reconnects),'expected':reconnect_clients*10,'clients':reconnect_clients,'state_read':stats([r['state_read_ms'] for r in reconnects])}
        if args.sse_reconnect:
            (output/'sse-reconnections.json').write_text(json.dumps(reconnects, indent=2))
        (output/'reconnections.jsonl').write_text(''.join(json.dumps(r)+'\n' for r in reconnects))
        summary['elapsed_seconds']=time.monotonic()-start
        summary['replenished']=args.renew
        if args.trace:
            checkpoints = checkpoint_after_load(dataset)
            summary['passive_checkpoints_after_load'] = checkpoints
            log.flush()
            summary['storage_trace'] = storage_trace(output, rows, args.browser)
            (output/'checkpoint-after-load.json').write_text(json.dumps(checkpoints, indent=2))
        timing_start=start_wall_ms
        if args.browser and (output/'browser-window.json').exists():timing_start=json.loads((output/'browser-window.json').read_text())['began']
        dispatches=dispatch_latency(dataset,timing_start,timing_start+args.seconds*1000)
        (output/'dependency-dispatch.jsonl').write_text(''.join(json.dumps(row)+'\n' for row in dispatches))
        summary['dependency_accept_to_dispatch']=stats([row['ms'] for row in dispatches])
        summary['dependency_dispatch_timing_scope']='Fixed benchmark chains only; dependency accept record.created_at to successor started wall-clock fields, both pre-COMMIT. Not exact SQL COMMIT or pure scheduler queue time. Initial nodes omitted; only successor starts within the offered window.'
        supply=pending_root_supply(dataset,slot_roots,timing_start,timing_start+args.seconds*1000)
        (output/'pending-root-supply.json').write_text(json.dumps(supply,indent=2))
        summary['pending_root_supply']=supply
        if supply['below_four_intervals'] or supply['unobserved_roots']:
            failures.append({'pending_work_supply_not_proven':supply})
            summary['failure_count']=len(failures)
        if args.real_runtime:
            summary['workload']='W2 production Host, actual Git/Docker/Artifact'
            summary['identity']['fixture_binary_sha256']=summary['identity']['binary_sha256']
            summary['identity']['binary_sha256']=hashlib.sha256(production.read_bytes()).hexdigest()
        summary['by_method']={m:stats([r['ms'] for r in rows if r['method']==m and r['label'] in ['read','write'] and not r.get('not_sent')]) for m in sorted({r['method'] for r in rows})}
        for method,measurement in summary['by_method'].items():
            selected=[r for r in rows if r['method']==method and r['label'] in ['read','write']]
            measurement.update(bytes_max=max((r.get('bytes',0) for r in selected),default=0),not_sent=sum(bool(r.get('not_sent')) for r in selected),failed=sum(bool(r.get('error') or r.get('exception')) for r in selected),scheduled_latency=stats([r['scheduled_ms'] for r in selected if not r.get('not_sent')]))
        if args.burst_cycles:
            summary['burst']={'cycles':args.burst_cycles,'rounds':burst_results,'read_group':stats([r['read_group_ms'] for r in burst_results]),'write_group':stats([r['write_group_ms'] for r in burst_results]),'compete_group':stats([r['compete_group_ms'] for r in burst_results]),'expected_revision_conflicts':sum(bool(r.get('expected_error')) for r in rows)}
        (output/'summary.json').write_text(json.dumps(summary,indent=2))
        print(json.dumps(summary,indent=2))
        if failures: raise RuntimeError('Joint load has failures; retained all samples')
    finally:
        renew_stop.set()
        stop.set()
        # Stop the private Host first so blocked SSE reads terminate cleanly.
        if server.poll() is None:
            server.send_signal(signal.SIGINT)
            try: server.wait(timeout=30)
            except subprocess.TimeoutExpired: server.kill(); server.wait()
        for thread in sse_threads:thread.join(timeout=2)
        for response in streams: response.close()
        event_sink.close()
        log.close()
        stopped=validate_stopped_fixture(dataset,fixture,args.real_runtime)
        (output/'post-stop-validation.json').write_text(json.dumps(stopped,indent=2))
        if 'summary' in locals():
            summary['post_stop_validation']=stopped
            summary['cleanup_failure_count']=len(stopped['errors'])
            if stopped['errors']:
                failures.append({'post_stop_validation_errors':stopped['errors']})
                summary['failure_count']=len(failures)
            (output/'summary.json').write_text(json.dumps(summary,indent=2))
        if stopped['errors'] and not sys.exc_info()[0]:raise RuntimeError('Private fixture post-stop integrity or worker checks failed; all evidence retained')


if __name__=='__main__':
    parser=argparse.ArgumentParser(description=__doc__)
    parser.add_argument('--test-binary',type=Path,required=True)
    parser.add_argument('--output',type=Path,required=True)
    parser.add_argument('--clients',type=int,default=100,choices=[0,1,2,4,10,16,25,50,100])
    parser.add_argument('--seconds',type=int,default=30)
    parser.add_argument('--read-rps',type=float,default=100)
    parser.add_argument('--write-rps',type=float,default=10)
    parser.add_argument('--scale',type=int,choices=[0,1,10],default=0)
    parser.add_argument('--projects',type=int,choices=[2,10],default=2)
    parser.add_argument('--browser',action='store_true')
    parser.add_argument('--trace',action='store_true')
    parser.add_argument('--sse-reconnect',action='store_true',help='Ten disconnect/resume/state-read cycles per selected subscriber; use a 120s or longer load')
    parser.add_argument('--sse-configuration',action='store_true',help='Subscribe to the catalog clock as the Web application does')
    parser.add_argument('--reconnecting-clients',type=int,help='Number of subscribers to reconnect; defaults to all')
    parser.add_argument('--burst-cycles',type=int,default=0,help='Separate synchronized read, independent write, same-version competition and exact replay rounds')
    parser.add_argument('--renew',action='store_true',help='Replenish only the deterministic Rust fixture before its finite control roots drain')
    parser.add_argument('--real-runtime',action='store_true',help='Actual Git/Docker with the production Host after private fixture preparation')
    parser.add_argument('--hot-project',action='store_true',help='Four active roots and 100 distinct editable targets in one project')
    parser.add_argument('--host-binary',type=Path)
    parser.add_argument('--test-image')
    args=parser.parse_args()
    if args.sse_reconnect and (args.seconds<120 or args.browser): parser.error('Reconnect scene needs at least 120 seconds of protocol load')
    if args.reconnecting_clients is not None and (not args.sse_reconnect or not 1<=args.reconnecting_clients<=args.clients): parser.error('Select between 1 and the total client count when reconnecting')
    if args.real_runtime and args.renew: parser.error('Renewal is limited to the W1 deterministic fixture')
    if args.burst_cycles and (args.browser or args.clients!=100 or not 1<=args.burst_cycles<=30): parser.error('Burst scene requires 100 protocol clients and 1..30 cycles')
    if not 10<=args.seconds<=3600 or args.read_rps<0 or args.write_rps<0 or (args.clients and (args.read_rps==0 or args.write_rps==0)): parser.error('Invalid duration/rate')
    run(args)

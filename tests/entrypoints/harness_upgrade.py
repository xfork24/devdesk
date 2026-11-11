#!/usr/bin/env python3
"""Real offline SQLite conversion and new Host acceptance; no model/network."""
import fcntl
import hashlib
import json
from pathlib import Path
import shutil
import sqlite3
import subprocess
import sys
import tempfile
import time
import uuid

sys.path.insert(0,str(Path(__file__).resolve().parents[1]/"live"))
from client import Client

ROOT=Path(__file__).resolve().parents[2]
BINARY=Path(sys.argv[1] if len(sys.argv)>1 else ROOT/"target/debug/devdesk-host").resolve()


def encoded(value):
    return json.dumps(value,ensure_ascii=False,sort_keys=True,separators=(",",":"))


def record(ident,kind,definition,scope,data):
    return (ident,kind,definition,encoded(scope),"升级保留",1,None,hashlib.sha256(encoded(data).encode()).hexdigest(),encoded(data),kind not in ("workflow_instance","template_catalog_state"),1,1)


def fixture(directory):
    root=directory/"data";root.mkdir()
    (root/".devdesk-dataset").write_text("devdesk-schema-v1\n")
    project=str(uuid.uuid4());run=str(uuid.uuid4());workflow=str(uuid.uuid4());execution=str(uuid.uuid4())
    project_dir=root/"projects"/project;project_dir.mkdir(parents=True)
    (project_dir/".devdesk-dataset").write_text("devdesk-schema-v1\n")
    (project_dir/"artifacts").mkdir();(project_dir/"artifacts/retained.txt").write_text("不可删除的原始内容")
    (root/"credentials").mkdir();(root/"credentials/retained.txt").write_text("fixture credential sentinel")
    project_value=dict(id=project,title="旧工程",workspace="",workspace_definition=dict(repository="",path="",mainline="main"),execution_mode="live",concurrency=2,revision=1)
    document=dict(schema_version=4,stages=[dict(key="work",name="旧阶段")],steps=[dict(key="gate",name="旧步骤",stage="work",executor=dict(type="control"))])
    workflow_row=record(workflow,"workflow",workflow,{"kind":"global"},document)
    export=directory/"factory.json"
    subprocess.run([str(BINARY),"--factory-export",str(export)],check=True,capture_output=True)
    defaults=next(r["data"]["defaults"] for r in json.loads(export.read_text())["governance"] if r["kind"]=="template_catalog_state")
    defaults={k:v for k,v in defaults.items() if k not in ("control_policy","orchestrator_role_ref")}
    defaults["workflow_ref"]=workflow
    catalog_row=record("template-catalog-state","template_catalog_state","template-catalog-state",{"kind":"global"},{"generation":2,"initial_generation":1,"bundle_hash":"legacy","bundle_version":"legacy","defaults":defaults})
    member_row=record("legacy-workflow-member","template_catalog_member",workflow,{"kind":"global"},{"kind":"workflow","generation":2,"origin":"custom","template_key":None,"source_hash":None})
    global_conn=sqlite3.connect(root/"global.sqlite3")
    global_conn.executescript((ROOT/"tests/fixtures/upgrade-v4/global.sql").read_text())
    global_conn.execute("INSERT INTO project_catalog VALUES (?,?,?,?)",(project,str(uuid.uuid4()),"fixture",encoded(project_value)))
    for row in (workflow_row,catalog_row,member_row):global_conn.execute("INSERT INTO governance_records VALUES (?,?,?,?,?,?,?,?,?,?,?,?)",row)
    global_conn.commit();global_conn.close()
    conn=sqlite3.connect(project_dir/"engineering.sqlite3")
    conn.executescript((ROOT/"tests/fixtures/upgrade-v4/project.sql").read_text())
    conn.execute("INSERT INTO projects VALUES (?,?,?,?,?,?,?)",(project,"旧工程","","live",2,1,encoded(project_value["workspace_definition"])))
    for row in (workflow_row,catalog_row,member_row):conn.execute("INSERT INTO governance_records VALUES (?,?,?,?,?,?,?,?,?,?,?,?)",row)
    keys=["id","kind","definition_id","scope","name","version","supersedes","content_hash","data","immutable","revision","created_at"]
    definition=dict(zip(keys,workflow_row));definition["scope"]={"kind":"global"};definition["data"]=document
    step=dict(id=execution,workflow_instance_id=run,workflow_revision_id=workflow,path="/gate",step="gate",round=1,sequence=1,status="queued",input_digest="",inputs={},outputs={},agent_revision_id=None,session_id=None,turn_id=None,tool_call_id=None,started=None,finished=None,reason="",budget_wait_started=None,budget_waited_ms=0)
    plan=dict(workflow_revision_id=workflow,document=document,roles={},definitions={workflow:definition},permission_locks={},tool_bindings={},integration_locks={},specification_snapshot=[])
    runtime=dict(schema_version=4,owner_id=project,workflow_ref=workflow,project_id=project,plan=plan,role_agent_bindings={},inputs={},steps=[step],status="queued",paused=False,parent_execution_id=None,root_instance_id=run,cancel_requested=False)
    conn.execute("INSERT INTO governance_records VALUES (?,?,?,?,?,?,?,?,?,?,?,?)",record(run,"workflow_instance",project,{"kind":"project","project_id":project},runtime))
    legacy_intent={"status":"prepared","configured":True,"method":"workflow.pause","command":{"command_id":"legacy-poison-pause","target":run,"expected_revision":1,"payload":{}}}
    conn.execute("INSERT INTO governance_records VALUES (?,?,?,?,?,?,?,?,?,?,?,?)",record("old-prepared-intent","workflow_intent",execution,{"kind":"project","project_id":project},legacy_intent));conn.commit();conn.close()
    return root,project,run,execution


with tempfile.TemporaryDirectory(prefix="devdesk-upgrade-contract-") as tmp:
    directory=Path(tmp);root,project,run,execution=fixture(directory)
    command=[sys.executable,str(ROOT/"scripts/upgrade/harness-v5.py"),str(root),"--binary",str(BINARY)]
    before=hashlib.sha256((root/"global.sqlite3").read_bytes()).hexdigest()
    subprocess.run(command,check=True,capture_output=True)
    assert hashlib.sha256((root/"global.sqlite3").read_bytes()).hexdigest()==before
    with (root/".devdesk.lock").open("a+") as held:
        fcntl.flock(held,fcntl.LOCK_EX|fcntl.LOCK_NB)
        assert subprocess.run(command,capture_output=True).returncode!=0
    subprocess.run(command+["--apply","--backup-directory",str(directory/"backup"),"--max-databases","1"],check=True)
    assert json.loads((root/"harness-v5-upgrade.json").read_text())["state"]=="running"
    blocked=subprocess.run([str(BINARY),"--data",str(root),"--port","0"],capture_output=True,text=True,timeout=20)
    assert blocked.returncode and "升级" in blocked.stderr+blocked.stdout
    subprocess.run(command+["--apply"],check=True)
    subprocess.run(command+["--apply"],check=True,capture_output=True)
    assert (root/"credentials/retained.txt").read_text()=="fixture credential sentinel"
    assert (root/"projects"/project/"artifacts/retained.txt").read_text()=="不可删除的原始内容"
    for path,version in [(root/"global.sqlite3",14),(root/"projects"/project/"engineering.sqlite3",45)]:
        with sqlite3.connect(path) as conn:
            assert conn.execute("PRAGMA user_version").fetchone()[0]==version
            assert conn.execute("PRAGMA integrity_check").fetchone()[0]=="ok"
            assert conn.execute("SELECT count(*) FROM upgrade_archive").fetchone()[0]>0
            assert conn.execute("SELECT count(*) FROM governance_records WHERE kind='workflow_intent'").fetchone()[0]==0
            assert conn.execute("SELECT count(*) FROM sqlite_master WHERE type='trigger' AND name IN ('governance_identity_update','governance_immutable_update')").fetchone()[0]==2
            if conn.execute("SELECT 1 FROM governance_records WHERE id='old-prepared-intent'").fetchone():
                try:conn.execute("UPDATE governance_records SET name='tampered' WHERE id='old-prepared-intent'")
                except sqlite3.IntegrityError:pass
                else:raise AssertionError('archived intent must remain immutable')
    log=directory/"host.log"
    with log.open("w") as output:
        process=subprocess.Popen([str(BINARY),"--data",str(root),"--port","0"],stdout=output,stderr=subprocess.STDOUT)
    try:
        for _ in range(100):
            urls=[line for line in log.read_text().splitlines() if line.startswith("http://")]
            if urls:break
            if process.poll() is not None:raise AssertionError(log.read_text())
            time.sleep(.1)
        c=Client(urls[0]);c.open(project)
        defaults=c.rpc("query.configuration_defaults")
        assert defaults["catalog_generation"]==2
        visible={r["id"] for r in c.rpc("query.configurations",{"limit":1000})["records"]}
        assert defaults["workflow_ref"] in visible
        assert defaults["role_agent_bindings"][defaults["orchestrator_role_ref"]] in visible
        view=c.rpc("query.workflow_instance",{"id":run})
        assert view["runtime"]["schema_version"]==5
        assert view["runtime"]["paused"] is False
        assert view["runtime"]["steps"][0]["id"]==execution
        plan=view["runtime"]["active_plan_revision_id"]
        dispatched=c.command("workflow.step.dispatch",run,view["record"]["revision"],{"step":"gate","plan_revision_id":plan})
        step=dispatched["execution"];assert step["status"]=="awaiting_acceptance"
        view=c.rpc("query.workflow_instance",{"id":run})
        c.command("workflow.step.accept",run,view["record"]["revision"],{"step":"gate","plan_revision_id":plan,"execution_id":step["id"],"input_digest":step["input_digest"],"outputs":step["outputs"]})
        view=c.rpc("query.workflow_instance",{"id":run});c.command("workflow.complete",run,view["record"]["revision"],{"plan_revision_id":plan,"reason":"升级后继续执行"})
    finally:
        process.terminate();process.wait(timeout=20)
    print("PASS offline v4 conversion, archives, idempotent resume, retained data and real Host continuation")

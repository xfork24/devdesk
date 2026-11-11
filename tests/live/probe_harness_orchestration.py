#!/usr/bin/env python3
"""Real Codex manager: runtime bypass, worker result, pause/restart and continuation."""
import argparse
import json
import shutil
import signal
import subprocess
import tempfile
import time
from pathlib import Path
from client import Client

parser=argparse.ArgumentParser(description=__doc__)
parser.add_argument("--binary",required=True,type=Path)
parser.add_argument("--codex-auth-source",required=True,type=Path)
parser.add_argument("--report-directory",type=Path)
args=parser.parse_args()
OUT=(args.report_directory or Path(tempfile.mkdtemp(prefix="devdesk-harness-native-"))).resolve();OUT.mkdir(parents=True,exist_ok=True)
private=OUT/"devdesk-host";shutil.copy2(args.binary.resolve(),private)
process=None


def boot():
    global process
    log=OUT/("host-"+str(time.time_ns())+".log")
    process=subprocess.Popen([str(private),"--data",str(OUT/"data"),"--port","0"],stdout=log.open("w"),stderr=subprocess.STDOUT)
    for _ in range(150):
        urls=[line for line in log.read_text().splitlines() if line.startswith("http://")]
        if urls:return Client(urls[0])
        if process.poll() is not None:raise RuntimeError(log.read_text())
        time.sleep(.1)
    raise TimeoutError("Host startup")


def controller_command(client,method,controller,payload=None):
    current=client.rpc("query.orchestration",{"id":controller})["record"]
    return client.command(method,controller,current["revision"],{"epoch":current["data"]["epoch"],**(payload or {})})


def wait_phase(client,run,predicate,timeout=900):
    deadline=time.monotonic()+timeout;last=None
    while time.monotonic()<deadline:
        view=client.rpc("query.workflow_instance",{"id":run})
        manager=client.rpc("query.orchestration",{"root_instance_id":run})
        status=[(s["step"],s["status"]) for s in view["runtime"]["steps"]]
        marker=(status,manager.get("record",{}).get("data",{}).get("status"))
        if marker!=last:print(marker,flush=True);last=marker
        (OUT/"latest-workflow.json").write_text(json.dumps(view,ensure_ascii=False,indent=2))
        (OUT/"latest-manager.json").write_text(json.dumps(manager,ensure_ascii=False,indent=2))
        for step in view["runtime"]["steps"]:
            if step["status"] in ("failed","needs_reconciliation","cancelled"):raise RuntimeError(step)
        if manager.get("record",{}).get("data",{}).get("status") in ("failed","budget_exhausted","needs_reconciliation"):raise RuntimeError(manager["record"]["data"])
        if predicate(view,manager):return view,manager
        time.sleep(.6)
    raise TimeoutError("native manager phase")


try:
    print("Evidence:",OUT,flush=True)
    c=boot();auth=OUT/"data/credentials/codex/auth.json";auth.parent.mkdir(parents=True,exist_ok=True);shutil.copy2(args.codex_auth_source.resolve(),auth);auth.chmod(0o600)
    c.rpc("runtime.prepare",{"provider":"codex"})
    defaults=c.rpc("query.configuration_defaults")
    native_tools=next(r["data"]["tools"] for r in defaults["records"] if r["id"]==defaults["capability_ref"])
    project=c.command("project.create",payload={"title":"Harness 原生编排验收"})["id"];c.project=project
    project_row=c.rpc("query.workspace")["active_project"];c.command("project.update",project,project_row["revision"],{"concurrency":1})
    role=c.command("role.create",payload={"name":"数据计算员","data":{"instructions":"只计算当前输入的数字，使用真实平台材料捕获工具并按输出合同交付。"}})
    capture=next(r["id"] for r in defaults["records"] if r["kind"]=="tool" and r["data"].get("qualified_name")=="materials.capture")
    capability=c.command("capability.create",payload={"name":"计算能力","data":{"model_ref":defaults["model_ref"],"harness_ref":defaults["harness_ref"],"skills":[],"tools":list(dict.fromkeys(native_tools+[capture]))}})
    agent=c.command("agent.create",payload={"name":"计算 Agent","data":{"role_ref":role["id"],"capability_ref":capability["id"]}})
    spec=c.command("spec.create",payload={"name":"排序分析内容规范","data":{"schema_version":3,"body":"输入整数按降序排列，保留重复值；sum 等于所有输入之和。结果可追溯当前 numbers 输入。","checks":[{"key":"calculation","name":"计算结果","instructions":"逐项核对降序、重复值、总和与实际输入一致。"}]}})
    delivery=c.command("delivery_type.create",payload={"name":"计算结果","data":{"spec_refs":[spec["id"]],"body":"降序及求和","format":"json","content_schema":{"type":"object","required":["ordered","sum"],"properties":{"ordered":{"type":"array","items":{"type":"integer"}},"sum":{"type":"integer"}},"additionalProperties":False}}})
    document={"schema_version":6,"stages":[{"key":"study","name":"计算"},{"key":"optional_check","name":"可选核对"},{"key":"handoff","name":"交接"}],"steps":[
        {"key":"calculate","name":"计算数字","stage":"study","executor":{"type":"agent","role_ref":role["id"]},"task_instructions":"将 inputs.numbers 降序排列并求和。必须实际调用 devdesk_materials_capture，参数直接为 ordered 和 sum。收到成功回执后以 result 端口提交相同对象。","inputs":{"numbers":{"type":"constant","value":[3,1,2,3]}},"outputs":{"result":{"delivery_ref":delivery["id"]}},"budget":{"max_turns":3,"timeout_seconds":300,"retry_limit":1}},
        {"key":"optional","name":"可选检查","stage":"optional_check","executor":{"type":"control"},"dependencies":["calculate"]},
        {"key":"finish","name":"确认交接","stage":"handoff","executor":{"type":"control"},"dependencies":["optional"],"inputs":{"analysis":{"type":"source","source":{"step":"calculate","port":"result"}}}}
    ],"outputs":{"result":{"step":"calculate","port":"result"}}}
    draft=c.command("workflow.create",payload={"name":"Harness 控制的数字工作流","document":document});workflow=c.command("workflow.publish",draft["id"],draft["revision"])
    context=next(r for r in defaults["records"] if r["id"]==defaults["permission_context_ref"])["data"]
    permission=c.command("permission_context.create",payload={"name":"Native calculation authorization","data":{**context,"step_grants":[{"workflow_ref":workflow["id"],"step":"calculate","tool_refs":[capture]}]}})
    bindings={role["id"]:agent["id"],**defaults["role_agent_bindings"]}
    p=c.rpc("query.workspace")["active_project"]
    config=c.command("project.configuration.update",project,p["revision"],{"workflow_ref":workflow["id"],"role_agent_bindings":bindings,"orchestrator_role_ref":defaults["orchestrator_role_ref"],"permission_context_ref":permission["id"]})
    p=c.rpc("query.workspace")["active_project"];manager=c.command("orchestration.open",project,p["revision"],{})["id"]
    controller_command(c,"orchestration.input",manager,{"input":"请读取本项目当前配置，通过 workflow.start 建立它的工作流。只建立根实例，保持所有业务步骤 queued；不派发任何步骤，不调整计划，不 complete。建立后结束 Turn 等待我继续。"})
    def started():
        view=c.rpc("query.orchestration",{"id":manager});record=view["record"]["data"]
        if record["status"] in ("failed","needs_reconciliation","budget_exhausted"):raise RuntimeError(record)
        sid=record["session_id"]
        return view if record["root_instance_id"] and sid and all(t["status"] not in ("queued","dispatching","running","waiting_interaction","needs_reconciliation") for t in c.rpc("query.session",{"id":sid})["turns"]) else None
    deadline=time.monotonic()+600
    while time.monotonic()<deadline:
        startup=started()
        if startup:break
        time.sleep(.6)
    else:raise TimeoutError("native workflow.start")
    run=startup["record"]["data"]["root_instance_id"];startup_calls=startup["calls"]
    initial=c.rpc("query.workflow_instance",{"id":run});assert all(s["status"]=="queued" for s in initial["runtime"]["steps"])
    print("PASS native workflow.start with all business nodes queued",run,flush=True)
    controller_command(c,"orchestration.input",manager,{"input":"先通过 workflow.change.preview 和 workflow.change.apply 在运行时绕过 optional_check 阶段。optional 没有向下游供给输出，finish 的 analysis 仍来自 calculate，不需要替代材料。随后仅派发 calculate，检查真实交付并接纳。完成 calculate 后调用 workflow.pause 暂停流程，不派发 finish，不 complete，结束当前 Turn 等待我继续。不得伪造计算或工具回执。"})
    def first_done(view,manager):
        calculated=next(s for s in view["runtime"]["steps"] if s["step"]=="calculate")
        optional=next(s for s in view["runtime"]["steps"] if s["step"]=="optional")
        finish=next(s for s in view["runtime"]["steps"] if s["step"]=="finish")
        if calculated["status"]!="completed" or optional["status"]!="skipped" or finish["status"]!="queued" or not view["runtime"]["paused"]:return False
        sid=manager["record"]["data"]["session_id"]
        session=c.rpc("query.session",{"id":sid})
        return not any(t["status"] in ("queued","dispatching","running","waiting_interaction","needs_reconciliation") for t in session["turns"])
    before,before_manager=wait_phase(c,run,first_done)
    assert before["runtime"]["active_plan_revision_id"]!=initial["runtime"]["active_plan_revision_id"]
    source=next(s for s in before["runtime"]["steps"] if s["step"]=="calculate")
    value=before["outputs"][source["outputs"]["result"]]["value"];assert value=={"ordered":[3,3,2,1],"sum":9},value
    (OUT/"before-restart.json").write_text(json.dumps(before,ensure_ascii=False,indent=2))
    controller_command(c,"orchestration.takeover",manager)
    sid=before_manager["record"]["data"]["session_id"]
    c.wait(lambda:c.rpc("query.session",{"id":sid}),lambda v:all(t["status"] not in ("queued","dispatching","running","waiting_interaction","needs_reconciliation") for t in v["turns"]) and all(e["writer_revoked"] for e in v["executions"]),timeout=60)
    process.send_signal(signal.SIGINT);process.wait(timeout=30)
    c=boot();c.open(project)
    resumed=c.rpc("query.workflow_instance",{"id":run});assert resumed["runtime"]["active_plan_revision_id"]==before["runtime"]["active_plan_revision_id"]
    controller_command(c,"orchestration.resume",manager)
    controller_command(c,"orchestration.input",manager,{"input":"现在继续：恢复暂停的流程，保留已接纳 calculate 的原始执行和产物，禁止重算，也不要执行已绕过的 optional。只派发并接纳 finish，然后使用 workflow.complete 完成根实例。"})
    after,manager_view=wait_phase(c,run,lambda view,manager:view["runtime"]["status"]=="completed" and any(r["data"]["method"]=="workflow.complete" and r["data"]["status"]=="succeeded" for r in manager["calls"]))
    final_source=next(s for s in after["runtime"]["steps"] if s["step"]=="calculate");assert {k:v for k,v in final_source.items() if not k.startswith("budget_wait")}=={k:v for k,v in source.items() if not k.startswith("budget_wait")}
    assert next(s for s in after["runtime"]["steps"] if s["step"]=="finish")["inputs"]["analysis"]==value
    calls=list({r["id"]:r for r in startup_calls+before_manager["calls"]+manager_view["calls"]}.values())
    methods={r["data"]["method"] for r in calls if r["data"]["status"]=="succeeded"}
    assert {"workflow.start","workflow.change.preview","workflow.change.apply","workflow.step.dispatch","workflow.step.accept","workflow.pause","workflow.resume","workflow.complete"}<=methods,methods
    assert manager_view["record"]["data"]["predecessor_session_id"] and manager_view["record"]["data"]["snapshot_id"]
    worker_calls=c.rpc("query.tool_calls",{"turn_id":source["turn_id"]})["records"]
    assert any(r["kind"]=="platform_call" and r["data"]["status"]=="succeeded" for r in worker_calls)
    (OUT/"after-restart.json").write_text(json.dumps(after,ensure_ascii=False,indent=2))
    (OUT/"controller-calls.json").write_text(json.dumps(calls,ensure_ascii=False,indent=2))
    manager_sessions={manager_view["record"]["data"][key] for key in ("session_id","predecessor_session_id")}
    reply_sizes=[]
    for journal in (OUT/"data/projects").glob("*/execution/sessions/*/coordinator/*.jsonl"):
        if journal.parent.parent.name not in manager_sessions:continue
        for line in journal.read_text().splitlines():
            entry=json.loads(line);frame=entry.get("frame",{});result=frame.get("result")
            if entry.get("direction")=="out" and isinstance(result,dict) and isinstance(result.get("contentItems"),list):reply_sizes.append(len(result["contentItems"]))
    assert reply_sizes and all(size==1 for size in reply_sizes),reply_sizes
    (OUT/"native-reply-shape.json").write_text(json.dumps({"passed":True,"content_item_counts":reply_sizes},indent=2))
    (OUT/"report.md").write_text("# Harness 原生验收\n\n真实 Codex 项目经理通过原生工具修改运行计划、绕过阶段、派发并接纳实际计算、暂停后重启并继续完成。原始执行和产物保持不变。\n\nRoot: "+run+"\n")
    print("PASS real Harness orchestration",run,flush=True)
except BaseException as error:
    (OUT/"failure.txt").write_text(str(error));raise
finally:
    if process is not None and process.poll() is None:process.send_signal(signal.SIGINT);process.wait(timeout=30)
    shutil.rmtree(OUT/"data/credentials",ignore_errors=True)
    for path in (OUT/"data").rglob("auth.json"):path.unlink(missing_ok=True)
    print("Evidence:",OUT,flush=True)

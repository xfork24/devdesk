#!/usr/bin/env python3
"""Real managed Agent: no-project discussion, project experience, detached read and restart."""
import argparse
import hashlib
import json
import shutil
import signal
import subprocess
import time
import uuid
from pathlib import Path
from client import Client

parser = argparse.ArgumentParser(description=__doc__)
parser.add_argument("--binary", required=True, type=Path)
parser.add_argument("--reuse-dataset", action="store_true", help="Retry a failed preparation in this probe's dedicated dataset")
parser.add_argument("--model-provider", choices=["codex", "minimax-cn"], default="codex")
parser.add_argument("--codex-auth-source", type=Path)
parser.add_argument("--api-key-file", type=Path)
parser.add_argument("--report-directory", required=True, type=Path)
args = parser.parse_args()
out = args.report_directory.resolve()
out.mkdir(parents=True, exist_ok=True)
assert args.reuse_dataset or not (out / "data").exists(), "Use a fresh, dedicated dataset."
binary = out / "devdesk-host"
shutil.copy2(args.binary.resolve(), binary)
process = None


def boot():
    global process
    log = out / f"host-{time.time_ns()}.log"
    process = subprocess.Popen([str(binary), "--data", str(out / "data"), "--port", "0"], stdout=log.open("w"), stderr=subprocess.STDOUT)
    for _ in range(150):
        urls = [line for line in log.read_text().splitlines() if line.startswith("http://")]
        if urls:
            return Client(urls[0])
        if process.poll() is not None:
            raise RuntimeError(log.read_text())
        time.sleep(0.1)
    raise TimeoutError("Host startup")


def stop():
    if process and process.poll() is None:
        process.send_signal(signal.SIGINT)
        process.wait(timeout=30)


def scope(global_scope):
    return {"scope": "global"} if global_scope else {}


def talk(client, session, prompt, global_scope, timeout=300):
    view = client.rpc("query.session", {"id": session, **scope(global_scope)})
    result = client.rpc("conversation.send", {**scope(global_scope), "command_id": str(uuid.uuid4()), "target": session, "expected_revision": view["session"]["revision"], "payload": {"client_message_id": str(uuid.uuid4()), "input": prompt}})
    session = result.get("effective_session_id", session)
    deadline = time.monotonic() + timeout
    while time.monotonic() < deadline:
        view = client.rpc("query.session", {"id": session, "limit": 100, **scope(global_scope)})
        turn = next(row for row in view["turns"] if row["id"] == result["id"])
        if turn["status"] in ("failed", "cancelled", "needs_reconciliation"):
            (out / "failed-turn.json").write_text(json.dumps(view, ensure_ascii=False, indent=2))
            raise AssertionError(f"Native turn {turn['id']} ended as {turn['status']}")
        if turn["status"] == "completed":
            return session, view
        time.sleep(0.5)
    raise TimeoutError("Native discussion deadline")


try:
    client = boot()
    provider = "codex" if args.model_provider == "codex" else "dsh"
    if provider == "codex":
        assert args.codex_auth_source, "--codex-auth-source is required for Codex"
        auth = out / "data/credentials/codex/auth.json"
        auth.parent.mkdir(parents=True, exist_ok=True)
        shutil.copy2(args.codex_auth_source.resolve(), auth)
        auth.chmod(0o600)
    else:
        if args.api_key_file:
            secret = args.api_key_file.read_text().strip()
            baseline = client.rpc("credentials.status", {"model_provider":args.model_provider})
            client.rpc("credentials.configure", {"command_id":str(uuid.uuid4()), "expected_revision":baseline["credential_version"]["revision"], "provider_ref":baseline["provider_ref"], "provider":"dsh", "model_provider":args.model_provider, "api_key":secret})
            del secret
            args.api_key_file.unlink()
        else:
            assert args.reuse_dataset, "--api-key-file is required for MiniMax"
        client.rpc("credentials.check", {"provider":"dsh", "model_provider":args.model_provider})
    client.rpc("runtime.prepare", {"provider": provider})
    defaults = client.rpc("query.configuration_defaults")
    agent = defaults["role_agent_bindings"]["builtin-role-analyst:1"] if provider == "codex" else next(row["id"] for row in client.rpc("query.configurations", {"kind":"agent", "limit":100})["records"] if row["definition_id"] == "builtin-agent-minimax-analyst")
    instance = client.command("agent.instance.create", payload={"name": "原生经验验收分析师", "agent_revision_id": agent, "scope": "global"})["id"]
    opened = client.rpc("session.open", {"scope": "global", "command_id": str(uuid.uuid4()), "target": "global", "expected_revision": 0, "payload": {"agent_instance_id": instance}})
    detached, first = talk(client, opened["id"], "当前没有项目、工作流或材料。请实际调用 devdesk_agent_experience_list，参数为空对象；收到回执后只回复 FREE_CONTEXT_READY。不要创建任何工程对象。", True)
    assert client.rpc("query.workspace")["active_project"] is None
    assert any(row["method"] == "agent.experience.list" and row["status"] == "succeeded" for row in first["experience_reads"])
    print("PASS native discussion without a project", flush=True)

    project = client.command("project.create", payload={"title": "原生经验来源项目"})["id"]
    local = Client(client.url, project=project)
    owner = local.rpc("query.workspace")["active_project"]
    opened = local.command("session.open", project, owner["revision"], {"agent_instance_id": instance})
    marker = "PROJECT_HISTORY_" + uuid.uuid4().hex[:12]
    _, source = talk(local, opened["id"], f"仅记录这个讨论中的验收记号 {marker}，不要创建或修改工程对象，只原样回复该记号。", False)
    (out / "source-session.json").write_text(json.dumps(source, ensure_ascii=False, indent=2))
    detached, recalled = talk(client, detached, f"请实际调用 devdesk_agent_experience_list，以 project_id={project} 筛选我的来源项目经验，再调用 devdesk_agent_experience_read 阅读返回的快照。按 next_offset 继续读取到结束，找到我在那次讨论中记录的 PROJECT_HISTORY_ 开头记号并报告。不要切换项目、启动工作流、恢复会话或修改工程对象。", True)
    reads = [row for row in recalled["experience_reads"] if row["method"] == "agent.experience.read" and row["status"] == "succeeded" and row.get("source", {}).get("project_id") == project]
    assert reads, "The Agent must actually read the source experience."
    assert any(marker in event["content"] for event in recalled["events"] if event["kind"] == "assistant")
    assert client.rpc("query.workspace")["active_project"]["id"] == project
    (out / "detached-session.json").write_text(json.dumps(recalled, ensure_ascii=False, indent=2))
    print("PASS native list/read of project experience in the original detached session", flush=True)
    stop()
    client = boot()
    restored = client.rpc("query.session", {"scope": "global", "id": detached})
    assert restored["agent_instance_id"] == instance
    assert restored["session"]["runtime_binding"]["agent_configuration"]["context_protocol"] == "devdesk-agent-context/v1"
    assert restored["experience_reads"] == recalled["experience_reads"]
    report = {"provider": provider, "model_provider":args.model_provider, "inference": "real", "agent": agent, "instance": instance, "project": project, "detached_session": detached, "history_reads": len(reads), "no_project_discussion": True, "original_session_preserved": True, "active_project_preserved": True, "restart_preserves_context": True, "binary_sha256": hashlib.sha256(binary.read_bytes()).hexdigest()}
    (out / "report.json").write_text(json.dumps(report, ensure_ascii=False, indent=2))
    print(json.dumps(report, ensure_ascii=False), flush=True)
finally:
    stop()
    shutil.rmtree(out / "data/credentials", ignore_errors=True)

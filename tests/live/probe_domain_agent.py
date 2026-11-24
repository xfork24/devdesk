#!/usr/bin/env python3
"""Verify an entity-owned workflow with a real Codex controller and Human approval."""
import argparse
import base64
import hashlib
import json
import os
from pathlib import Path
import shutil
import subprocess
import sys
import tempfile
import time
import uuid

REPO = Path(__file__).resolve().parents[2]
sys.path.insert(0, str(REPO / "tests" / "entrypoints"))
from domain_client import Client, lookup, prepare_document, start_for, submitted_proof, wait_view


def wait_for_host(host, log):
    deadline = time.monotonic() + 15
    while time.monotonic() < deadline:
        if host.poll() is not None:
            raise RuntimeError("Core Host exited during startup")
        for line in log.read_text().splitlines():
            if line.startswith("http://127.0.0.1:"):
                return Client(line)
        time.sleep(0.1)
    raise TimeoutError("Core Host startup")


def verify_discussion(client, epoch, entity, agent_instance):
    pid, owner = entity["project_id"], entity["id"]
    where = {"domain_epoch": epoch, "project_id": pid}

    def command(method, target, expected, **payload):
        return client.call(method, **where, target=target, expected_revision=expected,
                           command_id=uuid.uuid4().hex, payload=payload)

    sibling = client.call("domain.entity.put", epoch=epoch, project_id=pid, entity_id=str(uuid.uuid4()),
                          type_ref=entity["type_ref"], expected_revision=0, data={"title": "Other document"},
                          command_id=uuid.uuid4().hex)
    discussion = command("session.open", owner, entity["revision"], agent_instance_id=agent_instance)
    reused = command("session.open", owner, entity["revision"], agent_instance_id=agent_instance)
    assert reused["id"] == discussion["id"] and reused["reused"]
    view = client.call("query.session", **where, id=discussion["id"], owner_id=owner)
    assert view["session"]["work_owner"] == {"kind": "entity", "id": owner, "project": pid}
    assert any(row["id"] == discussion["id"] for row in client.call("query.sessions", **where, owner_id=owner)["sessions"])
    for other in (pid, sibling["id"]):
        assert all(row["id"] != discussion["id"] for row in client.call("query.sessions", **where, owner_id=other)["sessions"])
    try:
        command("session.open", sibling["id"], view["session"]["revision"], agent_instance_id=agent_instance,
                continue_from={"session_id": discussion["id"]})
    except RuntimeError as error:
        assert "续聊需要原会话的归属" in str(error), error
    else:
        raise AssertionError("continued from a sibling entity discussion")

    turn = command("conversation.send", discussion["id"], view["session"]["revision"],
                   client_message_id=uuid.uuid4().hex,
                   input="Reply with the word manuscript. This is a free discussion. Do not call tools.")
    deadline = time.monotonic() + 120
    while time.monotonic() < deadline:
        observed = client.call("query.session", **where, id=discussion["id"], owner_id=owner)
        statuses = {row["id"]: row["status"] for row in observed["turns"]}
        status = statuses.get(turn["id"])
        assert status not in {"failed", "cancelled", "needs_reconciliation"}, observed
        if status == "completed":
            entries = client.call("query.agent_experience", **where, agent_instance_id=agent_instance,
                                  entity_id=owner)["records"]
            detail = next((entry for entry in entries if entry["session_id"] == discussion["id"]), None)
            if detail:
                assert detail["work_owner"] == view["session"]["work_owner"]
                assert client.call("query.agent_experience", **where, agent_instance_id=agent_instance,
                                   entity_id=sibling["id"])["records"] == []
                try:
                    client.call("query.agent_experience", **where, agent_instance_id=agent_instance,
                                entity_id=sibling["id"], snapshot_id=detail["snapshot_id"], source_project_id=pid)
                except RuntimeError as error:
                    assert "not_found" in str(error), error
                else:
                    raise AssertionError("read an experience in a sibling scope")
                return {"session_id": discussion["id"], "turn_id": turn["id"], "experience": detail["snapshot_id"]}
        time.sleep(0.5)
    raise TimeoutError("Entity discussion or experience was not completed")


def verify_resource_workspace(client, epoch, entity, dataset):
    pid = entity["project_id"]
    run = client.call("domain.workflow.start", epoch=epoch, project_id=pid, entity_id=entity["id"],
                      expected_revision=entity["revision"], command_id=uuid.uuid4().hex, mode="rust",
                      workflow_ref="manuscript.resource-workflow:1")
    result = wait_view(client, epoch, run, lambda view: view["record"]["data"]["status"] == "completed",
                       timeout=150)
    proof = client.call("query.domain.entity", epoch=epoch, project_id=pid,
                        entity_id=result["domain_submission"]["entities"]["proof"])
    assert proof["data"] == {"title": "Local manuscript", "observedBy": "native-resource-reader"}
    assert proof["origin"]["producer"]["kind"] == "agent" and proof["relations"] == {"manuscript": [entity["id"]]}
    seal, read = result["runtime"]["steps"]
    original = result["outputs"][seal["outputs"]["sealed"]]
    source_file = dataset / "projects" / pid / "execution/sessions" / read["session_id"] / "work/artifact"
    assert json.loads(source_file.read_bytes()) == {"kind": "artifact", "title": "Local manuscript"}
    with client.request(f"/artifacts/{original['artifact_id']}?project_id={pid}&domain_epoch={epoch}") as archive:
        assert source_file.read_bytes() == archive.read()
    discussion = client.call("query.session", domain_epoch=epoch, project_id=pid, id=read["session_id"])
    command = next((item for event in discussion["events"] if event["kind"] == "tool"
                    and event["content"].startswith("{")
                    for item in [json.loads(event["content"])]
                    if item.get("type") == "commandExecution" and item.get("status") == "completed"
                    and item.get("exitCode") == 0 and "/work/artifact" in item["command"]
                    and item.get("aggregatedOutput", "").encode() == source_file.read_bytes()), None)
    assert command, "No native command read the exact mounted material"
    return {"workflow_execution_id": run["execution_id"], "session_id": read["session_id"],
            "source_artifact_id": original["artifact_id"], "proof_id": proof["id"],
            "read_command_id": command["id"], "material_sha256": hashlib.sha256(source_file.read_bytes()).hexdigest()}


def verify(client, archive, digest, report, dataset):
    client.call("runtime.prepare_harness", id="builtin-harness-codex-managed:2")
    installed = client.call("domain.install", archive=base64.b64encode(archive).decode())
    assert installed["digest"] == digest
    before = client.call("query.domain.active")
    epoch = client.call("domain.activate", epoch=before["epoch"], digest=digest)["epoch"]
    entity = prepare_document(client, epoch)
    run = start_for(client, epoch, entity, "agent")
    observed_calls = {}

    def await_progress(check, timeout=300):
        deadline = time.monotonic() + timeout
        last = None
        while time.monotonic() < deadline:
            view = lookup(client, epoch, run)
            controller = client.call("query.orchestration", domain_epoch=epoch,
                                     project_id=run["project_id"], root_instance_id=run["execution_id"])
            manager = controller.get("record", {}).get("data", {})
            for call in controller.get("calls", []):
                observed_calls[call["id"]] = call
            state = {"run": run, "status": view["record"]["data"]["status"],
                     "steps": [step["status"] for step in view["runtime"]["steps"]],
                     "controller": manager.get("status"), "session_id": manager.get("session_id"),
                     "calls": list(observed_calls.values())}
            if state != last:
                print(json.dumps(state, ensure_ascii=False), flush=True)
                last = state
            (report / "progress.json").write_text(json.dumps(state, ensure_ascii=False, indent=2))
            if manager.get("status") in {"failed", "needs_reconciliation", "budget_exhausted"}:
                raise RuntimeError((manager.get("status"), manager.get("reason")))
            result = check(view, controller)
            if result is not None:
                return result
            time.sleep(0.5)
        raise TimeoutError("Codex did not reach the next workflow condition")

    pending = await_progress(lambda view, controller: next((
        decision for decision in controller.get("decisions", [])
        if decision["data"]["status"] == "pending"
        and decision["data"]["request"]["operation"] == "workflow.step.confirm"
        and decision["data"]["actor"]["kind"] == "controller"
        and view["runtime"]["steps"][0]["status"] == "waiting_confirmation"
    ), None))
    arguments = pending["data"]["request"]["arguments"]
    step = lookup(client, epoch, run)["runtime"]["steps"][0]
    assert arguments["execution_id"] == step["id"]
    assert arguments["input_digest"] == step["input_digest"] and arguments["outputs"] == step["outputs"]
    decision = client.call("workflow.decision.resolve", domain_epoch=epoch, project_id=run["project_id"],
                           command_id="live-editorial-human-confirmation", target=pending["id"],
                           expected_revision=pending["revision"],
                           payload={"approved": True, "reason": "I verified the exact submitted proof."})

    controller = await_progress(lambda view, manager: manager if
                                view["record"]["data"]["status"] == "completed" else None)
    proof = submitted_proof(client, epoch, run)
    calls = [call["method"] for call in observed_calls.values() if call["status"] == "succeeded"]
    assert {"workflow.step.dispatch", "workflow.decision.request", "workflow.step.accept", "workflow.complete"} <= set(calls)
    session_id = controller["record"]["data"]["session_id"]
    session = client.call("query.session", domain_epoch=epoch, project_id=run["project_id"], id=session_id)
    assert session["session"]["external_session"], "Codex did not create a native thread"
    assert session["current_runtime_binding"]["model"] and session["executions"], "native runtime evidence missing"
    assert session["session"]["work_owner"] == {"kind": "entity", "id": entity["id"], "project": entity["project_id"]}
    discussion = verify_discussion(client, epoch, entity, session["agent_instance_id"])
    workspace = verify_resource_workspace(client, epoch, entity, dataset)
    return {"run": run, "workspace": workspace, "discussion": discussion, "proof": proof, "human_decision": decision, "session_id": session_id,
            "successful_native_calls": calls}


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--binary", required=True, type=Path)
    parser.add_argument("--codex-auth-source", required=True, type=Path)
    parser.add_argument("--report-directory", required=True, type=Path)
    args = parser.parse_args()
    args.report_directory.mkdir(parents=True, exist_ok=False)
    with tempfile.TemporaryDirectory(prefix="devdesk-domain-agent-") as temporary:
        root = Path(temporary)
        subprocess.run([sys.executable, str(REPO / "tests/fixtures/domain-packages/build.py"), temporary],
                       check=True)
        archive = (root / "manuscript.zip").read_bytes()
        digest = hashlib.sha256(archive).hexdigest()
        dataset = root / "data"
        env = {key: value for key, value in os.environ.items() if key != "DEVDESK_ACCESS_TOKEN"}
        log = root / "host.log"
        with log.open("w") as output:
            host = subprocess.Popen([str(args.binary.resolve()), "--data", str(dataset), "--port", "0"],
                                    env=env, stdout=output, stderr=subprocess.STDOUT)
            try:
                client = wait_for_host(host, log)
                auth = dataset / "credentials/codex/auth.json"
                auth.parent.mkdir(parents=True, exist_ok=True)
                auth.touch(mode=0o600, exist_ok=False)
                shutil.copyfile(args.codex_auth_source.resolve(), auth)
                result = verify(client, archive, digest, args.report_directory, dataset)
                (args.report_directory / "result.json").write_text(json.dumps(result, ensure_ascii=False, indent=2))
            finally:
                host.terminate()
                try:
                    host.wait(timeout=25)
                except subprocess.TimeoutExpired:
                    host.kill()
                    host.wait()
                # Keep only scheduler diagnostics, outside the ephemeral credential dataset.
                (args.report_directory / "scheduler.log").write_text("\n".join(
                    line for line in log.read_text().splitlines() if '"type":"scheduler_error"' in line
                ))


if __name__ == "__main__":
    main()

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

REPO = Path(__file__).resolve().parents[2]
sys.path.insert(0, str(REPO / "tests" / "entrypoints"))
from domain_client import Client, lookup, prepare_document, start_for, submitted_proof


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


def verify(client, archive, digest, report):
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
    return {"run": run, "proof": proof, "human_decision": decision, "session_id": session_id,
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
            host = subprocess.Popen([str(args.binary.resolve()), "--data", str(dataset), "--port", "0",
                                     "--trust-domain", digest], env=env, stdout=output, stderr=subprocess.STDOUT)
            try:
                client = wait_for_host(host, log)
                auth = dataset / "credentials/codex/auth.json"
                auth.parent.mkdir(parents=True, exist_ok=True)
                auth.touch(mode=0o600, exist_ok=False)
                shutil.copyfile(args.codex_auth_source.resolve(), auth)
                result = verify(client, archive, digest, args.report_directory)
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

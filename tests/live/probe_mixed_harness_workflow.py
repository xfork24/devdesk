#!/usr/bin/env python3
"""Exercise native Codex agent scheduling against five registered Harnesses."""

import argparse
import base64
from contextlib import closing
import json
from pathlib import Path
import shutil
import signal
import sqlite3
import subprocess
import sys
import time
from uuid import uuid4

from client import Client

sys.path.insert(0, str(Path(__file__).resolve().parents[2] / "plugins/software-development"))
from build import REPO, VERSION, build, source
from mixed_harness import BINDINGS, FEATURE, RELEASE_REVIEWER, STANDARD, ZCODE_REVIEWER, mixed_role, role


PROBE_WORKFLOW = "software-development.mixed-harness-scheduling-probe:1"
PROBE_STANDARD = "software-development.mixed-harness-scheduling-probe-standard:1"
HARNESSES = {
    mixed_role("analyst"): "pi",
    mixed_role("architect"): "opencode",
    mixed_role("developer"): "mcode",
    mixed_role("tester"): "dsh",
    RELEASE_REVIEWER: "zcode",
}
TERMINAL_FAILURES = {"failed", "cancelled", "needs_reconciliation", "budget_exhausted"}
MODEL = "MiniMax-M3.1-Flash"


def write_json(directory, name, value):
    path = directory / f"{name}.json"
    temporary = path.with_suffix(".tmp")
    temporary.write_text(json.dumps(value, ensure_ascii=False, indent=2) + "\n")
    temporary.replace(path)


def successful_methods(manager):
    calls = (row.get("data", row) for row in manager["calls"])
    return {call["method"] for call in calls if call["status"] == "succeeded"}


def completion_receipt(directory, project, root, manager):
    database = directory / "data" / "projects" / project / "engineering.sqlite3"
    with closing(sqlite3.connect(f"{database.as_uri()}?mode=ro", uri=True)) as connection:
        receipt = connection.execute(
            """SELECT completion.id, decision.data FROM governance_records AS completion
            JOIN governance_records AS decision ON decision.id = json_extract(completion.data, '$.decision_id')
            WHERE completion.type = 'workflow_completion' AND completion.definition_id = ?""", (root,),
        ).fetchone()
        assert receipt, root
        decision = json.loads(receipt[1])
        assert decision["operation"] == "complete" and decision["root_instance_id"] == root, decision
        actor = decision["actor"]
        if manager and actor["kind"] == "segment":
            segment = connection.execute(
                "SELECT data FROM governance_records WHERE type = 'workflow_segment' AND id = ? AND definition_id = ?",
                (actor["authorization_id"], root),
            ).fetchone()
            assert segment, actor
            delegation = json.loads(segment[0])
            assert delegation["complete_instance"] and delegation["actor"]["controller_id"] == manager["record"]["id"]
        elif manager:
            assert actor["kind"] == "controller" and actor["controller_id"] == manager["record"]["id"], actor
    return {"id": receipt[0], "actor": actor, "operation": "workflow.complete"}


def zcode_read_receipt(directory, project, turn, output):
    database = directory / "data" / "projects" / project / "engineering.sqlite3"
    with closing(sqlite3.connect(f"{database.as_uri()}?mode=ro", uri=True)) as connection:
        calls = connection.execute(
            """SELECT id, data FROM governance_records
            WHERE type = 'platform_call' AND json_extract(data, '$.handler') = 'workflow.outputs.read'
              AND json_extract(data, '$.request.caller.turn_id') = ?""", (turn,),
        ).fetchall()
        for identity, raw in calls:
            call = json.loads(raw)
            request = call["request"]
            if (call["status"] == "succeeded" and request["caller"]["request_id"]["kind"] == "bridge"
                    and request["arguments"]["ids"] == [output] and request["arguments"]["pointer"] == "/value"):
                return {"id": identity, "turn": turn, "output": output, "digest": call["receipt_digest"]}
    raise AssertionError("No successful exact output read through the ZCode Host bridge")


def prepare_credentials(client, key, refs):
    for harness in sorted(set(HARNESSES.values())):
        provider = "minimax-cn" if harness == "dsh" else f"{harness}-minimax-cn"
        params = {"provider": harness, "model_provider": provider}
        current = client.rpc("credentials.status", params)
        if current["configured"] and current["available"]:
            print(f"PASS {harness} MiniMax credential available", flush=True)
            continue
        result = client.rpc("credentials.configure", {
            **params, "command_id": str(uuid4()), "expected_revision": current["credential_version"]["revision"],
            "provider_ref": current["provider_ref"], "api_key": key,
        })
        status = client.rpc("credentials.status", params)
        reference = status.get("reference")
        if reference:
            refs.add(reference)
        assert status["configured"] and status["available"], (harness, result)
        print(f"PASS {harness} MiniMax credential bound", flush=True)


def probe_definitions():
    spec = source("spec", "mixed-harness-probe-spec", {
        "schema_version": 3, "body": "Increase the supplied integer by one and return the new integer.",
        "checks": [{"key": "value", "name": "Exact handoff", "instructions": "Output value must equal input plus one."}],
    })
    output = source("delivery_type", "mixed-harness-probe-output", {
        "format": "json", "content_schema": {
            "type": "object", "required": ["value"], "properties": {"value": {"type": "integer"}},
            "additionalProperties": False,
        },
    }, spec_refs=[f"{spec['identity']}:1"])
    stages = ["requirements", "design", "implementation", "tests", "review"]
    steps = []
    for key, role_ref in zip(stages, HARNESSES, strict=True):
        prior = steps[-1]["key"] if steps else None
        inputs = {"type": "source", "source": {"step": prior, "port": "result", "pointer": "/value"}} if prior else {
            "type": "constant", "value": 10,
        }
        steps.append({
            "key": key, "name": key.title(), "stage": key,
            "executor": {"type": "agent", "role_ref": role_ref}, "dependencies": [prior] if prior else [],
            "task_instructions": (
                'Add one to inputs.number. Return exactly {"status":"completed","summary":"handoff","outputs":'
                '{"result":{"value":NUMBER}}}, replacing NUMBER with the calculated integer. '
                "The entire final message is the object. No code fence, headings, narrative, file edits or additional review."
            ),
            "inputs": {"number": inputs}, "outputs": {"result": {"delivery_ref": f"{output['identity']}:1"}},
            "harness_permission_ref": "builtin-permission-harness-read-only:2",
            "budget": {"max_turns": 3, "timeout_seconds": 240, "retry_limit": 1},
        })
        if key == "review":
            steps[-1]["task_instructions"] += (
                " Before returning the envelope, call workflow.outputs.read with ids containing "
                "input_sources.number.source_output_id and pointer /value. Use the returned number."
            )
    document = {
        "schema_version": 6, "orchestrator_role_ref": role("project-manager"),
        "stages": [{"key": key, "name": key.title()} for key in stages], "steps": steps,
        "outputs": {"result": {"step": "review", "port": "result"}},
    }
    workflow = source("workflow", "mixed-harness-scheduling-probe", document)
    standard = source("standard", "mixed-harness-scheduling-probe-standard", {"schema_version": 4},
                      root_workflow_refs=[PROBE_WORKFLOW], default_workflow_ref=PROBE_WORKFLOW)
    return [spec, output, workflow, standard], stages


def run_probe(client, directory, stages, configuration):
    project = client.rpc("query.workspace")["active_project"]
    selected = configuration["data"]
    mode = selected["orchestration"]["mode"]
    root = client.command("workflow.start", client.project, project["revision"], {
        key: selected[key] for key in ("workflow_ref", "standard_ref", "role_agent_bindings",
                                       "harness_permission_ref", "sandbox_ref", "sandbox_permission_ref", "orchestration")
    })["id"]
    last = None

    def observe():
        view = client.rpc("query.workflow_instance", {"id": root})
        write_json(directory, "workflow", view)
        manager = None
        if mode == "agent":
            manager = client.rpc("query.orchestration", {"root_instance_id": root})
            write_json(directory, "manager", manager)
            state = (manager.get("record") or {}).get("data")
            if state and state["status"] in TERMINAL_FAILURES:
                raise RuntimeError(f"Manager status {state['status']} (see manager.json)")
        return view, manager

    def settled(result):
        nonlocal last
        if result is None:
            return False
        view, manager = result
        steps = [(step["step"], step["status"]) for step in view["runtime"]["steps"]]
        if steps != last:
            print(steps, flush=True)
            last = steps
        if any(status in TERMINAL_FAILURES for _, status in steps):
            raise RuntimeError("A worker failed (see workflow.json and its session)")
        return view["runtime"]["status"] == "completed"

    view, manager = client.wait(observe, settled, timeout=1200)
    verify_probe(client, directory, stages, mode, view, manager)


def verify_probe(client, directory, stages, mode, view, manager):
    methods = set()
    manager_model = None
    if manager is not None:
        methods = successful_methods(manager)
        assert methods & {"workflow.step.dispatch", "workflow.segment.dispatch"}, methods
        assert methods.isdisjoint({"workflow.change.apply", "workflow.execution.reconcile"}), methods
        binding = client.rpc("query.session", {"id": manager["record"]["data"]["session_id"]})["session"]["runtime_binding"]
        assert binding["installation"]["key"]["provider"] == "codex", binding
        manager_model = binding["model"]
        assert manager_model == "gpt-6.2-sol", manager_model
    evidence = []
    steps = {step["step"]: step for step in view["runtime"]["steps"]}
    for expected, (step_key, harness) in enumerate(zip(stages, HARNESSES.values(), strict=True), 11):
        step = steps[step_key]
        assert step["status"] == "completed"
        source = view["outputs"][step["outputs"]["result"]]["value"]
        assert source == {"value": expected}, (step_key, source)
        session = client.rpc("query.session", {"id": step["session_id"]})
        binding = session["session"]["runtime_binding"]
        assert binding["installation"]["key"]["provider"] == harness, (step_key, harness)
        assert binding["model"] == MODEL, (step_key, binding["model"])
        assert any(turn["id"] == step["turn_id"] and turn["consumed"] and turn["status"] == "completed"
                   for turn in session["turns"]), step_key
        evidence.append({"step": step_key, "harness": harness, "model": MODEL, "turn": step["turn_id"],
                         "value": source["value"]})
    write_json(directory, "result", {
        "root": view["record"]["id"], "manager": manager["record"]["id"] if manager else None,
        "manager_model": manager_model, "mode": mode, "native_methods": sorted(methods),
        "completion_receipt": completion_receipt(directory, client.project, view["record"]["id"], manager),
        "zcode_host_read": zcode_read_receipt(directory, client.project, steps["review"]["turn_id"],
                                             steps["tests"]["outputs"]["result"]),
        "steps": evidence,
    })
    print(f"PASS {mode} scheduling and five real MiniMax step turns", flush=True)


def observe_feature_entry(client, directory, configuration):
    feature = client.command("feature.create", payload={
        "project_id": client.project, "title": "Normalize whitespace without losing case",
        "initial_requirement": (
            "Add a public Python function normalize_whitespace(value: str) that trims leading and "
            "trailing whitespace without altering internal spaces or letter case. Empty input must remain empty. "
            "The function accepts Python str only; behavior for other input is out of scope. "
            "Document these cases for an isolated acceptance probe."
        ),
        "start_workflow": False, "configuration_revision_id": configuration["id"],
    })["id"]
    root = client.command("workflow.start", feature, client.feature(feature)["feature"]["revision"])["id"]

    def observe():
        view = client.rpc("query.workflow_instance", {"id": root})
        manager = client.rpc("query.orchestration", {"root_instance_id": root})
        write_json(directory, "engineering-workflow", view)
        write_json(directory, "engineering-manager", manager)
        state = (manager.get("record") or {}).get("data")
        if state and state["status"] in TERMINAL_FAILURES:
            raise RuntimeError(f"Feature manager status {state['status']} (see engineering-manager.json)")
        steps = {step["step"]: step for step in view["runtime"]["steps"]}
        if any(steps[key]["status"] in TERMINAL_FAILURES for key in ("requirements", "requirements_review")):
            raise RuntimeError("Feature entry failed (see engineering-workflow.json)")
        if steps["requirements_review"]["status"] != "waiting_confirmation":
            return None
        for key, harness in [("requirements", "pi"), ("requirements_review", "dsh")]:
            session = client.rpc("query.session", {"id": steps[key]["session_id"]})
            binding = session["session"]["runtime_binding"]
            assert binding["installation"]["key"]["provider"] == harness
            assert binding["model"] == MODEL
        assert steps["requirements"]["status"] == "completed"
        assert steps["source_workspace"]["status"] == "queued"
        review = steps["requirements_review"]
        review_output = review["outputs"]["result"]
        assert view["outputs"][review_output]["value"]["approve"] is True
        assert view["completion_status"][review["id"]]["human_satisfied"] is False
        methods = successful_methods(manager)
        assert methods & {"workflow.step.dispatch", "workflow.segment.dispatch"}, methods
        return {"root": root, "feature": feature, "status": "waiting_confirmation", "gate": "requirements_review",
                "review_output": review_output, "human_decisions_by_probe": 0}

    result = client.wait(observe, bool, timeout=900)
    write_json(directory, "engineering-result", result)
    print("PASS agent dispatch on the released Feature to the first human gate", flush=True)


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--binary", type=Path, required=True)
    parser.add_argument("--domain-zip", type=Path, required=True)
    parser.add_argument("--web-dir", type=Path, default=REPO / "target/web/software")
    parser.add_argument("--codex-auth-source", type=Path, required=True)
    parser.add_argument("--reports", type=Path, required=True)
    mode = parser.add_mutually_exclusive_group()
    mode.add_argument("--feature-entry", action="store_true",
                      help="observe the released Feature to its first human gate")
    mode.add_argument("--worker-only", action="store_true",
                      help="use Rust scheduling on the isolated worker probe")
    args = parser.parse_args()
    key = sys.stdin.readline().strip()
    if not key:
        parser.error("supply the MiniMax API key on stdin")
    directory = args.reports.resolve()
    directory.mkdir(parents=True, exist_ok=False, mode=0o700)
    data = directory / "data"
    refs = set()
    with (directory / "host.log").open("w") as log:
        host = subprocess.Popen([str(args.binary.resolve()), "--data", str(data), "--port", "0"], stdout=log, stderr=log)
    try:
        deadline = time.monotonic() + 30
        while time.monotonic() < deadline and host.poll() is None:
            urls = [line for line in (directory / "host.log").read_text().splitlines() if line.startswith("http://127.0.0.1:")]
            if urls:
                break
            time.sleep(.2)
        else:
            raise RuntimeError("Host did not start (see host.log)")
        client = Client(urls[0], timeout=300)
        auth = data / "credentials/codex/auth.json"
        auth.parent.mkdir(parents=True, exist_ok=True)
        shutil.copyfile(args.codex_auth_source, auth)
        auth.chmod(0o600)
        prepare_credentials(client, key, refs)
        del key
        for harness in ["codex", *sorted(set(HARNESSES.values()))]:
            client.rpc("runtime.prepare", {"provider": harness})
            print(f"PASS {harness} runtime prepared", flush=True)
        def configure_project(archive, workflow, standard, title, mode="agent"):
            client.project = None
            active = client.rpc("query.domain.active")
            installed = client.rpc("domain.install", {"archive": base64.b64encode(archive.read_bytes()).decode()})
            active = client.rpc("domain.activate", {"epoch": active["epoch"], "digest": installed["digest"]})
            client.domain_epoch = active["epoch"]
            client.project = client.rpc("domain.project.create", {"epoch": active["epoch"], "command_id": str(uuid4()),
                                                                   "title": title})["id"]
            project = client.rpc("query.workspace")["active_project"]
            return client.command("project.configuration.update", client.project, project["revision"], {
                "role_agent_bindings": bindings, "orchestration": {"mode": mode},
                "harness_permission_ref": active["defaults"]["harness_permission_ref"],
                "workflow_ref": workflow, "standard_ref": standard,
            })

        bindings = {**BINDINGS, RELEASE_REVIEWER: ZCODE_REVIEWER}
        production = configure_project(args.domain_zip, FEATURE, STANDARD, "Cross-Harness Engineering")
        write_json(directory, "engineering-configuration", production)
        print("PASS cross-Harness Feature and Task Standard selected and validated", flush=True)
        if args.feature_entry:
            observe_feature_entry(client, directory, production)
            return
        extra_sources, stages = probe_definitions()
        probe_archive = directory / "scheduling-probe.zip"
        build(probe_archive, args.web_dir, extra_sources=extra_sources, version=f"{VERSION}-scheduling-probe")
        probe = configure_project(probe_archive, PROBE_WORKFLOW, PROBE_STANDARD, "Isolated cross-Harness scheduling",
                                  mode="rust" if args.worker_only else "agent")
        run_probe(client, directory, stages, probe)
    finally:
        try:
            if host.poll() is None:
                host.send_signal(signal.SIGINT)
                host.wait(timeout=60)
        finally:
            for auth_path in data.rglob("auth.json"):
                auth_path.unlink()
            for reference in refs:
                service, account = reference.split(":", 1)
                if sys.platform == "darwin":
                    removal = ["security", "delete-generic-password", "-s", service, "-a", account]
                elif sys.platform.startswith("linux"):
                    removal = ["secret-tool", "clear", "service", service, "account", account]
                else:
                    continue
                if subprocess.run(removal, stdout=subprocess.DEVNULL, stderr=subprocess.DEVNULL).returncode:
                    print("The isolated probe could not remove a test credential.", file=sys.stderr)


if __name__ == "__main__":
    main()

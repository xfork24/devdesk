#!/usr/bin/env python3
"""Product Host and packaged Harnesses with a deterministic local model."""
import argparse
import base64
import json
from http.server import BaseHTTPRequestHandler, ThreadingHTTPServer
from pathlib import Path
import signal
import sqlite3
import subprocess
import sys
import threading
import time
import uuid

sys.path.insert(0, str(Path(__file__).resolve().parents[1] / "live"))
from client import Client


MODEL = "devdesk-pi-fixture"
ROLE_MARKER = "FROZEN_PI_ROLE_1"
EXPERIENCE = "devdesk_agent_experience_list"
WRITE = "pi-policy-write-marker.txt"
CAPTURED = "batch-b-captured-material"


class Model(BaseHTTPRequestHandler):
    def log_message(self, *_):
        pass

    def do_GET(self):
        if self.path != "/canary":
            self.send_error(404)
            return
        with self.server.audit_lock:
            self.server.external_calls += 1
        body = b'{"marker":"PI_EXTERNAL_FIXTURE_OK"}'
        self.send_response(200)
        self.send_header("Content-Type", "application/json")
        self.send_header("Content-Length", str(len(body)))
        self.end_headers()
        self.wfile.write(body)

    def do_POST(self):
        if self.path != "/v1/chat/completions":
            self.send_error(404)
            return
        body = json.loads(self.rfile.read(int(self.headers["Content-Length"])))
        messages = body["messages"]
        index = max(i for i, message in enumerate(messages) if message["role"] == "user")
        user = json.dumps(messages[index])
        results = [row for row in messages[index + 1:] if row["role"] == "tool"]
        names = [row["function"]["name"] for row in body.get("tools", [])]
        scenario = next((key for key in ("PI_CONTEXT", "PI_READ_ONLY", "PI_PRIVATE_PATH", "PI_CANCEL", "PI_RESUME", "PI_STEP", "PI_CONFIRM", "PI_EXTERNAL") if key in user), None)
        compaction = self.server.harness_provider == "opencode" and "<template>" in user
        with self.server.audit_lock:
            self.server.audit.append({
                "scenario": scenario,
                "frozen_role": any(ROLE_MARKER in json.dumps(row) for row in messages if row["role"] == "system"),
                "model": body["model"],
                "tools": names,
                "results": results,
                "has_prior_context": any("PI_CONTEXT" in json.dumps(row) for row in messages[:index]),
                "has_captured_material": CAPTURED in user,
                "compaction": compaction,
            })
        self.send_response(200)
        self.send_header("Content-Type", "text/event-stream")
        self.end_headers()
        identifier = "chatcmpl-" + uuid.uuid4().hex

        def chunk(delta, finished=None):
            row = {"id": identifier, "object": "chat.completion.chunk", "created": 1,
                   "model": MODEL, "choices": [{"index": 0, "delta": delta, "finish_reason": finished}]}
            self.wfile.write(b"data: " + json.dumps(row).encode() + b"\n\n")
            self.wfile.flush()

        try:
            chunk({"role": "assistant"})
            if compaction:
                chunk({"content": "## Objective\n- Verify the managed Harness.\n\n"
                                  "## Important Context\n- The initial discussion completed with its context-tool receipt."})
                chunk({}, "stop")
                self.wfile.write(b"data: [DONE]\n\n")
                return
            if scenario == "PI_CANCEL":
                chunk({"content": "PI_CANCEL_IN_PROGRESS"})
                self.server.release_cancel.wait(timeout=30)
            requested = {
                "PI_CONTEXT": (EXPERIENCE, {}),
                "PI_EXTERNAL": ("pi_fixture_external", {}),
                "PI_READ_ONLY": ("write", {"path": WRITE, "content": "unexpected write"}),
                "PI_PRIVATE_PATH": ("read", {"path": "file:///run/devdesk/profile"}),
                "PI_STEP": ("devdesk_materials_capture", {"note": "fixture"}),
                "PI_CONFIRM": ("devdesk_materials_capture", {"note": CAPTURED if CAPTURED in user else "missing"}),
            }
            if not results and scenario in requested and names:
                tool, arguments = requested[scenario]
                if self.server.harness_provider != "pi" and scenario == "PI_READ_ONLY":
                    tool = next((name for name in names if name.lower() in ("bash", "shell")), tool)
                    arguments = {"command": f"printf rejected > /work/{WRITE}", "description": "Policy canary"}
                if self.server.harness_provider != "pi" and scenario == "PI_PRIVATE_PATH":
                    tool = next((name for name in names if name.lower() == "read"), tool)
                    arguments = {"path": "/run/devdesk/permission"}
                if tool not in names and scenario in ("PI_CONTEXT", "PI_EXTERNAL", "PI_STEP", "PI_CONFIRM"):
                    tool = next((name for name in names if name.endswith("_" + tool)), tool)
                chunk({"tool_calls": [{"index": 0, "id": "call_" + uuid.uuid4().hex,
                                       "type": "function", "function": {"name": tool, "arguments": json.dumps(arguments)}}]})
                chunk({}, "tool_calls")
            elif scenario in ("PI_STEP", "PI_CONFIRM"):
                note = CAPTURED if scenario == "PI_STEP" and self.server.harness_provider != "pi" else "fixture"
                chunk({"content": json.dumps({"status": "completed", "summary": "Harness step fixture",
                                             "outputs": {"result": {"note": note}}})})
                chunk({}, "stop")
            else:
                chunk({"content": "PI_FIXTURE_"})
                chunk({"content": "OK"})
                chunk({}, "stop")
            self.wfile.write(b"data: [DONE]\n\n")
            self.wfile.flush()
        except (BrokenPipeError, ConnectionResetError):
            pass


def wait_for(action, timeout=90):
    deadline = time.monotonic() + timeout
    while time.monotonic() < deadline:
        result = action()
        if result:
            return result
        time.sleep(0.2)
    raise TimeoutError(f"timed out waiting for {action.__name__}")


def command(client, method, payload, *, target=None, revision=None, global_scope=False):
    params = {"command_id": str(uuid.uuid4()), "payload": payload, "target": target, "expected_revision": revision}
    if global_scope:
        params["scope"] = "global"
    if getattr(client, "catalog_generation", None) is not None:
        params["expected_catalog_generation"] = client.catalog_generation
    return client.rpc(method, params)


def create_agent(c, host_url, harness_provider):
    if harness_provider != "dsh":
        provider, harness, native = (f"{harness_provider}-ollama", f"builtin-harness-{harness_provider}-managed:1",
                                     f"builtin-tool-{harness_provider}-native:1")
    else:
        provider, harness, native = "ollama", "builtin-harness-dsh-multi-provider:2", "builtin-tool-dsh-native:2"
    role = command(c, "role.create", {"name": "Pi fixture role", "scope": "global",
                   "data": {"instructions": ROLE_MARKER}})["id"]
    model = command(c, "model.create", {"name": "Pi fixture model", "scope": "global",
                    "data": {"provider": provider, "model": MODEL, "endpoint": host_url,
                             "context_window": 32768, "max_output_tokens": 4096}})["id"]
    capability = command(c, "capability.create", {
        "name": "Pi fixture capability", "scope": "global", "model_ref": model,
        "harness_ref": harness, "tools": [native], "skills": [], "data": {},
    })["id"]
    agent = command(c, "agent.create", {"name": "Pi fixture Agent", "scope": "global",
                    "role_ref": role, "capability_ref": capability, "data": {}})["id"]
    instance = command(c, "agent.instance.create", {"name": "Pi fixture instance", "scope": "global",
                       "agent_revision_id": agent})["id"]
    return role, model, instance


def discussion(client, server, reports, restart):
    c = client
    host_url = f"http://127.0.0.1:{server.server_port}/v1"
    provider = server.harness_provider
    assert c.rpc("credentials.status", {"model_provider": f"{provider}-ollama"})["auth_kind"] == "none"
    resources = c.rpc("query.permission_resources")
    harness_binding = next(row for row in resources["targets"]
                           if row["target"]["revision_id"] == f"builtin-harness-{provider}-managed:1")
    harness = harness_binding["target"]
    profile = "builtin-permission-harness-read-only:2"
    preflight = c.rpc("permission.binding.preflight", {"target": harness, "permission_revision_id": profile})
    assert preflight
    command(c, "permission.bindings.apply", {"bindings": [{
        "target": harness, "permission_revision_id": profile,
        "expected_revision": (harness_binding["active"] or {}).get("revision", 0),
    }]})
    role, model, instance = create_agent(c, host_url, provider)
    session = command(c, "session.open", {"agent_instance_id": instance},
                      target="global", revision=0, global_scope=True)["id"]

    def view():
        return c.rpc("query.session", {"id": session, "scope": "global", "limit": 100})

    def send(prompt):
        return command(c, "conversation.send", {"client_message_id": str(uuid.uuid4()), "input": prompt},
                       target=session, revision=view()["session"]["revision"], global_scope=True)["id"]

    def finish(turn_id, expected="completed"):
        def terminal():
            current = view()
            turn = next(row for row in current["turns"] if row["id"] == turn_id)
            if turn["status"] in ("completed", "failed", "cancelled", "needs_reconciliation"):
                (reports / "discussion.json").write_text(json.dumps(current, ensure_ascii=False, indent=2) + "\n")
                assert turn["status"] == expected, (expected, turn)
                return current
            return None
        return wait_for(terminal)

    first = send("PI_CONTEXT")
    initial = finish(first)
    assistant = [e for e in initial["events"] if e["kind"] == "assistant"]
    assert any(e["mode"] == "delta" and "PI_FIXTURE_" in e["content"] for e in assistant)
    assert any(e["mode"] == "full" and e["content"] == "PI_FIXTURE_OK" for e in assistant)
    assert len([r for r in initial["experience_reads"] if r["method"] == "agent.experience.list" and r["status"] == "succeeded"]) == 1
    assert any(row["scenario"] == "PI_CONTEXT" and row["frozen_role"] and row["results"] for row in server.audit)
    assert any(event["kind"] == "tool" for event in initial["events"])
    binding = initial["session"]["runtime_binding"]
    native = initial["session"]["external_session"]
    assert binding["installation"]["key"]["provider"] == provider and native
    print(f"PASS {provider} discussion, streamed response and context tool receipt", flush=True)

    blocked = finish(send("PI_READ_ONLY"))
    results = [row["results"] for row in server.audit if row["scenario"] == "PI_READ_ONLY" and row["results"]]
    assert results and "frozen DevDesk Permission denies" in json.dumps(results), results
    assert not list((reports / "data").rglob(WRITE))
    assert blocked["session"]["external_session"] == native
    print(f"PASS {provider} native write denied by frozen read-only policy", flush=True)
    finish(send("PI_PRIVATE_PATH"))
    private = [row["results"] for row in server.audit if row["scenario"] == "PI_PRIVATE_PATH" and row["results"]]
    assert private and "frozen DevDesk Permission denies" in json.dumps(private), private
    assert ROLE_MARKER not in json.dumps(private)
    print(f"PASS {provider} native file read uses the permission-checked path", flush=True)

    c = restart()
    after = finish(send("PI_RESUME"))
    assert after["session"]["external_session"] == native
    assert after["session"]["runtime_binding"] == binding
    assert after["experience_reads"] == initial["experience_reads"]
    assert any(row["scenario"] == "PI_RESUME" and row["has_prior_context"] and row["model"] == MODEL for row in server.audit)
    print(f"PASS {provider} session resumed after a Host restart", flush=True)

    cancelling = send("PI_CANCEL")

    def ready_to_cancel():
        current = view()
        turn = next(row for row in current["turns"] if row["id"] == cancelling)
        if turn["status"] in ("completed", "failed", "cancelled", "needs_reconciliation"):
            (reports / "discussion.json").write_text(json.dumps(current, ensure_ascii=False, indent=2) + "\n")
            raise AssertionError(("Cancel canary settled before its stream marker", turn))
        if not any(event["turn_id"] == cancelling and event["content"] == "PI_CANCEL_IN_PROGRESS"
                   for event in current["events"]):
            return None
        return turn

    wait_for(ready_to_cancel)
    for attempt in range(3):
        turn = ready_to_cancel()
        try:
            command(c, "conversation.cancel", {}, target=cancelling, revision=turn["revision"], global_scope=True)
            break
        except RuntimeError as error:
            if attempt == 2 or "revision_conflict" not in str(error):
                raise
    cancelled = finish(cancelling, "cancelled")
    assert cancelled["executions"] and all(e["writer_revoked"] and e["status"] == "released"
                                           for e in cancelled["executions"])
    active = subprocess.check_output(["docker", "ps", "-aq", "--no-trunc"], text=True).splitlines()
    assert not any(e.get("container_id") in active for e in cancelled["executions"])
    server.release_cancel.set()
    print(f"PASS {provider} cancellation", flush=True)
    return c, role, model


def bound_external_tool(c, server, reports, project, role, model):
    scope = "project:" + project
    origin = f"http://host.docker.internal:{server.server_port}"
    provider = server.harness_provider
    harness = f"builtin-harness-{provider}-managed:1"
    source = command(c, "integration.create", {"name": "Pi HTTP fixture", "scope": scope,
                     "data": {"source": {"kind": "http_api", "base_url": origin}}})["id"]

    def tool(name):
        return command(c, "integration.tool.create", {
            "name": name, "scope": scope, "data": {"schema_version": 2, "qualified_name": name,
                "description": "Read the fixture marker.", "integration_ref": source,
                "implementation": {"kind": "http_api", "method": "GET", "path": "/canary"},
                "input_schema": {"type": "object", "properties": {}, "additionalProperties": False},
                "output_schema": {"type": "object", "required": ["marker"], "properties": {"marker": {"type": "string"}}},
                "effect": "read_only",
            },
        })["id"]

    included, unbound = tool("pi_fixture_external"), tool("pi_fixture_unbound")
    binding = command(c, "integration.binding.create", {
        "name": "Pi fixture account", "scope": scope, "data": {
            "integration_ref": source, "harness_ref": harness, "sandbox_ref": "builtin-sandbox-docker:2",
            "allow_private_network": True, "allowed_origins": [origin], "allowed_tools": [included],
            "timeout_ms": 15000, "max_result_bytes": 1048576,
        },
    })["id"]
    command(c, "integration.project.bind", {"project_id": project, "bindings": {source: binding}})
    capability = command(c, "capability.create", {
        "name": "Pi external tool capability", "scope": scope, "harness_ref": harness, "model_ref": model,
        "tools": [f"builtin-tool-{provider}-native:1", included, unbound], "skills": [], "data": {},
    })["id"]
    agent = command(c, "agent.create", {"name": "Pi external tool Agent", "scope": scope,
                    "role_ref": role, "capability_ref": capability, "data": {}})["id"]
    project_revision = c.rpc("query.execution_workspace")["active_project"]["revision"]
    opened = command(c, "session.open", {"agent": agent}, target=project, revision=project_revision)
    session = opened["id"]
    turn = command(c, "conversation.send", {"client_message_id": str(uuid.uuid4()), "input": "PI_EXTERNAL"},
                   target=session, revision=opened["revision"])["id"]

    def finished():
        view = c.rpc("query.session", {"id": session})
        row = next(item for item in view["turns"] if item["id"] == turn)
        assert row["status"] not in ("failed", "needs_reconciliation", "cancelled"), view["events"]
        return view if row["status"] == "completed" else None

    wait_for(finished)
    calls = c.rpc("query.tool_calls", {"session_id": session, "turn_id": turn})["records"]
    (reports / "external-tool-calls.json").write_text(json.dumps(calls, ensure_ascii=False, indent=2) + "\n")
    assert len(calls) == 1 and calls[0]["name"] == "pi_fixture_external", calls
    receipt = calls[0]["data"]
    assert receipt["status"] == "succeeded" and receipt["receipt"]["origin"] == "external" and receipt["reply_sent"]
    rows = [row for row in server.audit if row.get("scenario") == "PI_EXTERNAL"]
    assert rows and all(not any(tool.endswith("pi_fixture_unbound") for tool in row["tools"]) for row in rows)
    assert any("PI_EXTERNAL_FIXTURE_OK" in json.dumps(row["results"]) for row in rows)
    assert server.external_calls == 1
    print(f"PASS {provider} bound HTTP integration receipt and unbound tool filtering", flush=True)


def workflow(client, server, reports, role, model):
    provider = server.harness_provider
    root = Path(__file__).resolve().parents[2]
    fixtures = reports / "domains"
    subprocess.run(["python3", str(root / "tests/fixtures/domain-packages/build.py"), str(fixtures)], check=True)
    package = (fixtures / "git-probe.zip").read_bytes()
    installed = client.rpc("domain.install", {"archive": base64.b64encode(package).decode()})
    current = client.rpc("query.domain.active")
    client.rpc("domain.activate", {"epoch": current["epoch"], "digest": installed["digest"]})
    epoch = client.rpc("query.domain.active")["epoch"]
    project = client.rpc("domain.project.create", {
        "epoch": epoch, "title": "Pi native step acceptance", "command_id": str(uuid.uuid4()),
    })["id"]
    c = Client(client.url, project=project, domain_epoch=epoch)
    c.catalog_generation = getattr(client, "catalog_generation", None)
    capture = command(c, "tool.create", {
        "name": "Pi step capture", "data": {"qualified_name": "materials.capture", "implementation": "platform",
        "input_schema": {"type": "object", "additionalProperties": True}, "output_schema": {"type": "object"}},
    })["id"]
    capability = command(c, "capability.create", {
        "name": "Pi step capability", "model_ref": model, "harness_ref": f"builtin-harness-{provider}-managed:1",
        "tools": [f"builtin-tool-{provider}-native:1", capture], "skills": [], "data": {},
    })["id"]
    agent = command(c, "agent.create", {"name": "Pi step Agent", "role_ref": role,
                    "capability_ref": capability, "data": {}})["id"]
    spec = command(c, "spec.create", {"name": "Pi step contract", "data": {
        "schema_version": 3, "body": "Record the captured note.",
        "checks": [{"key": "note", "name": "Note", "instructions": "Keep the captured note."}],
    }})["id"]
    delivery = command(c, "delivery_type.create", {"name": "Pi step output", "spec_refs": [spec], "data": {
        "format": "json", "content_schema": {"type": "object", "required": ["note"]},
    }})["id"]
    document = {
        "schema_version": 6, "stages": [{"key": "fixture", "name": "Fixture", "default_role_ref": role}],
        "steps": [{"key": "capture", "name": "Capture fixture", "stage": "fixture", "executor": {"type": "agent"},
                   "task_instructions": "PI_STEP", "outputs": {"result": {"delivery_ref": delivery}}}],
    }
    if provider != "pi":
        document["steps"].append({**document["steps"][0], "key": "confirm",
                                  "name": "Confirm captured note", "dependencies": ["capture"],
                                  "task_instructions": "PI_CONFIRM",
                                  "inputs": {"captured": {"type": "source",
                                             "source": {"step": "capture", "port": "result", "pointer": ""}}}})
    draft = command(c, "workflow.create", {"name": f"{provider} native tool workflow", "document": document})
    definition = command(c, "workflow.publish", {}, target=draft["id"], revision=draft["revision"])
    owner = c.rpc("query.execution_workspace")["active_project"]
    run = command(c, "workflow.start", {
        "workflow_ref": definition["id"], "role_agent_bindings": {role: agent}, "orchestration": {"mode": "rust"},
    }, target=project, revision=owner["revision"])["id"]

    def completed():
        view = c.rpc("query.workflow_instance", {"id": run})
        if view["record"]["data"]["status"] in ("completed", "failed", "cancelled", "needs_reconciliation"):
            (reports / "workflow.json").write_text(json.dumps(view, ensure_ascii=False, indent=2) + "\n")
            assert view["record"]["data"]["status"] == "completed", view["runtime"]["steps"]
            return view
        return None

    try:
        result = wait_for(completed, timeout=120 * len(document["steps"]))
    except TimeoutError:
        view = c.rpc("query.workflow_instance", {"id": run})
        (reports / "workflow.json").write_text(json.dumps(view, ensure_ascii=False, indent=2) + "\n")
        raise
    all_calls = []
    steps = result["runtime"]["steps"]
    assert len(steps) == len(document["steps"]), steps
    expected_notes = ["fixture"] if provider == "pi" else ["fixture", CAPTURED]
    actual_notes = []
    for step in steps:
        calls = c.rpc("query.tool_calls", {"turn_id": step["turn_id"]})["records"]
        all_calls.extend(calls)
        matched = [call for call in calls if call["type"] == "platform_call" and call["data"]["status"] == "succeeded"]
        assert len(matched) == 1, calls
        actual_notes.append(matched[0]["data"]["platform_receipt"]["inputs"]["note"])
        session = c.rpc("query.session", {"id": step["session_id"]})
        assert any(event["kind"] == "tool" and "devdesk_materials_capture" in event["content"] for event in session["events"])
    (reports / "step-tool-calls.json").write_text(json.dumps(all_calls, ensure_ascii=False, indent=2) + "\n")
    assert sorted(actual_notes) == sorted(expected_notes), all_calls
    assert sum(bool(row["results"]) for row in server.audit if row["scenario"] in ("PI_STEP", "PI_CONFIRM")) == len(steps)
    if provider != "pi":
        assert any(row["scenario"] == "PI_CONFIRM" and row["has_captured_material"] and row["results"] for row in server.audit)
    print(f"PASS {provider} completed {len(steps)} workflow steps with model, platform and conversation receipts", flush=True)
    bound_external_tool(c, server, reports, project, role, model)


def rehearse_upgrade(args, server, reports, start, stop):
    old = start(args.v24_binary or args.v25_binary)
    before_ids = {r["id"] for r in old.rpc("query.model_providers")["providers"]}
    assert ("pi-ollama" in before_ids) == bool(args.v25_binary)
    assert before_ids.isdisjoint({"opencode-ollama", "mcode-ollama"})
    harnesses = ["builtin-harness-codex-managed:2", "builtin-harness-dsh-multi-provider:2"]
    if args.v25_binary:
        harnesses.append("builtin-harness-pi-managed:1")
    installs = [old.rpc("runtime.prepare_harness", {"id": reference}) for reference in harnesses]
    role, _, instance = create_agent(old, f"http://127.0.0.1:{server.server_port}/v1", "dsh")
    def dsh_turn(client, prompt, predecessor=None):
        payload = {"agent_instance_id": instance, "force_new": True}
        revision = 0
        if predecessor:
            payload["continue_from"] = {"session_id": predecessor}
            original = client.rpc("query.session", {"id": predecessor, "scope": "global"})
            revision = original["session"]["revision"]
        opened = command(client, "session.open", payload, target="global", revision=revision, global_scope=True)
        session = opened["id"]
        sent = command(client, "conversation.send", {"client_message_id": str(uuid.uuid4()), "input": prompt},
                       target=session, revision=opened["revision"], global_scope=True)["id"]

        def terminal():
            view = client.rpc("query.session", {"id": session, "scope": "global"})
            turn = next(row for row in view["turns"] if row["id"] == sent)
            return view if turn["status"] in ("completed", "failed", "cancelled", "needs_reconciliation") else None

        return session, wait_for(terminal)

    session, before = dsh_turn(old, "PI_LEGACY_DATASET")
    assert any(e["kind"] == "user" and e["content"] == "PI_LEGACY_DATASET" for e in before["events"])
    stop()
    if args.v24_binary:
        upgrade = subprocess.check_output([str(args.binary.resolve()), "storage", "upgrade-global",
                                           "--data", str(reports / "data")], text=True)
        migration = json.loads(upgrade)
        (reports / "upgrade.json").write_text(upgrade)
        assert migration["source_version"] == 24 and migration["target_version"] == 25
        assert migration["runtime_installations"] >= 2
    else:
        with sqlite3.connect(f"{(reports / 'data/global.sqlite3').as_uri()}?mode=ro", uri=True) as database:
            assert database.execute("PRAGMA user_version").fetchone() == (25,)
    new = start()
    assert {r["id"] for r in new.rpc("query.model_providers")["providers"]} == before_ids
    preview = new.rpc("query.template_reset_preview")
    update = command(new, "configuration.templates.reset", {
        "preserve_custom": True, "expected_catalog_generation": preview["generation"],
        "catalog_fingerprint": preview["catalog_fingerprint"], "bundle_hash": preview["bundle_hash"],
    })
    assert update["changed"]
    new.catalog_generation = update["generation"]
    expected = {"pi-ollama"}
    if args.provider != "pi" or args.v25_binary:
        expected.update(("opencode-ollama", "mcode-ollama"))
    assert expected <= {r["id"] for r in new.rpc("query.model_providers")["providers"]}
    assert any(row["id"] == role for row in new.rpc("query.configurations", {"kind": "role"})["records"])
    resources = new.rpc("query.permission_resources")
    bindings = {row["id"]: row for row in resources["bindings"]}
    changes = []
    legacy_providers = {"codex", "dsh", "pi", "docker"} if args.v25_binary else {"codex", "dsh", "docker"}
    for row in resources["targets"]:
        target = row["target"]
        if row["provider"] not in legacy_providers or not row["active"]:
            continue
        active = row["active"]
        profile = bindings[active["binding_revision_id"]]["data"]["permission_revision_id"]
        assert new.rpc("permission.binding.preflight", {"target": target, "permission_revision_id": profile})
        changes.append({"target": target, "permission_revision_id": profile, "expected_revision": active["revision"]})
    assert len(changes) >= 3
    command(new, "permission.bindings.apply", {"bindings": changes})
    after = new.rpc("query.session", {"id": session, "scope": "global"})
    assert after["events"] == before["events"]
    assert after["session"]["runtime_binding"] == before["session"]["runtime_binding"]
    assert after["permission_status"]["requires_successor"]
    for install in installs:
        prepared = new.rpc("runtime.prepare", {"provider": install["key"]["provider"]})
        assert prepared["id"] == install["id"] and prepared["artifact"] == install["artifact"]
    _, dsh = dsh_turn(new, "PI_UPGRADED_DATASET", predecessor=session)
    assert dsh["session"]["predecessor"] == session
    assert dsh["turns"][0]["status"] == "completed", dsh["events"]
    assert any(e["kind"] == "assistant" and e["content"] == "PI_FIXTURE_OK" for e in dsh["events"])
    version = "v24" if args.v24_binary else "batch A v25"
    print(f"PASS {version} discussion and installations survived the catalog update and permission rebinding", flush=True)
    return new


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--provider", choices=("pi", "opencode", "mcode"), default="pi")
    parser.add_argument("--binary", type=Path, required=True)
    parser.add_argument("--reports", type=Path, required=True)
    previous = parser.add_mutually_exclusive_group()
    previous.add_argument("--v24-binary", type=Path, help="first rehearse an isolated v24 upgrade")
    previous.add_argument("--v25-binary", type=Path, help="first rehearse the batch A v25 catalog and permission drift")
    args = parser.parse_args()
    reports = args.reports.resolve()
    reports.mkdir(parents=True, exist_ok=True)
    assert not (reports / "data").exists(), "use a fresh evidence directory"
    host = None
    server = ThreadingHTTPServer(("0.0.0.0", 0), Model)
    server.harness_provider = args.provider
    server.audit_lock = threading.Lock()
    server.audit = []
    server.external_calls = 0
    server.release_cancel = threading.Event()
    threading.Thread(target=server.serve_forever, daemon=True).start()

    def stop():
        if host is not None and host.poll() is None:
            host.send_signal(signal.SIGINT)
            try:
                host.wait(timeout=45)
            except subprocess.TimeoutExpired:
                host.kill()
                host.wait(timeout=5)
                raise

    def start(binary=None):
        nonlocal host
        binary = binary or args.binary
        log = reports / f"host-{time.time_ns()}.log"
        with log.open("w") as stream:
            host = subprocess.Popen([str(binary.resolve()), "--data", str(reports / "data"), "--port", "0"],
                                    stdout=stream, stderr=subprocess.STDOUT)

        def endpoint():
            assert host.poll() is None, log.read_text()
            return next((line for line in log.read_text().splitlines() if line.startswith("http://127.0.0.1:")), None)

        client = Client(wait_for(endpoint, timeout=20))
        client.catalog_generation = client.rpc("query.template_catalog")["generation"]
        return client

    def restart():
        stop()
        return start()

    try:
        client = rehearse_upgrade(args, server, reports, start, stop) if args.v24_binary or args.v25_binary else start()
        prepared = client.rpc("runtime.prepare", {"provider": args.provider})
        (reports / "runtime.json").write_text(json.dumps(prepared, ensure_ascii=False, indent=2) + "\n")
        print(f"PASS {args.provider} runtime.prepare", flush=True)
        client, role, model = discussion(client, server, reports, restart)
        workflow(client, server, reports, role, model)
    finally:
        server.release_cancel.set()
        stop()
        server.shutdown()
        server.server_close()
        (reports / "model-audit.json").write_text(json.dumps(server.audit, ensure_ascii=False, indent=2) + "\n")


if __name__ == "__main__":
    main()

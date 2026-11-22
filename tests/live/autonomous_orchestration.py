"""Audit durable actors and actual turn inputs, independently of final success."""
import hashlib
import json
from pathlib import Path
import re


def stored_payload(connection, value):
    marker = "$devdesk_project_payload_v1"
    prefix = "\x1ddevdesk-payload/v1:json:"
    if isinstance(value, str) and value.startswith(prefix):
        command = json.loads(value[len(prefix):])
        for field in ("arguments", "input"):
            if field in command[3]:
                command[3][field] = stored_payload(connection, command[3][field])
        return json.dumps(command, ensure_ascii=False, separators=(",", ":"), sort_keys=True)
    if not isinstance(value, dict) or marker not in value:
        return value
    body = value[marker]
    sha = body["sha256"]
    assert re.fullmatch(r"[0-9a-f]{64}", sha) and "artifact" not in body
    root = Path(connection.execute("PRAGMA database_list").fetchone()[2]).parent
    path = root / "payloads" / "objects" / sha[:2] / sha[2:4] / sha
    if path.is_file():
        data = path.read_bytes()
    else:
        pack, offset, size = connection.execute(
            "SELECT pack,offset,bytes FROM payload_pack_objects WHERE sha256=?", (sha,)
        ).fetchone()
        assert re.fullmatch(r"[0-9a-f-]{36}", pack)
        with (root / "payloads" / "packs" / f"{pack}.pack").open("rb") as file:
            file.seek(offset)
            data = file.read(size)
    assert len(data) == body["bytes"] and hashlib.sha256(data).hexdigest() == sha
    return json.loads(data)


def fingerprint_matches(stored, request):
    prefix = "devdesk-command-sha256/v1:"
    if stored.startswith(prefix):
        body = json.dumps(request, ensure_ascii=False, separators=(",", ":")).encode()
        return stored == prefix + hashlib.sha256(body).hexdigest()
    return stored == request


def verify(connection, root_id):
    records = {row[0]: (row[1], json.loads(row[2])) for row in connection.execute(
        "SELECT id,type,data FROM governance_records")}
    controllers = [(identity, data) for identity, (kind, data) in records.items()
                   if kind == "workflow_controller" and data.get("root_instance_id") == root_id
                   and not data.get("workspace_only")]
    assert len(controllers) == 1, "Expected a single uninterrupted root manager"
    controller_id, controller = controllers[0]
    assert controller["epoch"] == 1, "Autonomous baseline cannot be taken over or resumed"
    assert not any(kind == "controller_input" for kind, _ in records.values()), "Autonomous baseline received external steering"
    turns = {identity: json.loads(body) for identity, body in connection.execute(
        "SELECT id,input FROM turns WHERE session_id=?", (controller["session_id"],))}
    assert turns, "No actual manager turns"
    bootstraps = 0
    delivered_events = set()
    maximum_event_batch_bytes = 0
    for payload in turns.values():
        assert "instructions" not in payload, "Runtime repeated orchestration instructions"
        assert payload.get("user_messages", []) == [], "Unexpected supplemental manager instructions"
        assert payload["root_instance_id"] == root_id, "Foreign manager delivery"
        assert payload.get("delivery_id"), "Missing durable manager delivery identity"
        events = payload.get("events", [])
        size = len(json.dumps(events, ensure_ascii=False, separators=(',', ':')).encode())
        maximum_event_batch_bytes = max(maximum_event_batch_bytes, size)
        assert len(events) <= 32 and size <= 65536, "Event delivery exceeds its count or byte bound"
        for event in events:
            assert event['source']['root_instance_id'] == root_id, "Foreign root event delivery"
            assert event['id'] not in delivered_events, "Source event delivered in multiple manager turns"
            delivered_events.add(event['id'])
        if payload.get("bootstrap"):
            bootstraps += 1
        else:
            assert "instances" not in payload, "Repeated graph snapshot instead of events"
            assert payload.get("events") or payload.get("deadline_reached"), "Wakeup without a cause"
    assert bootstraps == 1, "Expected one initial manager goal"
    calls = [(identity, data) for identity, (kind, data) in records.items()
             if kind == "controller_tool_call" and data.get("status") == "succeeded"]
    for _, call in calls:
        assert call["turn_id"] in turns and call["epoch"] == 1, "Native call lacks its manager turn"
    decisions = 0
    initial_starts = 0
    start_commands = 0
    for identity, (kind, data) in records.items():
        if kind == "command_context":
            method = connection.execute(
                "SELECT name FROM governance_records WHERE id=?", (identity,)).fetchone()[0]
            if "context" in data:
                actor = data["context"]["actor"]
            else:
                # Formal session/input commits retain their durable coordinator
                # origin directly, rather than an InvocationContext envelope.
                assert method in ("session.open", "conversation.send"), "Unknown audit envelope"
                origin = data["origin"]
                assert origin["kind"] == "runtime_coordinator"
                intent_kind, intent = records[origin["workflow_intent_id"]]
                assert intent_kind == "workflow_intent" and intent["method"] == method
                if "controller_id" in origin:
                    assert origin["controller_id"] == controller_id and origin["epoch"] == 1
                actor = origin.get("actor", {"kind": "coordinator"})
            if actor["kind"] == "controller":
                assert actor["controller_id"] == controller_id and actor["epoch"] == 1
            if actor["kind"] == "human" and method.startswith(("workflow.", "orchestration.")):
                assert method in ("workflow.start", "workflow.decision.resolve"), "Human or script performed manager work: " + method
                if method == "workflow.start":
                    root = records[root_id][1]
                    request_digest = stored_payload(connection, data["request_digest"])
                    request = json.loads(request_digest)
                    assert data["target"] == root["owner_id"] and request[1] == root["owner_id"]
                    if request[3].get("workflow_ref") is not None:
                        assert request[3]["workflow_ref"] == root["workflow_ref"]
                    else:
                        # Default engineering starts freeze the selected Workflow
                        # inside the original atomic command result, not in an
                        # optional explicit request override.
                        command = connection.execute(
                            "SELECT definition_id FROM governance_records WHERE id=?", (identity,)).fetchone()[0]
                        saved = connection.execute(
                            "SELECT fingerprint,result FROM operations WHERE id=?", (command,)).fetchone()
                        assert saved and fingerprint_matches(saved[0], request_digest), "Default start lacks its exact durable command receipt"
                        receipt = stored_payload(connection, json.loads(saved[1]))
                        assert (receipt["id"] == root_id and receipt["type"] == "workflow_instance"
                                and receipt["data"]["owner_id"] == root["owner_id"]
                                and receipt["data"]["workflow_ref"] == root["workflow_ref"]), "Default start selected a different root or Workflow"
                    start_commands += 1
        if kind == "workflow_decision":
            actor = data["actor"]
            if actor["kind"] == "human" and data.get("operation") == "start":
                assert data.get("instance_id") == root_id and data.get("root_instance_id") == root_id, "Human started another workflow"
                initial_starts += 1
                continue
            if actor["kind"] == "segment":
                actor = records[actor["authorization_id"]][1]["actor"]
            assert (actor["kind"] == "controller" and actor["controller_id"] == controller_id
                    and actor["epoch"] == 1), "Business advancement bypassed the manager"
            decisions += 1
        if kind == "workflow_pending_decision":
            assert data["actor"]["kind"] == "controller", "Approval request was not prepared by manager"
            assert data["request"].get("interaction"), "Approval lacks materials and explanation"
            assert any(call["method"] == "workflow.decision.request"
                       and call.get("receipt", {}).get("id") == identity for _, call in calls), "Approval lacks its actual native manager request"
        if kind == "workflow_human_decision":
            assert any(other_kind == "workflow_pending_decision"
                       and other.get("decision_ref") == identity
                       for other_kind, other in records.values()), "Human signature lacks manager request"
        if kind == "controller_interaction":
            assert data.get('interaction') and data['turn_id'] in turns, "Human notice lacks its manager turn"
            assert any(call['method'] == 'workflow.controller.notify'
                       and call.get('receipt', {}).get('id') == identity for _, call in calls), "Human notice lacks its actual native manager call"
    assert start_commands == 1 and initial_starts in (0, 1), "Expected exactly one initial Human start"
    assert decisions, "No manager business decisions"
    return {"passed": True, "controller_id": controller_id, "manager_turns": len(turns),
            "bootstrap_count": bootstraps, "external_steering_count": 0,
            "initial_human_start_commands": start_commands,
            "delivered_events": len(delivered_events), "maximum_event_batch_bytes": maximum_event_batch_bytes,
            "human_feedback_packets": sum(kind == 'controller_interaction' for kind, _ in records.values()),
            "business_decisions": decisions, "native_calls": len(calls)}

"""Read an actual workflow tree through bounded status pages, without issuing controls."""
import json
import os
import hashlib
from pathlib import Path


class ReviewBasisError(RuntimeError):
    """A preserved request needs review; this never authorizes a control."""


def preserve_human_request(output, request, manager_url, check):
    """Persist the original packet before any potentially failing read/check.

    check is read-only. Classification is not evidence: production allows the
    current execution in either list. Review sufficiency is a separate check.
    """
    identity = request.get("id", "")
    if not identity or Path(identity).name != identity or identity in (".", ".."):
        raise ReviewBasisError("Invalid request identity")
    path = Path(output) / ("human-request-" + identity + ".json")
    packet = {"request": request, "manager_url": manager_url}
    try:
        with path.open("x") as stream:
            json.dump(packet, stream, ensure_ascii=False, indent=2)
            stream.flush()
            os.fsync(stream.fileno())
    except FileExistsError:
        if json.loads(path.read_text()) != packet:
            raise ReviewBasisError("Preserved request identity changed: " + identity)
    directory = os.open(path.parent, os.O_RDONLY)
    try:
        os.fsync(directory)
    finally:
        os.close(directory)
    try:
        check(request)
    except Exception as error:
        with path.with_suffix(".validation.jsonl").open("a") as stream:
            stream.write(json.dumps({"passed": False, "error": str(error)}) + "\n")
            stream.flush()
            os.fsync(stream.fileno())
        raise ReviewBasisError("Saved request needs review: " + str(path)) from error
    return path


def check_human_request(client, tree, request, expected_ancestors=None):
    """Check current, exact review links; the command remains authoritative.

    Reads only exact executions/contracts. Missing upstream/output links block
    review, even when a structurally valid request was accepted by production.
    This function never signs, repairs a request, or advances a workflow.
    """
    def require(condition, message):
        if not condition:
            raise ReviewBasisError(message)

    data = request["data"]
    if expected_ancestors is not None:
        require(request.get("ancestor_instance_ids") == expected_ancestors and
                request["definition_id"] not in expected_ancestors, "Not the expected child approval")
    owner = tree[request["definition_id"]]["runtime"]
    arguments = data["request"]["arguments"]
    interaction = data["request"]["interaction"]
    require(len(json.dumps(interaction, ensure_ascii=False, separators=(",", ":")).encode()) <= 32768,
            "Review explanation exceeds production bound")
    require(request.get("current", True) and not owner.get("cancel_requested") and
            owner.get("status") not in ("completed", "cancelled"), "Stale or cancelled review")
    require(data["status"] == "pending", "Request is no longer pending")
    require(data["plan_revision_id"] == arguments["plan_revision_id"] ==
            owner["active_plan_revision_id"], "Stale approval plan")
    for key in ("title", "background", "requested_action", "completed_work", "review_summary"):
        require(isinstance(interaction.get(key), str) and interaction[key].strip(),
                "Missing review explanation: " + key)
    for key in ("materials", "evidence"):
        require(isinstance(interaction.get(key), list) and len(interaction[key]) <= 32,
                "Invalid bounded reference list: " + key)
    references = interaction["materials"] + interaction["evidence"]
    require(any(r["instance_id"] == request["definition_id"] and
                r["execution_id"] == arguments["execution_id"] for r in references),
            "Missing current execution")
    executions = {}
    def execution(instance, identity):
        key = (instance, identity)
        if key not in executions:
            require(instance in tree, "Reference outside current root")
            require(tree[instance]["runtime"]["project_id"] == owner["project_id"],
                    "Reference outside project")
            executions[key] = client.rpc("workflow.execution.inspect", {
                "project_id": owner["project_id"], "id": instance, "execution_id": identity})
        return executions[key]
    current = execution(request["definition_id"], arguments["execution_id"])
    require(current["status"] in ("waiting_confirmation", "awaiting_acceptance"),
            "Execution is not a current review gate")
    require(any(s["id"] == current["id"] for s in owner["steps"]), "Replaced execution")
    for key in ("input_digest", "outputs"):
        require(arguments[key] == current[key], "Stale review " + key)
    linked_outputs = set()
    for reference in references:
        require(reference.get("label") and reference.get("purpose"), "Missing link explanation")
        exact = execution(reference["instance_id"], reference["execution_id"])
        actual_owner = tree[reference["instance_id"]]["runtime"]
        for key, actual in (("owner_id", actual_owner["owner_id"]),
                            ("session_id", exact.get("session_id")), ("turn_id", exact.get("turn_id"))):
            require(reference.get(key) == actual, "Missing or stale context: " + key)
        if reference.get("output_id"):
            require(reference["output_id"] in exact["outputs"].values(), "Output changed or belongs to another execution")
            linked_outputs.add(reference["output_id"])
        else:
            require(reference in interaction["evidence"], "Material lacks output")
    require(set(current["outputs"].values()) <= linked_outputs, "Missing required output")
    # Frozen contract lookup is deliberately explicit and bounded, once per new
    # Human request rather than on the ordinary status polling path.
    contract_params = {
        "project_id": owner["project_id"], "id": request["definition_id"],
        "execution_id": current["id"], "part": "contract", "max_bytes": 65536}
    chunks, offset, digest = [], 0, None
    for _ in range(129):
        contract = client.rpc("workflow.execution.inspect", {**contract_params, "offset": offset,
            **({"value_digest": digest} if digest is not None else {})})
        require(contract.get("offset") == offset and
                contract.get("execution_id") == current["id"] and
                contract.get("plan_revision_id") == current["plan_revision_id"], "Contract identity/cursor changed")
        require(isinstance(contract.get("total_bytes"), int) and
                0 <= contract["total_bytes"] <= 8 * 1024 * 1024, "Review contract size is invalid")
        if digest is None:
            digest = contract["value_digest"]
        require(contract["value_digest"] == digest, "Contract digest changed")
        chunk = contract["value_chunk"]
        chunks.append(chunk)
        offset += len(chunk.encode())
        require(offset <= 8 * 1024 * 1024, "Review contract exceeds observer byte budget")
        if contract.get("next_offset") is None:
            require(offset == contract["total_bytes"], "Incomplete contract")
            break
        require(contract["next_offset"] == offset and chunk, "Contract cursor did not advance")
    else:
        raise ReviewBasisError("Review contract exceeds observer chunk budget")
    body = "".join(chunks)
    require(hashlib.sha256(body.encode()).hexdigest() == digest, "Contract content digest mismatch")
    for binding in json.loads(body).get("inputs", {}).values():
        if binding.get("type") != "source" or binding["source"]["step"] in ("$input", "$material"):
            continue
        source = binding["source"]
        upstream = next(s for s in owner["steps"] if s["step"] == source["step"])
        require(upstream["outputs"][source["port"]] in linked_outputs, "Missing upstream output")


def manager_activity(view):
    """Track bounded manager activity, independent of omitted internal cursors.

    This is observer recovery grace, not the product's business-progress watchdog.
    A project event cursor alone can change for unrelated bookkeeping.
    """
    record = view.get("record") or {}
    data = record.get("data", {})
    calls = tuple((row.get("id"), (row.get("data") or row).get("status"))
                  for row in view.get("calls", []))
    return record.get("revision"), data.get("session_id"), data.get("status"), calls


def read_workflow_tree(client, root_id):
    pending = [root_id]
    views = {}
    while pending:
        identity = pending.pop(0)
        if identity in views:
            continue
        view = client.rpc("query.workflow_instance", {"id": identity, "view": "status"})
        step_page = view
        while step_page.get("next_step_offset") is not None:
            step_page = client.rpc("query.workflow_instance", {
                "id": identity, "view": "status", "step_offset": step_page["next_step_offset"],
            })
            view["runtime"]["steps"].extend(step_page["runtime"]["steps"])
            view["plan"]["steps"].extend(step_page["plan"]["steps"])
            for key in ("step_revisions", "completion_status", "actors"):
                view[key].update(step_page.get(key, {}))
        view["record"]["data"] = view["runtime"]
        views[identity] = view
        page = view
        while True:
            pending.extend(child["id"] for child in page.get("children", []))
            offset = page.get("next_child_offset")
            if offset is None:
                break
            page = client.rpc("query.workflow_instance", {
                "id": identity, "view": "status", "child_offset": offset,
            })
    return views

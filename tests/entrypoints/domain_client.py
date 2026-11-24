"""Exercise the domain protocol using only Python's HTTP client."""
import base64
import hashlib
import http.cookiejar
import json
import sys
import time
import urllib.request
import uuid


class Client:
    def __init__(self, origin):
        self.origin = origin
        self.http = urllib.request.build_opener(
            urllib.request.HTTPCookieProcessor(http.cookiejar.CookieJar()))
        self.request("/session", {}).close()

    def request(self, path, data=None):
        return self.http.open(urllib.request.Request(
            self.origin + path, data=json.dumps(data).encode() if data is not None else None,
            headers={"content-type": "application/json"}), timeout=25)

    def call(self, method, **params):
        with self.request("/rpc", {"jsonrpc": "2.0", "id": 1,
                                   "method": method, "params": params}) as response:
            reply = json.load(response)
        if "error" in reply:
            raise RuntimeError(reply["error"])
        return reply["result"]


def next_epoch(stream):
    fields = {}
    while True:
        chunk = stream.readline()
        assert chunk, "domain event stream closed"
        line = chunk.decode().strip()
        if not line and "data" in fields:
            assert fields["event"] == "domain"
            return int(fields["data"])
        if ":" in line and not line.startswith(":"):
            key, value = line.split(":", 1)
            fields[key] = value.strip()


def lookup(client, epoch, receipt):
    return client.call("query.domain.workflow", epoch=epoch,
                       project_id=receipt["project_id"], execution_id=receipt["execution_id"])


def prepare_document(client, epoch):
    project = client.call("domain.project.create", epoch=epoch, title="Editorial acceptance",
                          command_id=uuid.uuid4().hex)["id"]
    target = {"epoch": epoch, "project_id": project, "entity_id": str(uuid.uuid4())}
    original = {"type_ref": "manuscript.document:1", "expected_revision": 0,
                "data": {"title": "Draft"}, "command_id": uuid.uuid4().hex}
    try:
        client.call("domain.entity.put", **target, **{**original, "data": {"title": 42}})
    except RuntimeError as failure:
        assert "domain_contract" in str(failure), failure
    else:
        raise AssertionError("domain entity accepted the wrong data type")
    entity = client.call("domain.entity.put", **target, **original)
    assert entity["revision"] == 1 and client.call("domain.entity.put", **target, **original) == entity
    revised = {**original, "expected_revision": 1, "data": {"title": "  Local manuscript  "},
               "command_id": uuid.uuid4().hex}
    assert client.call("domain.entity.put", **target, **revised)["revision"] == 2
    try:
        client.call("domain.entity.put", **target, **{**revised, "command_id": uuid.uuid4().hex})
    except RuntimeError as failure:
        assert "revision_conflict" in str(failure), failure
    else:
        raise AssertionError("domain entity update used an obsolete revision")
    sibling = {**target, "entity_id": str(uuid.uuid4())}
    sibling_fields = {**original, "command_id": uuid.uuid4().hex,
                      "relations": {"previous": [target["entity_id"]]}}
    assert client.call("domain.entity.put", **sibling, **sibling_fields)["relations"] == sibling_fields["relations"]
    for links, error in [
        ({"previous": [sibling["entity_id"]]}, "不可成环"),
        ({"previous": [sibling["entity_id"], target["entity_id"]]}, "基数"),
        ({"undeclared": [sibling["entity_id"]]}, "未声明"),
    ]:
        try:
            client.call("domain.entity.put", **target, **{
                **revised, "expected_revision": 2, "command_id": uuid.uuid4().hex, "relations": links})
        except RuntimeError as failure:
            assert error in str(failure), failure
        else:
            raise AssertionError(f"accepted invalid relation {links}")
    another = client.call("domain.project.create", epoch=epoch, title="Unrelated editorial project",
                          command_id=uuid.uuid4().hex)["id"]
    try:
        client.call("domain.entity.put", **{**sibling, "project_id": another}, **sibling_fields)
    except RuntimeError as failure:
        assert "not_found" in str(failure), failure
    else:
        raise AssertionError("accepted relation to a different project")
    return client.call("query.domain.entity", **target)


def start_for(client, epoch, entity, mode):
    request = {"epoch": epoch, "workflow_ref": "manuscript.workflow:1", "mode": mode,
               "project_id": entity["project_id"], "entity_id": entity["id"],
               "expected_revision": entity["revision"], "command_id": uuid.uuid4().hex}
    receipt = client.call("domain.workflow.start", **request)
    assert client.call("domain.workflow.start", **request) == receipt
    assert receipt["owner_id"] == entity["id"]
    return receipt


def wait_view(client, epoch, receipt, matches, timeout=25):
    end = time.monotonic() + timeout
    details = None
    while time.monotonic() < end:
        details = lookup(client, epoch, receipt)
        if matches(details):
            return details
        status = details["record"]["data"]["status"]
        if status in {"failed", "cancelled", "needs_reconciliation"}:
            raise AssertionError((status, receipt))
        time.sleep(0.25)
    raise AssertionError((receipt, [(step["step"], step["status"], step["reason"])
                                   for step in (details or {}).get("runtime", {}).get("steps", [])]))


def confirm_proof(client, epoch, receipt, view):
    step = view["runtime"]["steps"][0]
    return client.call("workflow.step.confirm", domain_epoch=epoch, project_id=receipt["project_id"],
                       target=receipt["execution_id"], expected_revision=view["record"]["revision"],
                       command_id=uuid.uuid4().hex, payload={"step": "title", "execution_id": step["id"],
                       "input_digest": step["input_digest"], "outputs": step["outputs"],
                       "accept_result": True, "reason": "I reviewed the exact worker output."})


def submitted_proof(client, epoch, receipt, timeout=25):
    details = wait_view(client, epoch, receipt, lambda view: view["record"]["data"]["status"] == "completed", timeout)
    submission = details["domain_submission"]
    assert submission["owner_id"] == details["record"]["data"]["owner_id"]
    proof = client.call("query.domain.entity", epoch=epoch, project_id=receipt["project_id"],
                        entity_id=submission["entities"]["observation"])
    assert proof["data"] == {"title": "Local manuscript", "observedBy": "domain-worker"}
    assert proof["relations"] == {"manuscript": [submission["owner_id"]]}
    origin = proof["origin"]
    assert origin["owner_id"] == submission["owner_id"] and origin["owner_revision"] == 2
    assert origin["root_id"] == receipt["execution_id"]
    step = details["runtime"]["steps"][0]
    assert origin["execution_id"] == step["id"] and origin["input_digest"] == step["input_digest"]
    assert origin["input_digest"] != hashlib.sha256(b"{}").hexdigest()
    assert origin["output_id"] == step["outputs"]["observation"]
    assert origin["artifact_id"] == details["outputs"][origin["output_id"]]["artifact_id"]
    assert origin["producer"]["kind"] == "platform"
    assert origin["human_signature_id"]
    with client.request(f"/artifacts/{origin['artifact_id']}?project_id={receipt['project_id']}&domain_epoch={epoch}") as response:
        assert "Local manuscript" in response.read().decode()
    return proof


def run(origin, action, args):
    client = Client(origin)
    with client.request("/capabilities") as response:
        assert json.load(response)["domain_protocol"] == 1
    active = client.call("query.domain.active")
    epoch = active["epoch"]
    if action == "install-contract":
        try:
            client.call("domain.install", archive=base64.b64encode(b"not a domain ZIP").decode())
        except RuntimeError as failure:
            assert "invalid_domain_package" in str(failure), failure
        else:
            raise AssertionError("invalid domain ZIP was accepted")
        with open(args[0], "rb") as archive:
            contents = base64.b64encode(archive.read()).decode()
        first = client.call("domain.install", archive=contents)
        assert client.call("domain.install", archive=contents) == first
        assert client.call("query.domain.active") == active
        return first
    if action == "git":
        assert active["active"]["id"] == "git-probe"
        assert active["contributions"][0]["id"] == "git-probe.overview"
        return client.call("query.domain.invoke", epoch=epoch,
                           handler=active["contributions"][0]["query"], input={"path": args[0]})
    if action == "workflow":
        assert active["active"]["id"] == "manuscript"
        assert {"workflow", "spec", "skill", "agent"} <= {row["type"] for row in active["sources"]}
        assert active["agent_workflows"] == ["manuscript.workflow:1"]
        entity = prepare_document(client, epoch)
        receipt = start_for(client, epoch, entity, "rust")
        view = wait_view(client, epoch, receipt, lambda view: view["runtime"]["steps"][0]["status"] == "waiting_confirmation")
        assert "domain_submission" not in view
        confirm_proof(client, epoch, receipt, view)
        proof = submitted_proof(client, epoch, receipt)
        return {**receipt, "entity_id": entity["id"], "proof_id": proof["id"]}
    if action == "prepare-owner":
        return prepare_document(client, epoch)
    if action == "manual":
        project, root = args
        view = client.call("query.workflow_control", domain_epoch=epoch, project_id=project, id=root)
        dispatch = {
            "command_id": uuid.uuid4().hex, "project_id": project, "target": root,
            "expected_revision": view["record"]["revision"],
            "payload": {"execution_id": view["runtime"]["steps"][0]["id"], "step": "title"},
        }
        for stale in ({}, {"domain_epoch": epoch - 1}):
            try:
                client.call("workflow.step.dispatch", **dispatch, **stale)
            except RuntimeError as failure:
                expected = "domain_epoch_conflict" if stale else "invalid_params"
                assert expected in str(failure), failure
            else:
                raise AssertionError("manual dispatch accepted without the active epoch")
        client.call("workflow.step.dispatch", domain_epoch=epoch, **dispatch)
        receipt = {"project_id": project, "execution_id": root}
        view = wait_view(client, epoch, receipt, lambda view: view["runtime"]["steps"][0]["status"] == "waiting_confirmation")
        confirm_proof(client, epoch, receipt, view)
        view = lookup(client, epoch, receipt)
        client.call("workflow.complete", command_id=uuid.uuid4().hex, domain_epoch=epoch,
                    project_id=project, target=root, expected_revision=view["record"]["revision"], payload={})
        return {**receipt, "proof_id": submitted_proof(client, epoch, receipt)["id"]}
    if action == "unload":
        active_projects = client.call("query.project_directory")
        assert active_projects["total_projects"] > 0, active_projects
        # Start a manual root that stays active until a human sends a control command.
        pending = start_for(client, epoch, prepare_document(client, epoch), "manual")
        try:
            client.call("domain.uninstall", epoch=epoch, digest=active["active"]["digest"])
        except RuntimeError as failure:
            assert "domain_busy" in str(failure), failure
        else:
            raise AssertionError("active domain workflow did not block unload")
        assert client.call("query.domain.active")["epoch"] == epoch
        details = client.call("query.domain.workflow", epoch=epoch, **pending)
        client.call("workflow.cancel", command_id=uuid.uuid4().hex,
                    domain_epoch=epoch,
                    project_id=pending["project_id"], target=pending["execution_id"],
                    expected_revision=details["record"]["revision"], payload={})
        end = time.monotonic() + 12
        while time.monotonic() < end:
            details = client.call("query.domain.workflow", epoch=epoch, **pending)
            if details["record"]["data"]["status"] == "cancelled":
                break
            time.sleep(0.2)
        else:
            raise AssertionError(details)
        old = epoch
        project_events = f"/events?project_id={pending['project_id']}&domain_epoch={epoch}"
        with (client.request("/domain-events") as events,
              client.request(project_events + "&configuration=true") as replay,
              client.request(project_events + "&tail=true") as tail):
            assert next_epoch(events) == epoch
            assert replay.readline()
            assert tail.readline()
            result = client.call("domain.uninstall", epoch=epoch, digest=active["active"]["digest"])
            assert next_epoch(events) == result["epoch"]
            assert isinstance(replay.read(), bytes)
            assert isinstance(tail.read(), bytes)
        assert result["active"] is None and result["epoch"] > epoch, result
        assert client.call("query.project_directory")["total_projects"] == 0
        try:
            client.call("query.domain.invoke", epoch=old, handler="manuscript.title.read", input={})
        except RuntimeError as failure:
            assert "domain_epoch_conflict" in str(failure), failure
        else:
            raise AssertionError("stale epoch was accepted")
        return result
    if action == "read":
        return client.call("query.domain.workflow", epoch=epoch, project_id=args[0], execution_id=args[1])
    if action == "read-entity":
        return client.call("query.domain.entity", epoch=epoch, project_id=args[0], entity_id=args[1])
    if action == "check-foreign-entity":
        try:
            client.call("query.domain.entity", epoch=epoch, project_id=args[0], entity_id=args[1])
        except RuntimeError as failure:
            assert "not_found" in str(failure), failure
            return True
        raise AssertionError("domain entity was readable from a different project")
    if action == "project-read":
        method, project, data = args
        assert method.startswith("query.")
        return client.call(method, **json.loads(data), project_id=project, domain_epoch=epoch)
    if action == "contribution":
        return client.call("query.domain.contribution", epoch=int(args[0]), contribution=args[1],
                           input=json.loads(args[2]))
    if action == "stale-contribution":
        try:
            client.call("query.domain.contribution", epoch=int(args[0]), contribution=args[1],
                        input={"project_id": args[2]})
        except RuntimeError as failure:
            assert "domain_epoch_conflict" in str(failure), failure
            return True
        raise AssertionError("project contribution accepted an obsolete generation")
    if action == "software":
        assert active["active"]["id"] == "software-development"
        path, revision = args
        defaults = active["defaults"]
        assert {defaults["workflow_ref"], defaults["task_workflow_ref"]} <= set(active["agent_workflows"])
        assert defaults["standard_ref"].rsplit(":", 1)[0] == "software-development.standard-engineering-delivery"
        if active["active"]["version"] == "0.5.0":
            assert defaults["workflow_ref"] == "software-development.workflow-feature-development:6"
            assert defaults["task_workflow_ref"] == "software-development.workflow-task-development:5"
            for row in active["sources"]:
                if row["id"] in (defaults["workflow_ref"], defaults["task_workflow_ref"]):
                    assert "delivery_policy" not in row["data"]
                    assert row["data"]["operation_evidence"]["git.delivery"]["checks"]["$completion"] == ["delivery_receipt"]
        coordinate = {"path": path, "revision": revision}
        actual = client.call("query.domain.invoke", epoch=epoch,
                             handler="software-development.tests.discover", input=coordinate)
        assert actual["revision"] == revision
        assert actual["configuration"]["regression"]["command"]["argv"] == ["npm", "test"]
        project = client.call("domain.project.create", epoch=epoch, title="Software repository preflight",
                              command_id=uuid.uuid4().hex)["id"]
        owner = str(uuid.uuid4())
        client.call("domain.entity.put", epoch=epoch, project_id=project, entity_id=owner,
                    command_id=uuid.uuid4().hex, type_ref="software-development.repository:1",
                    expected_revision=0, data=coordinate)
        branch = client.call("query.domain.invoke", epoch=epoch,
                             handler="software-development.branch.plan", input={"owner": owner, "kind": "feature"})
        assert branch["branch"] == f"devdesk/features/{owner}"
        candidate = client.call("query.domain.invoke", epoch=epoch,
                                handler="software-development.branch.plan", input={"owner": owner, "kind": "candidate"})
        assert candidate["branch"] == f"devdesk/candidates/{owner}"
        marker = f"<!-- devdesk:project={project}:merge={owner} -->"
        plan = client.call("query.domain.invoke", epoch=epoch, handler="software-development.pull-request.create",
                           input={"marker": marker, "request": {
                               "merge": owner, "head_branch": branch["branch"], "base_branch": "main",
                               "candidate": revision, "expected_target": revision}})
        assert plan["head"] == branch["branch"] and plan["base"] == "main"
        assert plan["body"].splitlines()[0] == marker and revision in plan["body"]
        run = client.call("domain.workflow.start", epoch=epoch, project_id=project, entity_id=owner,
                          expected_revision=1, command_id=uuid.uuid4().hex,
                          workflow_ref="software-development.repository-preflight:1")
        completed = wait_view(client, epoch, run, lambda view: view["record"]["data"]["status"] == "completed")
        entity_id = completed["domain_submission"]["entities"]["test_configuration"]
        proof = client.call("query.domain.entity", epoch=epoch, project_id=project, entity_id=entity_id)
        assert proof["data"] == actual and proof["origin"]["owner_id"] == owner
        assert {row["type"] for row in active["sources"]} >= {"agent", "spec", "workflow", "skill"}
        return {"run": run, "branch": branch["branch"], "proof_id": proof["id"], "revision": revision,
                "epoch": epoch, "contributions": active["contributions"],
                "version": active["active"]["version"], "standard_ref": defaults["standard_ref"]}
    raise ValueError(action)


if __name__ == "__main__":
    print(json.dumps(run(sys.argv[1], sys.argv[2], sys.argv[3:])))

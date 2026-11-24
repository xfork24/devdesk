"""Exercise the domain protocol using only Python's HTTP client."""
import base64
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


def completed(client, epoch, receipt):
    end = time.monotonic() + 24
    while time.monotonic() < end:
        details = client.call("query.domain.workflow", epoch=epoch, **receipt)
        status = details.get("record", {}).get("data", {}).get("status")
        if status == "completed":
            assert "Local manuscript" in json.dumps(details), details
            return receipt
        if status in {"failed", "cancelled", "needs_reconciliation"}:
            raise AssertionError((status, details["record"]["id"]))
        time.sleep(0.2)
    raise AssertionError((status, details["record"]["id"]))


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
        start = {"epoch": epoch, "workflow_ref": "manuscript.workflow:1", "command_id": uuid.uuid4().hex}
        receipt = client.call("domain.workflow.start", **start)
        completed(client, epoch, receipt)
        assert client.call("domain.workflow.start", **start) == receipt
        return receipt
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
        end = time.monotonic() + 16
        while time.monotonic() < end:
            view = client.call("query.domain.workflow", epoch=epoch, **receipt)
            step = view["runtime"]["steps"][0]
            if step["status"] == "awaiting_acceptance":
                break
            time.sleep(0.2)
        else:
            raise AssertionError("manual tool output was not accepted")
        artifact = next(iter(view["outputs"].values()))["artifact_id"]
        with client.request(f"/artifacts/{artifact}?project_id={project}&domain_epoch={epoch}") as response:
            assert "Local manuscript" in response.read().decode()
        client.call("workflow.step.accept", command_id=uuid.uuid4().hex, domain_epoch=epoch,
                    project_id=project, target=root, expected_revision=view["record"]["revision"],
                    payload={key: step[value] for key, value in
                             [("execution_id", "id"), ("input_digest", "input_digest"), ("outputs", "outputs")]})
        view = client.call("query.domain.workflow", epoch=epoch, **receipt)
        client.call("workflow.complete", command_id=uuid.uuid4().hex, domain_epoch=epoch,
                    project_id=project, target=root, expected_revision=view["record"]["revision"], payload={})
        return completed(client, epoch, receipt)
    if action == "unload":
        active_projects = client.call("query.project_directory")
        assert active_projects["total_projects"] > 0, active_projects
        # Start a manual root that stays active until a human sends a control command.
        pending = client.call("domain.workflow.start", epoch=epoch, mode="manual",
                              workflow_ref="manuscript.workflow:1", command_id=uuid.uuid4().hex)
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
    raise ValueError(action)


if __name__ == "__main__":
    print(json.dumps(run(sys.argv[1], sys.argv[2], sys.argv[3:])))

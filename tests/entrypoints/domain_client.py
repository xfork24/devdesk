"""Exercise the domain protocol using only Python's HTTP client."""
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


def run(origin, action, args):
    client = Client(origin)
    with client.request("/capabilities") as response:
        assert json.load(response)["domain_protocol"] == 1
    active = client.call("query.domain.active")
    epoch = active["epoch"]
    if action == "git":
        assert active["active"]["id"] == "git-probe"
        assert active["contributions"][0]["id"] == "git-probe.overview"
        return client.call("query.domain.invoke", epoch=epoch,
                           handler=active["contributions"][0]["query"], input={"path": args[0]})
    if action == "workflow":
        assert active["active"]["id"] == "manuscript"
        assert {"workflow", "spec", "skill", "agent"} <= {row["type"] for row in active["sources"]}
        receipt = client.call("domain.workflow.start", epoch=epoch,
                              workflow_ref="manuscript.workflow:1", command_id=uuid.uuid4().hex)
        end = time.monotonic() + 24
        while time.monotonic() < end:
            details = client.call("query.domain.workflow", epoch=epoch, **receipt)
            data = details.get("record", {}).get("data", {})
            status = data.get("status")
            if status == "completed":
                assert "Local manuscript" in json.dumps(details), details
                return receipt
            if status in {"failed", "cancelled", "needs_reconciliation"}:
                raise AssertionError(details)
            time.sleep(0.2)
        raise AssertionError(details)
    if action == "unload":
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
        with client.request("/domain-events") as events:
            assert next_epoch(events) == epoch
            result = client.call("domain.uninstall", epoch=epoch, digest=active["active"]["digest"])
            assert next_epoch(events) == result["epoch"]
        assert result["active"] is None and result["epoch"] > epoch, result
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

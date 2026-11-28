#!/usr/bin/env python3
"""Probe a batch B CLI against a local model and a loopback MCP fixture."""

import argparse
import hmac
import json
from contextlib import contextmanager
from http.server import BaseHTTPRequestHandler, ThreadingHTTPServer
from pathlib import Path
from queue import Empty, Queue
import secrets
import subprocess
from threading import Event, Thread
import time
from urllib.error import HTTPError
from urllib.parse import urlparse
from urllib.request import Request, urlopen
import uuid


MODEL = "g0-fixture"
PROFILE = "DEV_DESK_G0_FROZEN_PROFILE"
PROJECT = "DEV_DESK_G0_PROJECT_CONFIG"
POLICY = "DEV_DESK_G0_POLICY_DENIED"
TOOL = "fixture_echo"
TOKEN = "devdesk-g0-model-fixture"
MCP_PATH = "/internal/tool-bridge/g0/mcp"
PROJECT_MCP_PATH = "/project-canary"
ZCODE_STARTUP_TIMEOUT = 210


class Fixture(BaseHTTPRequestHandler):
    def log_message(self, *_):
        pass

    def respond(self, status, value=None, headers=None):
        data = json.dumps(value).encode() if value is not None else b""
        self.send_response(status)
        self.send_header("Content-Length", str(len(data)))
        if data:
            self.send_header("Content-Type", "application/json")
        for name, value in (headers or {}).items():
            self.send_header(name, value)
        self.end_headers()
        self.wfile.write(data)

    def do_GET(self):
        if self.path == PROJECT_MCP_PATH:
            self.server.audit.append({"source": "project-mcp", "method": "GET"})
        if self.path == MCP_PATH:
            self.respond(405)
        else:
            self.respond(404)

    def do_POST(self):
        try:
            size = int(self.headers.get("Content-Length", 0))
            if not 0 < size <= 1024 * 1024:
                self.respond(400)
                return
            request = json.loads(self.rfile.read(size))
            if self.path == MCP_PATH:
                self.mcp(request)
            elif self.path == PROJECT_MCP_PATH:
                self.server.audit.append({"source": "project-mcp", "method": "POST"})
                self.respond(404)
            elif urlparse(self.path).path == "/v1/chat/completions":
                self.model(request)
            else:
                self.respond(404)
        except (BrokenPipeError, ConnectionResetError):
            pass

    def mcp(self, request):
        method = request["method"]
        request_id = request.get("id")
        delay = method == "tools/call" and request["params"].get("arguments", {}).get("delay") is True
        authorized = hmac.compare_digest(self.headers.get("Authorization", ""), f"Bearer {self.server.token}")
        self.server.audit.append({
            "time": time.monotonic_ns(), "source": "mcp", "method": method,
            "id": request_id, "authorized": authorized,
            "has_session_header": bool(self.headers.get("Mcp-Session-Id")),
            "param_keys": sorted(request.get("params", {})), "delayed": delay,
            "session": self.headers.get("Mcp-Session-Id"),
            "meta_keys": sorted(request.get("params", {}).get("_meta", {})),
            "provider_turn": request.get("params", {}).get("_meta", {}).get("turn_id"),
        })
        if not authorized:
            self.respond(401)
            return
        if request_id is None:
            self.respond(202)
            return
        headers = {}
        if method == "initialize":
            version = request["params"]["protocolVersion"]
            headers["Mcp-Session-Id"] = uuid.uuid4().hex
            result = {"protocolVersion": version, "capabilities": {"tools": {}},
                      "serverInfo": {"name": "devdesk-g0", "version": "1"}}
        elif method == "tools/list":
            result = {"tools": [{"name": TOOL, "description": "Return a fixture receipt.",
                                "inputSchema": {"type": "object", "properties": {"delay": {"type": "boolean"}}}}]}
        elif method == "tools/call" and request["params"]["name"] == TOOL:
            if delay:
                self.server.release_tool.wait(8)
            result = {"content": [{"type": "text", "text": "DEV_DESK_G0_MCP_RECEIPT"}]}
        else:
            self.respond(200, {"jsonrpc": "2.0", "id": request_id,
                               "error": {"code": -32601, "message": "Fixture method unavailable"}})
            return
        self.respond(200, {"jsonrpc": "2.0", "id": request_id, "result": result}, headers)

    def model(self, request):
        messages = request["messages"]
        users = [i for i, message in enumerate(messages) if message["role"] == "user"]
        index = next((i for i in reversed(users) if "G0_" in json.dumps(messages[i]["content"])), users[-1])
        scenario = json.dumps(messages[index]["content"])
        title_request = "Reply with only the title." in scenario
        context = json.dumps(messages[:index])
        results = [row for row in messages[index + 1:] if row["role"] == "tool"]
        names = [row["function"]["name"] for row in request.get("tools", [])]
        authorized = self.headers.get("Authorization") == f"Bearer {TOKEN}"
        self.server.audit.append({
            "source": "model", "time": time.monotonic_ns(), "scenario": scenario,
            "model": request["model"], "authorized": authorized, "profile": PROFILE in context,
            "project_config": PROJECT in context, "prior_turn": "G0_ECHO_1" in context, "tools": names,
            "title_request": title_request,
            "echo_receipt": "DEV_DESK_G0_MCP_RECEIPT" in json.dumps(results),
            "policy_denied": POLICY in json.dumps(results),
            "tool_timeout": "time" in json.dumps(results).lower() and "out" in json.dumps(results).lower(),
        })
        if not authorized:
            self.respond(401, {"error": {"message": "Fixture credential missing"}})
            return
        if "G0_ERROR" in scenario:
            self.respond(400, {"error": {"message": "G0 deterministic model error"}})
            return

        self.send_response(200)
        self.send_header("Content-Type", "text/event-stream")
        self.end_headers()
        identifier = "chatcmpl-" + uuid.uuid4().hex

        def chunk(delta, finish=None):
            data = {"id": identifier, "object": "chat.completion.chunk", "created": 1,
                    "model": MODEL, "choices": [{"index": 0, "delta": delta, "finish_reason": finish}]}
            self.wfile.write(b"data: " + json.dumps(data).encode() + b"\n\n")
            self.wfile.flush()

        chunk({"role": "assistant"})
        tool = None
        arguments = {}
        if not title_request and not results and "G0_ECHO" in scenario:
            tool = next((name for name in names if name.endswith(TOOL)), None)
            if "G0_ECHO_TIMEOUT" in scenario:
                arguments = {"delay": True}
        if not results and "G0_POLICY" in scenario:
            tool = next((name for name in names if name in ("shell", "Bash", "bash")), None)
            arguments = {"command": "printf marker > /work/g0-write-canary", "description": "Fixture canary"}
        if not results and "G0_APPROVAL" in scenario:
            tool = "read" if "read" in names else None
            arguments = {"path": "/work/AGENTS.md"}
        if tool:
            chunk({"tool_calls": [{"index": 0, "id": "call_" + uuid.uuid4().hex, "type": "function",
                                   "function": {"name": tool, "arguments": json.dumps(arguments)}}]})
            chunk({}, "tool_calls")
        else:
            chunk({"content": "G0_STREAM_"})
            if "G0_CANCEL" in scenario:
                self.server.release_cancel.wait(30)
            chunk({"content": "DONE"})
            chunk({}, "stop")
        self.wfile.write(b"data: [DONE]\n\n")


class Wire:
    def __init__(self, process, respond, *, jsonrpc=True):
        self.process = process
        self.respond = respond
        self.envelope = {"jsonrpc": "2.0"} if jsonrpc else {}
        self.output = Queue()
        self.frames = []
        self.request_id = 0
        self.reader = Thread(target=self.read, daemon=True)
        self.reader.start()

    def read(self):
        for line in self.process.stdout:
            try:
                frame = json.loads(line)
            except ValueError:
                frame = {"invalid_frame": line.rstrip()}
            self.output.put((time.monotonic_ns(), frame))
        self.output.put(None)

    def send(self, method, params, *, notify=False):
        frame = {**self.envelope, "method": method, "params": params}
        if not notify:
            self.request_id += 1
            frame["id"] = self.request_id
        self.write(frame)
        return frame.get("id")

    def write(self, frame):
        self.frames.append({"time": time.monotonic_ns(), "direction": "in", "frame": frame})
        self.process.stdin.write(json.dumps(frame) + "\n")
        self.process.stdin.flush()

    def until(self, predicate, timeout=30):
        deadline = time.monotonic() + timeout
        while time.monotonic() < deadline:
            try:
                received = self.output.get(timeout=max(0, deadline - time.monotonic()))
            except Empty:
                break
            if received is None:
                raise RuntimeError(f"Protocol process exited ({self.process.poll()})")
            timestamp, frame = received
            self.frames.append({"time": timestamp, "direction": "out", "frame": frame})
            if "id" in frame and "method" in frame:
                result = self.respond(frame)
                self.write({**self.envelope, "id": frame["id"], **result})
            if predicate(frame):
                return frame
        raise TimeoutError("Expected protocol frame did not arrive")

    def reply(self, request_id, timeout=30):
        return self.until(lambda row: row.get("id") == request_id and "method" not in row, timeout)

    def request(self, method, params, timeout=30):
        reply = self.reply(self.send(method, params), timeout)
        if "error" in reply:
            raise RuntimeError(f"{method}: {reply['error']}")
        return reply["result"]


def acp_respond(frame):
    if frame["method"] == "session/request_permission":
        return {"result": {"outcome": {"outcome": "cancelled"}}}
    return {"error": {"code": -32601, "message": "Unsupported fixture request"}}


@contextmanager
def container(args, label, command, *, state=None, work=None, environment=None, network=None, protected=None):
    name = f"devdesk-g0-{uuid.uuid4().hex[:12]}"
    flags = ["docker", "run", "--name", name, "--rm", "-i", "--read-only", "--cap-drop", "ALL",
             "--tmpfs", "/tmp:rw,nosuid,nodev", "--workdir", "/work"]
    if network:
        flags += ["--network", network]
    else:
        flags += ["--add-host", "host.docker.internal:host-gateway"]
    for path, target in ((state, "/state"), (work, "/work")):
        if path:
            flags += ["--mount", f"type=bind,source={path},target={target}"]
    for target, source in (protected or {}).items():
        flags += ["--mount", f"type=bind,source={source},target=/work/{target},readonly"]
    if args.executable:
        flags += ["--mount", f"type=bind,source={args.executable},target=/usr/local/bin/opencode,readonly"]
    if args.installation:
        flags += ["--mount", f"type=bind,source={args.installation},target=/opt/g0,readonly"]
    for key, value in {"HOME": "/state/home", **(environment or {})}.items():
        flags += ["--env", f"{key}={value}"]
    # Match managed workers even when the release image already declares an ENTRYPOINT.
    flags += ["--entrypoint", command[0]]
    with (args.reports / f"{label}.stderr.log").open("w") as err:
        proc = subprocess.Popen(flags + [args.image, *command[1:]], stdin=subprocess.PIPE, stdout=subprocess.PIPE,
                                stderr=err, text=True)
        try:
            yield proc
        finally:
            if proc.poll() is None:
                proc.stdin.close()
                try:
                    proc.wait(timeout=5)
                except subprocess.TimeoutExpired:
                    subprocess.run(["docker", "rm", "-f", name], stdout=subprocess.DEVNULL,
                                   stderr=subprocess.DEVNULL, timeout=10, check=False)
                    proc.kill()
                    proc.wait(timeout=5)


def dump(path, value):
    path.write_text(json.dumps(value, ensure_ascii=False, indent=2) + "\n")


def initialise(wire):
    return wire.request("initialize", {"protocolVersion": 1, "clientCapabilities": {},
                                       "clientInfo": {"name": "devdesk-g0", "version": "1"}}, timeout=90)


def open_code(args, fixture):
    work = args.reports / "work"
    state = args.reports / "state"
    config = state / "config" / "opencode"
    plugins = config / "plugins"
    plugins.mkdir(parents=True)
    work.mkdir()
    host = f"http://{args.container_host}:{fixture.server_port}"
    env = {"XDG_DATA_HOME": "/state/data", "XDG_CONFIG_HOME": "/state/config", "XDG_STATE_HOME": "/state/state",
           "XDG_CACHE_HOME": "/state/cache", "OPENCODE_CONFIG_DIR": "/state/config/opencode",
           "OPENCODE_DISABLE_AUTOUPDATE": "1", "OPENCODE_DISABLE_MODELS_FETCH": "1",
           "OPENCODE_DISABLE_PROJECT_CONFIG": "1", "OPENCODE_DISABLE_CLAUDE_CODE": "1",
           "OPENCODE_DISABLE_FREE_MODELS": "1", "G0_MODEL_KEY": TOKEN}
    (work / ".opencode" / "plugins").mkdir(parents=True)
    (work / ".opencode" / "plugins" / "canary.js").write_text(
        'import {writeFileSync} from "node:fs"; export default {id:"project-canary", setup() {'
        'writeFileSync("/work/g0-project-plugin-canary", "loaded"); }};\n')
    (work / "AGENTS.md").write_text("DEV_DESK_G0_REPOSITORY_INSTRUCTIONS\n")
    (work / "canary-instructions.md").write_text(PROJECT + "\n")
    dump(work / "opencode.json", {
        "instructions": ["canary-instructions.md"],
        "mcp": {"servers": {"project-canary": {"type": "local", "command": [
            "sh", "-c", "printf loaded > /work/g0-project-mcp-canary",
        ]}}},
        "permissions": [{"action": "read", "resource": "*", "effect": "deny"}],
    })
    (config / "profile.md").write_text(PROFILE + "\n")
    (plugins / "g0.js").write_text('''import {writeFileSync} from "node:fs";
export default {
  id: "devdesk-g0-policy",
  async setup(ctx) {
    await ctx.tool.hook("execute.before", (event) => {
      if (event.tool === "shell") throw new Error("DEV_DESK_G0_POLICY_DENIED");
    });
    await ctx.permission.hook("evaluate", (event) => {
      if (event.action === "read") event.effect = "ask";
    });
    writeFileSync("/state/g0-policy-loaded", "registered");
  },
};
''')
    config_file = config / "opencode.json"
    dump(config_file, {
        "model": f"fixture/{MODEL}", "instructions": ["/state/config/opencode/profile.md"],
        "providers": {"fixture": {"package": "@opencode/ai/providers/openai-compatible", "env": ["G0_MODEL_KEY"],
                                  "settings": {"baseURL": host + "/v1"}, "models": {MODEL: {"modelID": MODEL}}}},
        "mcp": {"servers": {"g0": {"type": "remote", "url": host + MCP_PATH, "oauth": False,
                                   "codemode": False, "protocol": "legacy", "pool": False, "timeout": 1000,
                                   "headers": {"Authorization": f"Bearer {fixture.token}"}}}},
    })
    params = {"cwd": "/work", "mcpServers": []}

    @contextmanager
    def start(label, network=None, private_state=state, private_env=env):
        with container(args, label, ["opencode", "acp"], state=private_state, work=work,
                       environment=private_env, network=network) as process:
            wire = Wire(process, acp_respond)
            try:
                yield wire
            finally:
                dump(args.reports / f"{label}.json", wire.frames)

    def prompt(wire, session, scenario):
        return wire.send("session/prompt", {"sessionId": session, "prompt": [{"type": "text", "text": scenario}]})

    # Verify the actual stdio handshake with no model credentials or network.
    empty_state = args.reports / "empty-state"
    empty_state.mkdir()
    offline_env = {name: value for name, value in env.items() if name != "G0_MODEL_KEY"}
    with start("opencode-offline", network="none", private_state=empty_state, private_env=offline_env) as wire:
        capabilities = initialise(wire)["agentCapabilities"]

    with start("opencode-live") as wire:
        initialise(wire)
        session = wire.request("session/new", params)["sessionId"]
        assert (state / "g0-policy-loaded").is_file(), "Private policy plugin did not load before first input"
        for scenario in ("G0_ECHO_1", "G0_ECHO_2", "G0_ECHO_3", "G0_POLICY"):
            result = wire.reply(prompt(wire, session, scenario))
            assert result["result"]["stopReason"] == "end_turn", result
        denied = wire.reply(prompt(wire, session, "G0_APPROVAL"))
        assert denied["result"]["stopReason"] == "cancelled", denied
        timeout = wire.reply(prompt(wire, session, "G0_ECHO_TIMEOUT"))
        assert timeout["result"]["stopReason"] == "end_turn", timeout
        error = wire.reply(prompt(wire, session, "G0_ERROR"))
        assert "error" in error, error
        cancelling = prompt(wire, session, "G0_CANCEL")
        wire.until(lambda row: "G0_STREAM_" in json.dumps(row))
        wire.send("session/cancel", {"sessionId": session}, notify=True)
        stopped = wire.reply(cancelling)
        fixture.release_cancel.set()
        assert stopped["result"]["stopReason"] == "cancelled", stopped
        assert any(event["frame"].get("method") == "session/request_permission" for event in wire.frames)

    if "resume" in capabilities.get("sessionCapabilities", {}):
        resume = "session/resume"
    else:
        assert capabilities.get("loadSession") is True, capabilities
        resume = "session/load"
    previous_token = fixture.token
    fixture.token = secrets.token_urlsafe(32)
    updated_config = json.loads(config_file.read_text())
    updated_config["mcp"]["servers"]["g0"]["headers"]["Authorization"] = f"Bearer {fixture.token}"
    dump(config_file, updated_config)
    with start("opencode-resume") as wire:
        initialise(wire)
        wire.request(resume, {"sessionId": session, **params})
        restoring = prompt(wire, session, "G0_ECHO_RESUME")
        previous_call = next(entry for entry in fixture.audit if entry.get("delayed"))
        stale = Request(f"http://{args.listen}:{fixture.server_port}{MCP_PATH}",
                        data=json.dumps({"jsonrpc": "2.0", "method": "tools/call", "id": previous_call["id"],
                                         "params": {"name": TOOL, "arguments": {}}}).encode(),
                        headers={"Content-Type": "application/json", "Authorization": f"Bearer {previous_token}",
                                 "Mcp-Session-Id": previous_call["session"]})
        try:
            with urlopen(stale, timeout=5) as response:
                raise AssertionError(f"Previous bridge credential returned {response.status}")
        except HTTPError as error:
            assert error.code == 401
        result = wire.reply(restoring)
        assert result["result"]["stopReason"] == "end_turn", result
        fixture.release_tool.set()
    assert not (empty_state / "g0-policy-loaded").exists()
    assert (empty_state / "data" / "opencode" / "opencode.db").is_file()

    models = [entry for entry in fixture.audit if entry["source"] == "model"]
    assert models and all(entry["authorized"] and entry["model"] == MODEL for entry in models)
    assert all(entry["profile"] for entry in models if not entry["title_request"])
    assert not any(entry["project_config"] for entry in models)
    assert not (work / "g0-project-plugin-canary").exists()
    assert not (work / "g0-project-mcp-canary").exists()
    assert not (work / "g0-write-canary").exists()
    assert any(entry["policy_denied"] for entry in models)
    assert sum(entry["echo_receipt"] for entry in models) == 4
    assert any("G0_ECHO_TIMEOUT" in entry["scenario"] and entry["tool_timeout"] for entry in models)
    assert sum(entry.get("delayed", False) for entry in fixture.audit) == 1
    assert any(entry["prior_turn"] and "G0_ECHO_RESUME" in entry["scenario"] for entry in models)
    print("PASS OpenCode handshake, policy, MCP, terminal outcomes and recovery", flush=True)


def mcode(args, fixture):
    state = args.reports / "state"
    work = args.reports / "work"
    state.mkdir()
    work.mkdir()
    url = f"http://{args.container_host}:{fixture.server_port}"
    executable = "/opt/g0/bin/mcode" if args.installation else "mcode"
    environment = {"MINIMAX_DATA_DIR": "/state", "CI": "1", "NODE_ENV": "production",
                   "NO_UPDATE_NOTIFIER": "1", "MAVIS_BUILD_ENV": "production"}
    with container(args, "mcode-offline", [executable, "acp"], state=state, work=work,
                   environment=environment, network="none") as process:
        wire = Wire(process, acp_respond)
        initialized = initialise(wire)
        dump(args.reports / "mcode-initialize.json", initialized)

    dump(state / "config.yaml", {
        "defaultModel": f"custom_provider:fixture/{MODEL}",
        "custom_provider": {"fixture": {"api": "openai-completions", "name": "Fixture",
                                         "options": {"baseURL": url + "/v1", "apiKey": TOKEN},
                                         "models": {MODEL: {"name": MODEL, "contextWindow": 32768,
                                                            "maxTokens": 4096}}}},
    })
    (state / "profile.md").write_text(PROFILE + "\n")
    (work / "AGENTS.md").write_text(PROJECT + "\n")
    subprocess.run(["git", "init", "--quiet", work], check=True, timeout=10)
    project = work / ".minimax"
    project.mkdir()
    project_hook = {"hooks": [{"type": "command", "command": "printf loaded > /work/g0-project-hook"}]}
    dump(project / "hooks.json", {"enabled": True, "hooks": {"SessionStart": [project_hook]}})
    project_mcp = {"mcpServers": {"project-canary": {"command": "sh", "args": [
        "-c", "printf loaded > /work/g0-project-mcp-canary",
    ]}}}
    dump(work / ".mcp.json", project_mcp)
    dump(project / "mcp.json", project_mcp)
    protected = {}
    if args.mask_project_config:
        empty_directory = args.reports / "empty-project-dir"
        empty_directory.mkdir()
        empty_file = args.reports / "empty-project-mcp.json"
        empty_file.touch()
        protected = {".minimax": empty_directory, ".mcp.json": empty_file}
    (state / "hook.mjs").write_text('''import fs from "node:fs";
let data = "";
for await (const chunk of process.stdin) data += chunk;
const request = JSON.parse(data);
fs.appendFileSync("/state/hook-observed", request.hook_event_name + "\\n");
if (request.hook_event_name === "PreToolUse" && /^(bash|shell)$/i.test(request.tool_name)) {
  console.log(JSON.stringify({hookSpecificOutput: {hookEventName: "PreToolUse",
    permissionDecision: "deny", permissionDecisionReason: "DEV_DESK_G0_POLICY_DENIED"}}));
}
''')
    handler = {"hooks": [{"type": "command", "command": "node /state/hook.mjs"}]}
    dump(state / "hooks.json", {"enabled": True, "hooks": {"SessionStart": [handler], "PreToolUse": [handler]}})
    servers = [{"type": "http", "name": "g0", "url": url + MCP_PATH,
                "headers": [{"name": "Authorization", "value": f"Bearer {fixture.token}"}]}]
    command = [executable, "acp", "--append-system-prompt-file", "/state/profile.md"]
    params = {"cwd": "/work", "mcpServers": servers}

    @contextmanager
    def start(label):
        with container(args, label, command, state=state, work=work, environment=environment,
                       protected=protected) as process:
            wire = Wire(process, acp_respond)
            initialise(wire)
            try:
                yield wire
            finally:
                dump(args.reports / f"{label}.json", redact_frames(wire.frames, fixture.token))

    def prompt(wire, scenario):
        return wire.send("session/prompt", {"sessionId": session, "prompt": [{"type": "text", "text": scenario}]})

    with start("mcode-live") as wire:
        session = wire.request("session/new", params)["sessionId"]
        for scenario in ("G0_ECHO_1", "G0_ECHO_2", "G0_ECHO_3", "G0_POLICY"):
            result = wire.reply(prompt(wire, scenario))
            assert result["result"]["stopReason"] == "end_turn", result
        error = wire.reply(prompt(wire, "G0_ERROR"))
        assert "error" in error, error
        cancelling = prompt(wire, "G0_CANCEL")
        wire.until(lambda row: "G0_STREAM_" in json.dumps(row))
        wire.send("session/cancel", {"sessionId": session}, notify=True)
        stopped = wire.reply(cancelling)
        fixture.release_cancel.set()
        assert stopped["result"]["stopReason"] == "cancelled", stopped

    capabilities = initialized["agentCapabilities"]
    resume = "session/resume" if "resume" in capabilities.get("sessionCapabilities", {}) else "session/load"
    with start("mcode-resume") as wire:
        wire.request(resume, {"sessionId": session, **params})
        result = wire.reply(prompt(wire, "G0_ECHO_RESUME"))
        assert result["result"]["stopReason"] == "end_turn", result

    models = [entry for entry in fixture.audit if entry["source"] == "model"]
    assert sum(entry["echo_receipt"] and entry["profile"] and entry["authorized"] for entry in models) == 4
    assert any(entry["policy_denied"] for entry in models), "mcode policy denial did not reach the model"
    assert {"SessionStart", "PreToolUse"} <= set((state / "hook-observed").read_text().splitlines())
    assert not (work / "g0-write-canary").exists()
    assert not (work / "g0-project-hook").exists()
    assert not (work / "g0-project-mcp-canary").exists()
    print("PASS mcode loopback MCP and policy canary", flush=True)


def redact_frames(frames, token):
    return json.loads(json.dumps(frames).replace(token, "[redacted]").replace(TOKEN, "[redacted]"))


def zcode(args, fixture):
    state = args.reports / "state"
    work = args.reports / "work"
    state.mkdir()
    work.mkdir()
    url = f"http://{args.container_host}:{fixture.server_port}"
    protected = {}
    if args.mask_project_config:
        project_server = {"type": "http", "url": url + PROJECT_MCP_PATH, "protocolVersion": "legacy"}
        project_config = {"mcp": {"servers": {"project-canary": project_server}}, "hooks": {
            "enabled": True, "events": {"SessionStart": [{"hooks": [{"type": "command",
                "command": "printf loaded > /work/g0-project-hook"}]}]},
        }}
        (work / ".zcode").mkdir()
        (work / ".agents").mkdir()
        dump(work / "zcode.json", project_config)
        dump(work / ".zcode/config.json", project_config)
        dump(work / ".agents/mcp.json", {"mcpServers": {"project-canary": project_server}})
        for leaf in (".zcode", ".agents"):
            replacement = args.reports / f"hidden-{leaf[1:]}"
            replacement.mkdir()
            protected[leaf] = replacement
        replacement = args.reports / "hidden-zcode.json"
        dump(replacement, {})
        protected["zcode.json"] = replacement
    executable = "/opt/g0/bin/zcode" if args.installation else "zcode"
    environment = {"ZCODE_APP_SERVER_AUTH_MODE": "host", "ZCODE_DISABLE_UPDATE_CHECK": "1",
                   "NO_UPDATE_NOTIFIER": "1", "ZCODE_CLI_SETTINGS_FILE": "/state/setting.json",
                   "ZCODE_PERSONAL_PROVIDER_CONFIG_FILE": "/state/provider_config.json",
                   "ZCODE_SESSION_DB_PATH": "/state/db.sqlite"}
    workspace = {"workspacePath": "/work", "workspaceKey": "devdesk-g0"}
    selection = {"providerId": "g0-fixture", "modelId": MODEL}
    model_selection = {**selection, "options": {"reasoningLevel": "disabled"}}
    account = "account:zai-individual-coding-plan"
    supply_account_auth = False

    def respond(frame):
        if frame["method"] == "session/requestRuntimePreferences":
            return {"result": {"nativeSearchEnhancementsEnabled": False}}
        if frame["method"] == "interaction/requestProviderRuntimeHeaders":
            if frame["params"].get("providerId") == account:
                if not supply_account_auth:
                    return {"result": {"headersApplied": False, "errorMessage": "DEVDESK_G0_ACCOUNT_CREDENTIAL_WITHHELD"}}
                return {"result": {"headersApplied": True, "requestAuth": {"apiKey": f"{TOKEN}.fixture-signing-secret"}}}
            return {"result": {"headersApplied": True, "requestAuth": {"apiKey": TOKEN}}}
        if frame["method"] == "interaction/requestPermission" and frame["params"]["toolName"] == f"mcp__g0__{TOOL}":
            option = next(option for option in frame["params"]["options"] if option["kind"] == "allow_once")
            return {"result": option["response"]}
        return {"error": {"code": -32601, "message": "Unsupported fixture request"}}

    @contextmanager
    def start(label, network=None):
        with container(args, label, [executable, "app-server"], state=state, work=work,
                       environment=environment, network=network, protected=protected) as process:
            wire = Wire(process, respond, jsonrpc=False)
            try:
                yield wire
            finally:
                dump(args.reports / f"{label}.json", redact_frames(wire.frames, fixture.token))

    offline_error = None
    try:
        with start("zcode-offline", network="none") as wire:
            wire.request("session/create", {"workspace": workspace, "titleGenerationEnabled": False},
                         timeout=ZCODE_STARTUP_TIMEOUT)
    except (RuntimeError, TimeoutError) as error:
        offline_error = error
        print(f"ZCode offline: {error}", flush=True)

    dump(state / "provider_config.json", {"schemaVersion": 1, "config": {
        "providerConfigRules": {"providerRules": [{"providerId": selection["providerId"],
            "providerName": "DevDesk G0", "enabled": True, "config": {
                "group": "standard-personal", "visibility": "visible", "personalModelIds": [MODEL],
                "modelOrder": [MODEL], "access": {"type": "api-key", "apiKey": TOKEN},
                "api": {"type": "openai-chat-completions", "baseUrl": url + "/v1", "headers": {}},
            }},
        ]},
        "modelConfigRules": {"providerModelRules": [], "manualProviderModelRules": [{**selection, "config": {
            "enabled": True, "properties": {
                "contextWindow": 32768, "inputFormat": {"supportsImage": False, "supportsVideo": False, "supportsPdf": False},
                "supportsJsonSchemaOutput": False, "supportsNativeWebSearch": False, "supportsMidConversationSystem": False,
            },
            "optionSpecs": {"maxOutputTokens": {"max": 4096}, "reasoningLevel": {"values": ["disabled"], "map": "{}"}},
        }}]},
        "providerOrder": [selection["providerId"]], "defaultModelSelection": model_selection,
    }})
    dump(state / "setting.json", {"mcp": {"servers": {"g0": {"type": "http", "url": url + MCP_PATH,
                                      "protocolVersion": "legacy",
                                      "headers": {"Authorization": f"Bearer {fixture.token}"}}}}})

    settings = json.loads((state / "setting.json").read_text())
    settings["permission"] = {"mode": "build", "allowedTools": ["Bash"]}
    settings["hooks"] = {"enabled": True, "events": {
        "SessionStart": [{"hooks": [{"type": "command", "command": "printf loaded > /state/zcode-policy-loaded",
                                    "onError": "deny"}]}],
        "PreToolUse": [{"matcher": "Bash", "hooks": [{"type": "command",
            "command": "printf called > /state/zcode-policy-called; printf DEV_DESK_G0_POLICY_DENIED >&2; exit 2",
            "onError": "deny"}]}],
    }}
    dump(state / "setting.json", settings)

    def submit(wire, scenario):
        accepted = wire.request("session/send", {"sessionId": session, "content": scenario,
            "modelSelection": model_selection, "modelExecution": {"selectionScope": "execution", "memoryExtraction": "skip"},
        })
        assert accepted["accepted"] is True, accepted

    def terminal(wire, timeout=30):
        return wire.until(lambda frame: frame.get("method") == "session/event" and frame["params"]["type"]
                          in ("turn.completed", "turn.failed"), timeout=timeout)["params"]

    with start("zcode-live") as wire:
        wire.request("provider/refreshRegistry", {"reason": "devdesk-g0"}, timeout=ZCODE_STARTUP_TIMEOUT)
        created = wire.request("session/create", {"workspace": workspace, "titleGenerationEnabled": False},
                               timeout=ZCODE_STARTUP_TIMEOUT)
        session = created["session"]["sessionId"]
        wire.request("session/subscribe", {"sessionId": session, "deliveryKind": "web-remote-replayable"})
        completed_turns = []
        for scenario in ("G0_ECHO_1", "G0_ECHO_2", "G0_ECHO_3", "G0_ERROR"):
            submit(wire, scenario)
            result = terminal(wire)
            expected = "turn.failed" if scenario == "G0_ERROR" else "turn.completed"
            assert result["type"] == expected, result
            if expected == "turn.completed":
                completed_turns.append(result["turnId"])
        receipts = [event["frame"]["params"] for event in wire.frames
                    if event["frame"].get("method") == "session/event"
                    and event["frame"]["params"]["type"] == "tool.updated"
                    and event["frame"]["params"]["payload"].get("result", {}).get("content") == "DEV_DESK_G0_MCP_RECEIPT"]
        assert [receipt["turnId"] for receipt in receipts] == completed_turns
        submit(wire, "G0_POLICY")
        terminal(wire)
        assert (state / "zcode-policy-loaded").read_text() == "loaded"
        assert (state / "zcode-policy-called").read_text() == "called"
        assert not (work / "g0-write-canary").exists()
        assert any(entry.get("policy_denied") for entry in fixture.audit)
        submit(wire, "G0_CANCEL")
        wire.until(lambda frame: frame.get("method") == "session/event" and "G0_STREAM_" in json.dumps(frame))
        wire.request("session/stop", {"sessionId": session})
        stopped = terminal(wire)
        fixture.release_cancel.set()
        assert stopped["type"] == "turn.completed" and stopped["payload"]["resultType"] == "cancelled", stopped

    with start("zcode-resume") as wire:
        resumed = wire.request("session/resume", {"sessionId": session, "workspace": workspace},
                               timeout=ZCODE_STARTUP_TIMEOUT)
        assert resumed["session"]["sessionId"] == session
        wire.request("session/subscribe", {"sessionId": session, "deliveryKind": "web-remote-replayable"})
        submit(wire, "G0_ECHO_RESUME")
        resumed_turn = terminal(wire)
        assert resumed_turn["payload"]["resultType"] == "success", resumed_turn
        completed_turns.append(resumed_turn["turnId"])

    assert [entry["provider_turn"] for entry in fixture.audit if entry.get("method") == "tools/call"] == completed_turns
    assert sum(entry.get("echo_receipt", False) for entry in fixture.audit) == 4, "ZCode did not receive the MCP receipts"
    if protected:
        assert not (work / "g0-project-hook").exists(), "ZCode loaded a project hook"
        assert not any(entry["source"] == "project-mcp" for entry in fixture.audit), "ZCode contacted repository MCP"
        assert not any("project-canary" in name for entry in fixture.audit for name in entry.get("tools", [])), (
            "ZCode exposed a repository MCP tool")
        assert json.loads((work / "zcode.json").read_text()) == project_config
        assert json.loads((work / ".zcode/config.json").read_text()) == project_config
        print("PASS ZCode project settings, hooks and MCP were isolated by the read-only masks", flush=True)
    if offline_error is not None:
        raise offline_error
    print("PASS ZCode selected the fixture model and reached loopback MCP", flush=True)
    if args.zcode_account_probe:
        # The fake credential must never reach the catalog's external endpoint.
        with start("zcode-account", network="none") as wire:
            update = wire.request("provider/updateAccountConfig", {
                "revision": "devdesk-g0-account",
                "basedOnZCodeBuiltinRevision": "zcode-builtin:44:54d884c4e9d2d321d5bc6f0c20836b994418dd25c21425fe706755cd17da77e2",
                "providers": {account: {"builtinModelIds": ["GLM-5.3"], "access": {
                    "type": "zhipu-account", "entitled": True}}},
                "states": {account: {"availability": "available", "entitled": True, "current": True}},
            }, timeout=ZCODE_STARTUP_TIMEOUT)
            assert update["providerCount"] == 1 and update["receivedRevision"] == "devdesk-g0-account", update
            created = wire.request("session/create", {"workspace": workspace, "titleGenerationEnabled": False},
                                   timeout=ZCODE_STARTUP_TIMEOUT)
            account_session = created["session"]["sessionId"]
            wire.request("session/subscribe", {"sessionId": account_session, "deliveryKind": "web-remote-replayable"})
            for supply_account_auth in (False, True):
                frame_offset = len(wire.frames)
                accepted = wire.request("session/send", {"sessionId": account_session, "content": "G0_ACCOUNT_CALLBACK",
                    "modelSelection": {"providerId": account, "modelId": "GLM-5.3",
                                       "options": {"reasoningLevel": "high"}},
                    "modelExecution": {"selectionScope": "execution", "memoryExtraction": "skip"},
                })
                assert accepted["accepted"] is True, accepted
                failure = terminal(wire, timeout=120 if supply_account_auth else 30)
                callbacks = [record["frame"]["params"] for record in wire.frames[frame_offset:]
                             if record["frame"].get("method") == "interaction/requestProviderRuntimeHeaders"]
                assert callbacks and (supply_account_auth or len(callbacks) == 1), (callbacks, failure)
                for callback in callbacks:
                    assert callback["providerId"] == account and callback["reason"] == "model-request"
                    assert callback["sessionId"] == account_session and callback["turnId"] == failure["turnId"]
                assert failure["type"] == "turn.failed", failure
                error = failure["payload"]["error"]
                if supply_account_auth:
                    assert failure["payload"]["turnPhase"] == "processing_input", failure
                    assert error["attribution"]["providerId"] == account, failure
                    assert error["attribution"]["source"] == "network", failure
                else:
                    assert "DEVDESK_G0_ACCOUNT_CREDENTIAL_WITHHELD" in json.dumps(error), failure
        print("PASS ZCode host credential callback correlation, withholding and offline request with fixture auth", flush=True)


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--provider", choices=("opencode", "mcode", "zcode"), default="opencode")
    parser.add_argument("--image", required=True, help="locally prepared, pinned Harness or Node image")
    parser.add_argument("--executable", type=lambda value: Path(value).resolve(),
                        help="bind-mount a verified Linux OpenCode binary instead of installing it in the image")
    parser.add_argument("--installation", type=lambda value: Path(value).resolve(),
                        help="bind-mount a private package installation instead of baking it into the image")
    parser.add_argument("--reports", type=lambda value: Path(value).resolve(), required=True)
    parser.add_argument("--listen", default="127.0.0.1", help="host loopback or the local Docker bridge gateway")
    parser.add_argument("--container-host", default="host.docker.internal")
    parser.add_argument("--zcode-account-probe", action="store_true",
                        help="test the pinned host-auth account callback with credentials withheld and no network")
    parser.add_argument("--mask-project-config", action="store_true",
                        help="mask mcode or ZCode project config with protected read-only mounts")
    args = parser.parse_args()
    args.reports.mkdir(parents=True, exist_ok=False)
    fixture = ThreadingHTTPServer((args.listen, 0), Fixture)
    fixture.token = secrets.token_urlsafe(32)
    fixture.release_cancel = Event()
    fixture.release_tool = Event()
    fixture.audit = []
    Thread(target=fixture.serve_forever, daemon=True).start()
    try:
        {"opencode": open_code, "mcode": mcode, "zcode": zcode}[args.provider](args, fixture)
    finally:
        fixture.release_cancel.set()
        fixture.release_tool.set()
        fixture.shutdown()
        fixture.server_close()
        dump(args.reports / "http-audit.json", fixture.audit)


if __name__ == "__main__":
    main()

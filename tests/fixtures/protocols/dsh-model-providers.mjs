// Runs inside --network=none. All inference responses are fixtures from this process's loopback server.
import assert from "node:assert/strict";
import { createServer } from "node:http";
import { spawn } from "node:child_process";
import { mkdir, writeFile } from "node:fs/promises";
import { createInterface } from "node:readline";

let input = "";
for await (const chunk of process.stdin) input += chunk;
const cases = JSON.parse(input);
const reports = [];
for (const test of cases) {
  let child;
  const deadline = setTimeout(() => { child?.kill("SIGKILL"); process.exit(2); }, 45000);
  let calls = 0, toolResults = 0, replayedThinking = false;
  const marker = `MOCK_${test.provider}`;
  const server = createServer(async (request, response) => {
    try {
      let raw = "";
      for await (const chunk of request) raw += chunk;
      const body = JSON.parse(raw);
      assert.equal(body.model, "devdesk-mock-model");
      assert.equal(body.max_tokens ?? body.max_completion_tokens, 2048);
      const anthropic = test.provider === "minimax-cn";
      assert.equal(new URL(request.url, "http://fixture").pathname, anthropic ? "/anthropic/v1/messages" : "/v1/chat/completions");
      assert.equal(anthropic ? request.headers["x-api-key"] : request.headers.authorization, anthropic ? "FAKE_DSH_TEST_VALUE" : `Bearer ${test.provider === "ollama" ? "ollama" : "FAKE_DSH_TEST_VALUE"}`);
      calls++;
      const history = JSON.stringify(body.messages);
      if (test.task_payload) {
        assert(history.includes(JSON.stringify(test.task_payload).slice(1, -1)), "frozen task and Spec must reach the native model request verbatim");
        const strings = value => typeof value === "string" ? [value] : value && typeof value === "object" ? Object.values(value).flatMap(strings) : [];
        assert(strings(body).some(text => text.includes(test.persona)), "frozen Role/Skill persona must reach the native system request verbatim");
      }
      if (history.includes("tool_result") || body.messages.some(m => m.role === "tool")) {
        toolResults++;
        assert(history.includes(marker), "native tool result must reach the model");
      }
      if (anthropic && history.includes("mock thinking")) replayedThinking = true;
      const tool = anthropic ? body.tools.find(t => t.name === "bash") : body.tools.find(t => t.function?.name === "bash")?.function;
      assert(tool, "native bash tool is present");
      const args = JSON.stringify({ command: `printf '${marker}'`, description: "Print offline fixture marker" });
      response.writeHead(200, { "Content-Type": "text/event-stream" });
      const send = (name, value) => response.write(`${name ? `event: ${name}\n` : ""}data: ${JSON.stringify(value)}\n\n`);
      if (anthropic) {
        send("message_start", { type: "message_start", message: { id: `msg_${calls}`, type: "message", role: "assistant", model: body.model, content: [], stop_reason: null, usage: { input_tokens: 10, output_tokens: 0 } } });
        if (calls === 1) {
          send("content_block_start", { type: "content_block_start", index: 0, content_block: { type: "thinking", thinking: "" } });
          send("content_block_delta", { type: "content_block_delta", index: 0, delta: { type: "thinking_delta", thinking: "mock thinking" } });
          send("content_block_delta", { type: "content_block_delta", index: 0, delta: { type: "signature_delta", signature: "mock-signature" } });
          send("content_block_stop", { type: "content_block_stop", index: 0 });
          send("content_block_start", { type: "content_block_start", index: 1, content_block: { type: "tool_use", id: "call_mock", name: tool.name, input: {} } });
          send("content_block_delta", { type: "content_block_delta", index: 1, delta: { type: "input_json_delta", partial_json: args } });
          send("content_block_stop", { type: "content_block_stop", index: 1 });
        } else {
          send("content_block_start", { type: "content_block_start", index: 0, content_block: { type: "text", text: "" } });
          send("content_block_delta", { type: "content_block_delta", index: 0, delta: { type: "text_delta", text: marker } });
          send("content_block_stop", { type: "content_block_stop", index: 0 });
        }
        send("message_delta", { type: "message_delta", delta: { stop_reason: calls === 1 ? "tool_use" : "end_turn", stop_sequence: null }, usage: { output_tokens: 20 } });
        send("message_stop", { type: "message_stop" });
      } else {
        const chunk = (delta, finish_reason = null) => ({ id: `chat_${calls}`, object: "chat.completion.chunk", created: 1, model: body.model, choices: [{ index: 0, delta, finish_reason }] });
        send(null, chunk({ role: "assistant", ...(calls === 1 ? { tool_calls: [{ index: 0, id: "call_mock", type: "function", function: { name: tool.name, arguments: args } }] } : { content: marker }) }));
        send(null, chunk({}, calls === 1 ? "tool_calls" : "stop"));
        response.write("data: [DONE]\n\n");
      }
      response.end();
    } catch (error) {
      // No request headers or bodies are included in diagnostics.
      console.error("mock assertion:", error.message);
      response.writeHead(500).end();
    }
  });
  await new Promise(resolve => server.listen(0, "127.0.0.1", resolve));
  const home = `/tmp/devdesk-provider-${test.provider}`;
  await mkdir(home, { recursive: true });
  const endpoint = `http://127.0.0.1:${server.address().port}${test.provider === "minimax-cn" ? "/anthropic" : "/v1"}`;
  const patch = test.patch.replaceAll(test.endpoint, endpoint) + "\n- id: sandbox-policy\n  config:\n    mode: danger-full-access\n    workspaceRoot: /tmp\n- id: approval\n  config:\n    policy: never\n- id: session-telemetry-otel\n  disabled: true\n- id: session-log-deepseek\n  disabled: true\n- id: agent-instructions\n  disabled: true\n";
  await writeFile(`${home}/patch.yaml`, patch + (test.persona ? `\n- id: system-prompt\n  config:\n    personaPrefix: ${JSON.stringify(test.persona)}\n` : ""));
  let serial = 0, pending = new Map(), events = [];
  const launch = () => {
    child = spawn("dsh", ["--profile", "sdk", "--patch", `${home}/patch.yaml`], { env: { ...process.env, DSH_HOME: home, [test.env]: test.provider === "ollama" ? "ollama" : "FAKE_DSH_TEST_VALUE" }, stdio: ["pipe", "pipe", "pipe"] });
    child.stderr.resume();
    const lines = createInterface({ input: child.stdout });
    lines.on("line", line => {
      const frame = JSON.parse(line);
      if (frame.id !== undefined && pending.has(frame.id)) {
        const request = pending.get(frame.id); pending.delete(frame.id);
        if (frame.error) request.reject(Error(JSON.stringify(frame.error))); else request.resolve(frame.result);
      } else events.push(frame);
    });
    child.on("exit", code => { for (const request of pending.values()) request.reject(Error(`DSH exited ${code}`)); pending.clear(); });
  };
  const rpc = (method, params) => new Promise((resolve, reject) => {
    const id = ++serial; pending.set(id, { resolve, reject });
    child.stdin.write(`${JSON.stringify({ jsonrpc: "2.0", id, method, params })}\n`);
  });
  const initialize = () => rpc("initialize", { cwd: "/tmp", provider: test.provider, model: "devdesk-mock-model", maxTokens: 2048, approvalRequests: true, capabilities: { userQuestions: true } });
  const session = `mock-${test.provider}`;
  try {
    launch(); await initialize();
    await rpc("session/prompt", { sessionId: session, contentBlocks: [{ type: "text", text: test.task_payload || "Run the fixture tool, then print its marker." }] });
    await rpc("session/wait", { sessionId: session });
    const ends = events.filter(e => e.method === "session.event" && e.params.event.type === "turn/end");
    if (ends.at(-1)?.params.event.data.reason.kind !== "completed") console.error(test.provider, "calls", calls, JSON.stringify(ends.at(-1)?.params.event.data).replaceAll("FAKE_DSH_TEST_VALUE", "[redacted]"));
    assert.equal(ends.at(-1)?.params.event.data.reason.kind, "completed");
    assert.equal(calls, 2); assert.equal(toolResults, 1);
    if (test.provider === "minimax-cn") assert(replayedThinking, "thinking must be replayed with tool results");
    const exited = new Promise(resolve => child.once("exit", resolve));
    await rpc("shutdown", {}); child.stdin.end(); await exited;
    events = []; launch(); await initialize(); await rpc("session/resume", { sessionId: session });
    await rpc("session/prompt", { sessionId: session, contentBlocks: [{ type: "text", text: "Reply with the previous fixture marker." }] });
    await rpc("session/wait", { sessionId: session });
    assert.equal(calls, 3); assert.equal(toolResults, 2);
    const resumedEnds = events.filter(e => e.method === "session.event" && e.params.event.type === "turn/end");
    assert.equal(resumedEnds.at(-1)?.params.event.data.reason.kind, "completed");
    await rpc("shutdown", {}); child.stdin.end();
    reports.push({ provider: test.provider, calls, tool_result_consumed: true, native_resume: true, frozen_task_delivered: !!test.task_payload, thinking_replayed: test.provider === "minimax-cn" ? replayedThinking : null });
  } finally {
    child?.kill("SIGTERM"); server.close(); clearTimeout(deadline);
  }
}
console.log(JSON.stringify({ network: "none", inference: "mock_only", reports }));

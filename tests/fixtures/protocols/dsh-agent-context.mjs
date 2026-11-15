// Real DSH SDK and tool registry, with deterministic loopback-only inference.
import assert from "node:assert/strict";
import { createServer } from "node:http";
import { spawn } from "node:child_process";
import { mkdir, writeFile } from "node:fs/promises";
import { createInterface } from "node:readline";

const directory = "/tmp/devdesk-agent-context-native";
await mkdir(directory, { recursive: true });
const names = ["devdesk_agent_experience_list", "devdesk_agent_experience_read"];
let inferenceCalls = 0;
let contextCalls = 0;
let child;
let diagnostics = "";
const deadline = setTimeout(() => { child?.kill("SIGKILL"); process.exit(2); }, 45000);
const server = createServer(async (request, response) => {
  let body = "";
  for await (const chunk of request) body += chunk;
  const payload = JSON.parse(body);
  inferenceCalls += 1;
  for (const name of names) assert(payload.tools.some((tool) => tool.function?.name === name));
  if (inferenceCalls > 1) assert(JSON.stringify(payload.messages).includes("historical-feature"));
  const delta = inferenceCalls < 3 ? { role: "assistant", tool_calls: [{ index: 0, id: `context_${inferenceCalls}`, type: "function", function: {
    name: names[inferenceCalls - 1], arguments: JSON.stringify(inferenceCalls === 1 ? {} : { source_project_id: "source-project", snapshot_id: "historical-feature" }),
  } }] } : { role: "assistant", content: "Read historical-feature without an active project." };
  response.writeHead(200, { "Content-Type": "text/event-stream" });
  const chunk = (delta, finish_reason) => ({ id: `chat_${inferenceCalls}`, object: "chat.completion.chunk", created: 1, model: "devdesk-context-fixture", choices: [{ index: 0, delta, finish_reason }] });
  response.write(`data: ${JSON.stringify(chunk(delta, null))}\n\n`);
  response.write(`data: ${JSON.stringify(chunk({}, inferenceCalls < 3 ? "tool_calls" : "stop"))}\n\n`);
  response.end("data: [DONE]\n\n");
});
await new Promise((resolve) => server.listen(0, "127.0.0.1", resolve));
const endpoint = `http://127.0.0.1:${server.address().port}/v1`;
const dynamicTools = names.map((name, index) => ({ name, description: "Read this Agent's historical experience.", inputSchema: index === 0 ? { type: "object", properties: {}, additionalProperties: false } : {
  type: "object", properties: { source_project_id: { type: "string" }, snapshot_id: { type: "string" } }, required: ["source_project_id", "snapshot_id"], additionalProperties: false,
} }));
const providers = { providers: { ollama: { api: "openai-completions", baseURL: endpoint, headers: { Authorization: "Bearer ollama" }, compat: { supportsStore: false, supportsDeveloperRole: false, maxTokensField: "max_tokens", supportsStrictMode: false }, models: [{ id: "devdesk-context-fixture", contextWindow: 32768, maxTokens: 2048, reasoningEfforts: false }] } } };
const patch = `
- id: llm-deepseek
  disabled: true
- id: llm-deepseek-account
  disabled: true
- id: llm-pi-ai
  disabled: false
  config: ${JSON.stringify(providers)}
- id: sdk-jsonrpc-server
  disabled: true
- id: session-telemetry-otel
  disabled: true
- id: session-log-deepseek
  disabled: true
- id: sandbox-policy
  config:
    mode: danger-full-access
    workspaceRoot: /tmp
- id: approval
  config:
    policy: never
- insert:
  - id: devdesk-tool-boundary
    name: /run/devdesk/tool-policy.mjs
    config:
      contextBridge: true
      dynamicTools: ${JSON.stringify(dynamicTools)}
`;
await writeFile(`${directory}/patch.yaml`, patch);
let serial = 0;
const pending = new Map();
const events = [];
child = spawn("dsh", ["--profile", "sdk", "--patch", `${directory}/patch.yaml`], { env: { ...process.env, DSH_HOME: directory }, stdio: ["pipe", "pipe", "pipe"] });
child.stderr.on("data", (chunk) => { diagnostics = (diagnostics + chunk).slice(-8000); });
createInterface({ input: child.stdout }).on("line", (line) => {
  const frame = JSON.parse(line);
  if (frame.method === "devdesk/tool/call") {
    assert.equal(frame.params.sessionId, "detached-experience");
    assert.equal(frame.params.tool, names[contextCalls]);
    contextCalls += 1;
    const value = contextCalls === 1 ? { records: [{ source_project_id: "source-project", snapshot_id: "historical-feature" }], total: 1 } : { source: "historical-feature", content: "Past feature requirements." };
    child.stdin.write(`${JSON.stringify({ jsonrpc: "2.0", id: frame.id, result: { success: true, structured_content: value } })}\n`);
  } else if (frame.id !== undefined && pending.has(frame.id)) {
    const { resolve, reject } = pending.get(frame.id); pending.delete(frame.id);
    if (frame.error) reject(Error(JSON.stringify(frame.error))); else resolve(frame.result);
  } else events.push(frame);
});
child.once("exit", (code) => { for (const request of pending.values()) request.reject(Error(`DSH exited ${code}: ${diagnostics}`)); });
const rpc = (method, params) => new Promise((resolve, reject) => {
  const id = ++serial; pending.set(id, { resolve, reject });
  child.stdin.write(`${JSON.stringify({ jsonrpc: "2.0", id, method, params })}\n`);
});
try {
  await rpc("initialize", { cwd: "/tmp", provider: "ollama", model: "devdesk-context-fixture", maxTokens: 2048 });
  await rpc("session/prompt", { sessionId: "detached-experience", contentBlocks: [{ type: "text", text: "Read my historical feature experience." }] });
  await rpc("session/wait", { sessionId: "detached-experience" });
  assert.equal(contextCalls, 2);
  assert.equal(inferenceCalls, 3);
  assert.equal(events.filter((frame) => frame.method === "session.event" && frame.params.event.type === "turn/end").at(-1).params.event.data.reason.kind, "completed");
  await rpc("shutdown", {});
  console.log(JSON.stringify({ sdk: "real", inference: "mock_only", network: "none", contextCalls, inferenceCalls, detached_history_read: true }));
} finally {
  child.kill("SIGTERM"); server.close(); clearTimeout(deadline);
}

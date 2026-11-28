// ACP compatibility for the pinned ZCode app-server. The host owns the turn and MCP listener.
import { spawn } from 'node:child_process';
import { randomUUID } from 'node:crypto';
import { mkdtempSync, readFileSync, rmSync, writeFileSync } from 'node:fs';
import { tmpdir } from 'node:os';
import { join } from 'node:path';
import { createInterface } from 'node:readline';

const probe = process.argv.includes('--probe');
const config = probe ? null : JSON.parse(readFileSync(process.argv[2], 'utf8'));
const instructions = probe ? '' : readFileSync(process.argv[3], 'utf8');
const marker = process.argv[4];
const scratch = mkdtempSync(join(tmpdir(), 'devdesk-zcode-'));
const environment = {
  ...process.env,
  ZCODE_DISABLE_UPDATE_CHECK: '1',
  NO_UPDATE_NOTIFIER: '1',
  ZCODE_APP_SERVER_AUTH_MODE: 'host',
};
if (config) {
  const key = environment[config.credential];
  if (!key) throw Error('Frozen ZCode credential is unavailable');
  delete environment[config.credential];
  config.providers.config.providerConfigRules.providerRules[0].config.access.apiKey = key;
  const file = join(scratch, 'provider-config.json');
  writeFileSync(file, JSON.stringify(config.providers), { mode: 0o600 });
  environment.ZCODE_PERSONAL_PROVIDER_CONFIG_FILE = file;
}
const child = spawn('zcode', ['app-server'], { env: environment, stdio: ['pipe', 'pipe', 'ignore'] });
const pending = new Map();
let counter = 0;
let session;
let active;
let sequence = 0;
let finishing = false;

function close(code) {
  if (finishing) return;
  finishing = true;
  child.kill();
  rmSync(scratch, { recursive: true, force: true });
  process.exitCode = code;
  process.stdin.destroy();
}
process.on('exit', () => rmSync(scratch, { recursive: true, force: true }));
process.on('SIGTERM', () => close(143));
process.on('SIGINT', () => close(130));
child.on('error', () => close(1));
child.on('exit', () => close(1));
child.stdin.on('error', () => close(1));

function send(frame) {
  child.stdin.write(JSON.stringify(frame) + '\n');
}
function request(method, params = {}) {
  return new Promise((resolve, reject) => {
    const id = ++counter;
    pending.set(id, { resolve, reject });
    send({ id, method, params });
  });
}
function publish(frame) {
  process.stdout.write(JSON.stringify({ jsonrpc: '2.0', ...frame }) + '\n');
}
function notify(update) {
  publish({ method: 'session/update', params: { sessionId: session, update } });
}
function policyLoaded() {
  try { return readFileSync(marker, 'utf8') === 'loaded'; } catch { return false; }
}
function answerRequest({ id, method, params }) {
  if (method === 'session/requestRuntimePreferences') {
    send({ id, result: { nativeSearchEnhancementsEnabled: false } });
    return;
  }
  if (method === 'interaction/requestPermission') {
    const allowed = policyLoaded() && params.sessionId === session && active?.turn && params.turnId === active.turn
      && params.toolName?.startsWith('mcp__devdesk__');
    const option = allowed && params.options?.find((candidate) => candidate.kind === 'allow_once');
    if (option) {
      send({ id, result: option.response });
      return;
    }
  }
  if (method === 'interaction/requestProviderRuntimeHeaders') {
    send({ id, result: { headersApplied: false, errorMessage: 'Provider route is not bound to a ZCode account' } });
    return;
  }
  send({ id, error: { code: -32601, message: 'Request denied by the frozen DevDesk route' } });
}
function onEvent(event) {
  if (event.sessionId !== session || !Number.isSafeInteger(event.seq) || event.seq <= sequence) return;
  sequence = event.seq;
  if (!active) return;
  const payload = event.payload ?? {};
  if (event.type === 'turn.started' && payload.inputId === active.input && typeof event.turnId === 'string' && !active.turn) {
    active.turn = event.turnId;
    return;
  }
  if (!active.turn || event.turnId !== active.turn) return;
  if (event.type === 'tool.updated' && typeof payload.toolCallId === 'string'
      && typeof payload.toolName === 'string') {
    const existing = active.tools.has(payload.toolCallId);
    active.tools.add(payload.toolCallId);
    notify({ sessionUpdate: existing ? 'tool_call_update' : 'tool_call', toolCallId: payload.toolCallId,
      title: payload.toolName, status: 'in_progress' });
    return;
  }
  if (!['turn.completed', 'turn.failed'].includes(event.type) || payload.inputId !== active.input) return;
  const { id } = active;
  active = null;
  if (event.type === 'turn.completed' && payload.resultType === 'success' && typeof payload.response === 'string' && policyLoaded()) {
    if (payload.response) notify({ sessionUpdate: 'agent_message_chunk', content: { type: 'text', text: payload.response } });
    publish({ id, result: { stopReason: 'end_turn' } });
  } else if (event.type === 'turn.completed' && payload.resultType === 'cancelled') {
    publish({ id, result: { stopReason: 'cancelled' } });
  } else {
    publish({ id, error: { code: -32000, message: 'ZCode turn did not complete successfully' } });
  }
}
createInterface({ input: child.stdout }).on('line', (line) => {
  let frame;
  try { frame = JSON.parse(line); } catch { close(1); return; }
  if (frame.method === 'session/event') onEvent(frame.params);
  else if (frame.method && frame.id !== undefined) answerRequest(frame);
  else if (pending.has(frame.id)) {
    const callback = pending.get(frame.id);
    pending.delete(frame.id);
    if (frame.error) callback.reject(Error('ZCode rejected the native request'));
    else callback.resolve(frame.result);
  }
});

let initializing;
async function dispatch({ id, method, params = {} }) {
  if (method === 'initialize') {
    if (probe) await request('session/create', { workspace: { workspacePath: '/tmp', workspaceKey: 'devdesk-probe' }, titleGenerationEnabled: false });
    return { protocolVersion: 1, agentCapabilities: { loadSession: true, mcpCapabilities: { http: true } } };
  }
  if (method === 'session/new' || method === 'session/load') {
    if (initializing || session || params.cwd !== '/work' || params.mcpServers?.length) throw Error('Invalid session');
    initializing = true;
    const workspace = { workspacePath: '/work', workspaceKey: 'devdesk' };
    await request('provider/refreshRegistry', { reason: 'devdesk' });
    const response = method === 'session/load'
      ? await request('session/resume', { sessionId: params.sessionId, workspace })
      : await request('session/create', { workspace, titleGenerationEnabled: false });
    session = response.session?.sessionId;
    if (!session || (method === 'session/load' && session !== params.sessionId)) throw Error('Invalid session identity');
    await request('session/subscribe', { sessionId: session, deliveryKind: 'web-remote-replayable' });
    return { sessionId: session, models: { currentModelId: config.model } };
  }
  if (!session || params.sessionId !== session) throw Error('Unexpected session');
  if (method === 'session/cancel') {
    if (active) await request('session/stop', { sessionId: session });
    return;
  }
  if (method === 'session/prompt' && !active && Array.isArray(params.prompt)
      && params.prompt.every((content) => content.type === 'text' && typeof content.text === 'string')) {
    const content = instructions + '\n\n' + params.prompt.map((part) => part.text).join('\n');
    const input = randomUUID();
    active = { id, input, tools: new Set() };
    const response = await request('session/send', {
      sessionId: session, content, inputId: input,
      modelSelection: config.providers.config.defaultModelSelection,
      modelExecution: { selectionScope: 'execution', memoryExtraction: 'skip' },
    });
    if (response?.accepted !== true || response.sessionId !== session) {
      active = null;
      throw Error('Input not accepted');
    }
    return undefined;
  }
  throw Error('Unsupported request');
}

createInterface({ input: process.stdin }).on('line', async (line) => {
  let frame;
  try { frame = JSON.parse(line); } catch { close(1); return; }
  try {
    if (frame.jsonrpc !== '2.0') throw Error('Invalid ACP version');
    const result = await dispatch(frame);
    if (frame.id !== undefined && result !== undefined) publish({ id: frame.id, result });
  } catch {
    if (frame.id !== undefined) publish({ id: frame.id, error: { code: -32000, message: 'Invalid or rejected ZCode operation' } });
  }
}).on('close', () => close(0));

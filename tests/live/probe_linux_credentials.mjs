// Actual Linux Secret Service, isolated synthetic entries, browser settings and Host restart.
// No model credentials are used. Secret values never enter argv or the report.
import { chromium } from '../../web/node_modules/playwright/index.mjs';
import { spawn, spawnSync } from 'node:child_process';
import { createHash, randomBytes } from 'node:crypto';
import { once } from 'node:events';
import fs from 'node:fs';
import os from 'node:os';
import path from 'node:path';
import readline from 'node:readline';

const args = process.argv.slice(2);
const option = (name, fallback) => {
  const index = args.indexOf(name);
  return index < 0 ? fallback : args[index + 1];
};
const output = option('--output');
if (!output || process.platform !== 'linux') throw new Error('Linux and --output are required');
const binary = path.resolve(option('--binary', 'target/debug/devdesk-host'));
const root = fs.mkdtempSync(path.join(os.tmpdir(), 'devdesk-linux-credentials-'));
const dataset = path.join(root, 'data');
const service = 'devdesk.' + createHash('sha256').update(dataset).digest('hex');
const secrets = [randomBytes(24).toString('hex'), randomBytes(24).toString('hex')];
const references = [];
const report = {
  passed: false, source: 'actual Chromium → Host → Linux Secret Service',
  synthetic_entries_only: true, collection_lock_tested: false,
  unavailable_backend_test: 'dedicated Host with unreachable session D-Bus',
  binary, dataset, hosts: [], checks: [],
};
const check = (value, name) => {
  if (!value) throw new Error(name);
  report.checks.push(name);
};
let host, browser, page, url;
const start = async (unavailable = false) => {
  const argv = ['--data', dataset, '--port', '0'];
  host = spawn(binary, argv, {
    cwd: root, env: { ...process.env, ...(unavailable ? {
      DBUS_SESSION_BUS_ADDRESS: 'unix:path=' + path.join(root, 'absent-session-bus'),
    } : {}) }, stdio: ['ignore', 'pipe', 'pipe'],
  });
  host.stderr.resume();
  report.hosts.push({ pid: host.pid, argv: [binary, ...argv], unavailable_backend: unavailable });
  const lines = readline.createInterface({ input: host.stdout });
  const ready = (async () => {
    for await (const line of lines) if (line.startsWith('http://127.0.0.1:')) return line;
    throw new Error('host_start_failed');
  })();
  let timeout;
  try {
    url = await Promise.race([ready, new Promise((_, reject) => {
      timeout = setTimeout(() => reject(new Error('host_start_timeout')), 15000);
    })]);
  } finally { clearTimeout(timeout); lines.close(); host.stdout.resume(); }
  page = await browser.newPage();
  await page.goto(url);
};
const rpc = async (method, params = {}, allowError = false) => {
  const response = await page.request.post(url + '/rpc', {
    data: { jsonrpc: '2.0', id: 1, method, params },
  });
  const body = await response.json();
  check(!secrets.some(secret => JSON.stringify(body).includes(secret)), 'rpc_response_contains_no_secret');
  if (body.error && !allowError) throw new Error(method + ':' + body.error.data.code);
  return allowError ? body : body.result;
};
const stop = async () => {
  if (!host || host.exitCode !== null) return;
  const exited = once(host, 'exit');
  await page.request.post(url + '/shutdown', { data: {} });
  const [code] = await exited;
  check(code === 0, 'owned_host_gracefully_stopped');
  await page.close();
};
const tool = (operation, account) => spawnSync('secret-tool', [operation, 'service', service, 'account', account], { timeout: 20000 });
const account = reference => {
  check(reference.startsWith(service + ':'), 'entry_belongs_to_probe_dataset');
  return reference.slice(service.length + 1);
};

try {
  browser = await chromium.launch({ headless: true });
  await start();
  check(!(await rpc('credentials.status')).configured, 'missing_key_status');
  check(Boolean((await rpc('credentials.check', {}, true)).error), 'missing_key_read_rejected');
  const project = await rpc('project.create', {
    command_id: randomBytes(16).toString('hex'), payload: { title: 'Linux credential probe' },
  });
  report.project_id = project.id;
  await page.getByRole('button', { name: '设置', exact: true }).click();
  for (const secret of secrets) {
    const password = page.getByLabel('配置或更换 API key');
    await password.fill(secret);
    const saved = page.waitForResponse(response => response.url() === url + '/rpc'
      && response.request().postDataJSON()?.method === 'credentials.configure');
    await page.getByRole('button', { name: '保存密钥', exact: true }).click();
    const body = await (await saved).json();
    check(!body.error, 'browser_key_saved');
    references.push(body.result.reference);
    await page.waitForFunction(() => document.querySelector('#deepseek-api-key')?.value === '');
    check((await password.inputValue()) === '', 'password_field_cleared');
    await rpc('credentials.check');
    const actual = tool('lookup', account(body.result.reference));
    check(actual.status === 0 && actual.stdout.toString().trimEnd() === secret, 'actual_secret_service_value_matches');
  }
  check(references[0] !== references[1], 'updates_create_new_opaque_reference');
  check(tool('lookup', account(references[0])).stdout.toString().trimEnd() === secrets[0], 'old_frozen_reference_preserved');
  const storage = await page.evaluate(() => JSON.stringify([Object.entries(localStorage), Object.entries(sessionStorage)]));
  check(!secrets.some(secret => storage.includes(secret)), 'browser_storage_contains_no_secret');
  for (const name of fs.readdirSync(dataset).filter(name => name.startsWith('global.sqlite3'))) {
    const bytes = fs.readFileSync(path.join(dataset, name));
    check(!secrets.some(secret => bytes.includes(secret)), 'sqlite_contains_no_secret');
  }
  await stop();
  await start();
  check((await rpc('credentials.status')).reference === references[1], 'reference_survives_restart');
  check((await rpc('query.workspace')).projects.some(value => value.id === project.id), 'existing_project_survives_restart');
  await rpc('credentials.check');
  report.checks.push('secret_read_survives_restart');
  await stop();
  await start(true);
  const started = Date.now();
  check((await rpc('credentials.status')).configured, 'unavailable_backend_status_uses_metadata');
  check(Date.now() - started < 2000, 'unavailable_backend_status_is_responsive');
  const failed = await rpc('credentials.check', {}, true);
  check(failed.error?.data?.code === 'authentication_required', 'unavailable_backend_explicit_error');
  check(Date.now() - started < 20000, 'unavailable_backend_read_is_bounded');
  check(!fs.existsSync(path.join(dataset, 'runtime.json')), 'no_plaintext_fallback');
  await stop();
  await start();
  await rpc('credentials.check');
  for (const reference of references) check(tool('clear', account(reference)).status === 0, 'probe_entry_deleted');
  check(Boolean((await rpc('credentials.check', {}, true)).error), 'deleted_key_read_rejected');
  for (const reference of references) check(tool('lookup', account(reference)).status !== 0, 'deleted_entry_absent');
  await stop();
  report.passed = true;
} catch (error) {
  report.error = secrets.some(secret => String(error).includes(secret)) ? 'redacted_probe_failure' : String(error);
} finally {
  if (host?.exitCode === null) {
    try { await stop(); } catch { host.kill('SIGTERM'); await once(host, 'exit'); }
  }
  await browser?.close();
  report.cleanup = references.map(reference => ({
    reference, clear_exit: tool('clear', account(reference)).status,
    absent: tool('lookup', account(reference)).status === 1,
  }));
  report.executors_released = report.hosts.every(({ pid }) => {
    try { process.kill(pid, 0); return false; } catch { return true; }
  });
  report.passed &&= report.executors_released && report.cleanup.every(entry => entry.absent);
  fs.mkdirSync(path.dirname(path.resolve(output)), { recursive: true });
  fs.writeFileSync(output, JSON.stringify(report, null, 2) + '\n');
  fs.rmSync(root, { recursive: true, force: true });
}
console.log(JSON.stringify({ passed: report.passed, output }));
process.exitCode = report.passed ? 0 : 1;

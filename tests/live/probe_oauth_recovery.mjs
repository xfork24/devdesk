// Real native OAuth start/cancel with an intentionally lost local HTTP response.
// Official authorization navigation is intercepted; no user login or token is used.
import { chromium, expect } from '../../web/node_modules/@playwright/test/index.mjs';
import { spawn } from 'node:child_process';
import { once } from 'node:events';
import fs from 'node:fs';
import os from 'node:os';
import path from 'node:path';
import readline from 'node:readline';

const args = process.argv.slice(2);
const option = (key, fallback) => args.includes(key) ? args[args.indexOf(key) + 1] : fallback;
const output = option('--output');
if (!output) throw new Error('--output is required');
const binary = path.resolve(option('--binary', 'target/debug/devdesk-host'));
const root = fs.mkdtempSync(path.join(os.tmpdir(), 'devdesk-oauth-recovery-'));
const dataset = path.join(root, 'data');
const argv = [binary, '--data', dataset, '--port', '0'];
const host = spawn(binary, argv.slice(1), { cwd: root, stdio: ['ignore', 'pipe', 'ignore'] });
const report = {
  passed: false, source: 'actual browser → Host → native Codex OAuth',
  official_login_completed: false, official_navigation_intercepted: true,
  injected_fault: 'abort first local HTTP response after native start succeeded',
  host: { pid: host.pid, argv }, checks: [],
};
let browser, page, url;
const check = (condition, name) => {
  if (!condition) throw new Error(name);
  report.checks.push(name);
};
const rpc = async (method, params = {}) => {
  const response = await page.request.post(url + '/rpc', { data: { jsonrpc: '2.0', id: 1, method, params } });
  const body = await response.json();
  if (body.error) throw new Error(method + ':' + body.error.data.code);
  return body.result;
};
try {
  const lines = readline.createInterface({ input: host.stdout });
  const ready = (async () => {
    for await (const line of lines) if (line.startsWith('http://127.0.0.1:')) return line;
    throw new Error('host_start_failed');
  })();
  let timer;
  try { url = await Promise.race([ready, new Promise((_, reject) => {
    timer = setTimeout(() => reject(new Error('host_start_timeout')), 15000);
  })]); } finally { clearTimeout(timer); lines.close(); host.stdout.resume(); }
  browser = await chromium.launch({ headless: true });
  const context = await browser.newContext();
  await context.route('https://auth.openai.com/**', route => route.fulfill({ body: 'Authorization navigation fixture; no login performed.' }));
  page = await context.newPage();
  await page.goto(url);
  const installation = await rpc('runtime.discover_codex');
  report.installation_id = installation.id;
  const params = { installation_id: installation.id };
  await page.reload();
  await page.getByRole('button', { name: '设置', exact: true }).click();
  let dropped;
  await page.route('**/rpc', async route => {
    if (route.request().postDataJSON()?.method === 'codex.auth.start' && !dropped) {
      const response = await route.fetch();
      const body = await response.json();
      if (body.error) throw new Error('initial_native_start_failed:' + body.error.data.code);
      dropped = body.result;
      await route.abort('failed');
    } else await route.continue();
  });
  const loginButton = page.getByRole('button', { name: '网页登录', exact: true });
  await loginButton.click();
  await expect(page.getByText(/无法开始登录，可再次点击重试/)).toBeVisible();
  check((await rpc('codex.auth.status', params)).pending, 'lost_response_leaves_actual_native_login_pending');
  const recoveredResponse = page.waitForResponse(response => response.url() === url + '/rpc'
    && response.request().postDataJSON()?.method === 'codex.auth.start');
  await loginButton.click();
  const recovered = (await (await recoveredResponse).json()).result;
  check(recovered.loginId === dropped.loginId && recovered.authUrl === dropped.authUrl, 'explicit_retry_recovers_same_login');
  check(new URL(recovered.authUrl).origin === 'https://auth.openai.com', 'actual_official_origin');
  await expect(page.getByRole('link', { name: '打开官方授权网页', exact: true })).toBeVisible();
  await page.unrouteAll({ behavior: 'wait' });
  await page.reload();
  await page.getByRole('button', { name: '设置', exact: true }).click();
  await page.evaluate(() => { window.open = () => null; });
  await loginButton.click();
  await expect(page.getByText(/浏览器未打开新窗口/)).toBeVisible();
  check(await page.getByRole('link', { name: '打开官方授权网页', exact: true }).getAttribute('href') === dropped.authUrl, 'reload_and_blocked_popup_keep_same_official_link');
  await page.getByRole('button', { name: '取消登录', exact: true }).click();
  await expect(page.getByRole('button', { name: '取消登录', exact: true })).toHaveCount(0);
  check(!(await rpc('codex.auth.status', params)).pending, 'native_cancel_clears_pending');
  const next = await rpc('codex.auth.start', params);
  check(next.loginId !== dropped.loginId, 'new_explicit_login_after_cancel_has_new_identity');
  await rpc('codex.auth.cancel', { ...params, login_id: next.loginId });
  check(!(await rpc('codex.auth.status', params)).pending, 'second_native_login_cancelled');
  report.passed = true;
} catch (error) {
  // Browser errors can include a full authorization URL. Keep only the class.
  report.failure_type = error.constructor.name;
} finally {
  if (page && host.exitCode === null) {
    const exited = once(host, 'exit');
    await page.request.post(url + '/shutdown', { data: {} });
    const [code] = await exited;
    report.host_released = code === 0;
  } else if (host.exitCode === null) {
    host.kill('SIGTERM'); await once(host, 'exit'); report.host_released = true;
  }
  await browser?.close();
  report.passed &&= report.host_released;
  fs.mkdirSync(path.dirname(path.resolve(output)), { recursive: true });
  fs.writeFileSync(output, JSON.stringify(report, null, 2) + '\n');
  fs.rmSync(root, { recursive: true, force: true });
}
console.log(JSON.stringify({ passed: report.passed, output }));
process.exitCode = report.passed ? 0 : 1;

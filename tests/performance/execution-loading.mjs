import { chromium } from "../../web/node_modules/playwright-core/index.mjs";
import fs from "node:fs";

const [origin, output, count, , selectedProject] = process.argv.slice(2);
const browser = await chromium.launch({ channel: "chromium", headless: true });
const page = await browser.newPage({ viewport: { width: 1440, height: 1000 } });
const trace = [], navigation = [], errors = [], consoleErrors = [];
const httpFailures = [];
let sample = -1, started = Date.now();
await page.addInitScript(() => {
  window.__executionLongTasks = [];
  new PerformanceObserver((list) => {
    for (const entry of list.getEntries()) {
      if (window.__executionLongTasks.length < 1000)
        window.__executionLongTasks.push({ start_ms: entry.startTime, ms: entry.duration });
    }
  }).observe({ type: "longtask", buffered: true });
});
page.on("pageerror", (error) => errors.push(String(error)));
page.on("console", (message) => {
  if (message.type() === "error") consoleErrors.push(message.text().slice(0, 500));
});
page.on("response", (response) => {
  if (response.status() >= 400) httpFailures.push({ url: response.url(), status: response.status() });
});
page.on("request", (request) => {
  if (!request.url().endsWith("/rpc")) return;
  const call = request.postDataJSON();
  const row = { sample, method: call.method, start_ms: Date.now() - started, phase_started: started, request_started: Date.now() };
  trace.push(row);
  request.performanceRow = row;
});
page.on("requestfinished", async (request) => {
  const row = request.performanceRow;
  if (!row) return;
  try {
    const response = await request.response();
    const body = await response.body();
    row.end_ms = Date.now() - row.phase_started;
    row.duration_ms = Date.now() - row.request_started;
    row.bytes = body.length;
    const value = JSON.parse(body);
    if (value.error) row.error = value.error;
  } catch (error) { row.error = String(error); }
});
page.on("requestfailed", (request) => {
  if (request.performanceRow) request.performanceRow.error = request.failure();
});
function save() {
  fs.writeFileSync(output + "/browser.json", JSON.stringify({
    trace, navigation, errors, consoleErrors, httpFailures, scheduler: false,
    formal_100_frontend_acceptance: false,
  }, null, 2));
}
try {
  await page.goto(origin);
  const bootstrap = await page.evaluate(async (selectedProject) => {
    const capabilities = await (await fetch("/capabilities")).json();
    const project = selectedProject || capabilities.active_project_id;
    const response = await fetch("/rpc", { method: "POST", headers: { "Content-Type": "application/json" }, body: JSON.stringify({ jsonrpc: "2.0", id: "execution-profile", method: "query.workspace", params: { project_id: project, limit: 1 } }) });
    const value = await response.json();
    if (value.error) throw Error(JSON.stringify(value.error));
    return { project, feature: value.result.features?.[0]?.id };
  }, selectedProject);
  if (!bootstrap.project || !bootstrap.feature) throw Error("execution baseline needs a project and Feature");
  await page.goto(`${origin}/?tab=overview&detail=manager&project_id=${encodeURIComponent(bootstrap.project)}&feature_id=${encodeURIComponent(bootstrap.feature)}`);
  const execution = page.getByRole("button", { name: "执行与交付", exact: true });
  await execution.waitFor();
  for (let index = 0; index < Number(count); index++) {
    await page.getByRole("button", { name: "项目管理", exact: true }).click();
    await page.getByRole("heading", { name: "项目管理", exact: true, level: 1 }).waitFor({ timeout: 60000 });
    sample = index;
    started = Date.now();
    await execution.click();
    await page.getByRole("region", { name: "项目经理协作", exact: true }).waitFor({ timeout: 60000 });
    const managerVisible = Date.now() - started;
    await page.getByRole("region", { name: "项目经理协作", exact: true }).getByText(/^当前调度：/).waitFor({ timeout: 60000 });
    await page.evaluate(() => new Promise((resolve) => requestAnimationFrame(() => requestAnimationFrame(resolve))));
    navigation.push({ sample, manager_visible_ms: managerVisible, manager_state_visible_ms: Date.now() - started });
    save();
    if (index === 0) await page.screenshot({ path: output + "/execution-page.png", fullPage: true });
  }
  fs.writeFileSync(output + "/main-thread.json", JSON.stringify(await page.evaluate(() => window.__executionLongTasks), null, 2));
} finally {
  save();
  await browser.close();
}

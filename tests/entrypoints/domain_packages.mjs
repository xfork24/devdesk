// Run with a separately built core Host and core Web. Uses an actual Docker worker.
import assert from "node:assert/strict";
import { createHash } from "node:crypto";
import { execFile as call, spawn } from "node:child_process";
import { mkdir, mkdtemp, readFile, rm, writeFile } from "node:fs/promises";
import { once } from "node:events";
import os from "node:os";
import path from "node:path";
import { createRequire } from "node:module";
import { fileURLToPath } from "node:url";
import { promisify } from "node:util";

const execFile = promisify(call);
const root = fileURLToPath(new URL("../..", import.meta.url));
const require = createRequire(path.join(root, "web", "package.json"));
const { chromium } = require("playwright");
const [hostBinary, web] = process.argv.slice(2);
assert(hostBinary && web, "Pass core devdesk-host and core Web paths");
const tmp = await mkdtemp(path.join(os.tmpdir(), "devdesk-domains-"));
const fixtures = path.join(root, "tests/fixtures/domain-packages/build.py");
const cli = path.join(root, "tests/entrypoints/domain_client.py");
const repository = path.join(tmp, "repository");
let host, browser;
let hostLog = "";
try {
  await execFile("python3", [fixtures, tmp]);
  const gitZip = path.join(tmp, "git-probe.zip");
  const manuscriptZip = path.join(tmp, "manuscript.zip");
  const digests = await Promise.all([gitZip, manuscriptZip].map(async (file) =>
    createHash("sha256").update(await readFile(file)).digest("hex"),
  ));
  await mkdir(repository);
  const git = async (...args) => (await execFile("git", ["-C", repository, ...args])).stdout.trim();
  await git("init", "-b", "main");
  await writeFile(path.join(repository, "note.txt"), "packaged Git probe\n");
  await git("add", "note.txt");
  await git("-c", "user.name=Domain probe", "-c", "user.email=domain@localhost",
    "-c", "commit.gpgsign=false", "-c", "core.hooksPath=/dev/null", "commit", "-m", "probe");
  const head = await git("rev-parse", "HEAD");
  const env = { ...process.env };
  delete env.DEVDESK_ACCESS_TOKEN;
  host = spawn(hostBinary, [
    "--data", path.join(tmp, "data"), "--web-dir", web, "--port", "0",
    ...digests.flatMap((digest) => ["--trust-domain", digest]),
  ], { env });
  const exited = once(host, "exit");
  host.stderr.on("data", (data) => { hostLog += data; });
  const base = await new Promise((resolve, reject) => {
    const timer = setTimeout(() => reject(new Error("Host startup timed out")), 15000);
    host.stdout.on("data", (data) => {
      hostLog += data;
      const url = hostLog.match(/http:\/\/127\.0\.0\.1:\d+/)?.[0];
      if (url) { clearTimeout(timer); resolve(url); }
    });
    void exited.then(([code]) => { clearTimeout(timer); reject(new Error(`Host exited ${code}`)); });
  });
  const native = async (action, ...args) =>
    JSON.parse((await execFile("python3", [cli, base, action, ...args], { timeout: 40000 })).stdout);
  browser = await chromium.launch();
  const page = await browser.newPage();
  const second = await browser.newPage();
  const pageErrors = [];
  page.on("pageerror", (error) => pageErrors.push(error.message));
  for (const tab of [page, second]) tab.setDefaultTimeout(12000);
  await Promise.all([page.goto(base), second.goto(base)]);
  await page.getByText("空领域", { exact: true }).waitFor();
  await page.getByRole("button", { name: "核心配置" }).click();
  await page.getByText("GPT-6.2 Sol High", { exact: false }).first().waitFor();
  await page.getByLabel("显示名称").fill("Domain smoke model");
  await page.getByLabel("模型标识").fill("test-core-model");
  await page.getByLabel("供应商").selectOption("openai");
  await page.getByRole("button", { name: "保存模型" }).click();
  await page.getByText("Domain smoke model", { exact: false }).first().waitFor();
  const preparation = page.waitForResponse(
    (r) => r.url().endsWith("/rpc") && r.request().postDataJSON()?.method === "runtime.prepare_harness",
    { timeout: 120000 },
  );
  await page.getByRole("listitem").filter({ hasText: "builtin-harness-codex-managed:2" })
    .getByRole("button", { name: "准备运行环境" }).click();
  const ready = await (await preparation).json();
  assert(!ready.error, JSON.stringify(ready.error));
  assert.equal(ready.result.artifact.kind, "docker_image");
  assert.match(ready.result.artifact.image_id, /^sha256:[0-9a-f]{64}$/);
  await page.getByRole("button", { name: "Git", exact: true }).click();
  await page.getByLabel("本地仓库路径").first().fill(repository);
  await page.getByRole("button", { name: "查询", exact: true }).first().click();
  await page.getByText(head, { exact: false }).first().waitFor();

  const activate = async (file) => {
    await page.getByRole("button", { name: "领域包", exact: true }).click();
    await page.getByLabel("领域 ZIP").setInputFiles(file);
    const installed = page.waitForResponse((r) => r.url().endsWith("/rpc") && r.request().postDataJSON()?.method === "domain.install");
    await page.getByRole("button", { name: "导入", exact: true }).click();
    const installReply = await (await installed).json();
    assert(!installReply.error, JSON.stringify(installReply.error));
    const row = page.getByRole("listitem").filter({ hasText: path.basename(file, ".zip") });
    const activated = page.waitForResponse((r) => r.url().endsWith("/rpc") && r.request().postDataJSON()?.method === "domain.activate");
    await row.getByRole("button", { name: "激活", exact: true }).click();
    const activationReply = await (await activated).json();
    assert(!activationReply.error, JSON.stringify(activationReply.error));
  };
  await activate(gitZip);
  const gitPage = page.getByRole("button", { name: "Git repository" });
  await gitPage.click();
  await second.getByRole("button", { name: "Git repository" }).waitFor();
  await page.getByLabel("领域处理器").fill("git-probe.repository.head");
  await page.getByLabel("本地仓库路径").fill(repository);
  await page.getByRole("button", { name: "运行领域查询" }).click();
  await page.getByText(head, { exact: false }).waitFor();
  const webResult = JSON.parse(await page.locator("output").innerText());
  assert.deepEqual(webResult, await native("git", repository));
  assert.equal(webResult.head, head);
  assert.equal((await native("install-contract", gitZip)).digest, digests[0]);

  await activate(manuscriptZip);
  await second.getByRole("button", { name: "Manuscript", exact: true }).waitFor();
  await second.getByRole("button", { name: "Git repository" }).waitFor({ state: "detached" });
  const result = await native("workflow");
  await page.getByRole("button", { name: "领域工作流" }).click();
  await page.getByLabel("项目 ID", { exact: true }).fill(result.project_id);
  await page.getByLabel("执行 ID").fill(result.execution_id);
  await page.getByRole("button", { name: "读取执行" }).click();
  const execution = page.getByLabel("领域执行结果");
  await execution.getByText("Local manuscript", { exact: false }).waitFor();
  const body = JSON.parse(await execution.innerText());
  assert.equal(body.record.id, result.execution_id);
  assert.equal(body.record.data.status, "completed");

  const mode = page.getByLabel("调度方式");
  await mode.locator('option[value="agent"]').waitFor({ state: "attached" });
  await mode.selectOption("manual");
  const owner = await native("prepare-owner");
  await page.getByLabel("归属项目 ID").fill(owner.project_id);
  await page.getByLabel("归属对象 ID").fill(owner.id);
  await page.getByLabel("归属对象修订").fill(String(owner.revision));
  const starting = page.waitForResponse(
    (r) => r.url().endsWith("/rpc") && r.request().postDataJSON()?.method === "domain.workflow.start",
  );
  await page.getByRole("button", { name: "启动工作流" }).click();
  const started = await (await starting).json();
  assert(!started.error, JSON.stringify(started.error));
  const manual = await native("manual", started.result.project_id, started.result.execution_id);
  assert(await native("check-foreign-entity", manual.project_id, result.entity_id));

  await second.getByRole("button", { name: "Manuscript", exact: true }).click();
  await second.getByLabel("领域处理器").waitFor();
  assert.match(hostLog, /Runtime:/);
  const pid = host.pid;
  await native("unload");
  await second.getByText("空领域", { exact: true }).waitFor();
  await second.getByLabel("领域处理器").waitFor({ state: "detached" });
  // Reinstallation reuses the frozen sources and finished execution.
  await activate(manuscriptZip);
  const original = await native("read", result.project_id, result.execution_id);
  assert.equal(original.record.id, result.execution_id);
  assert.equal(original.record.data.status, "completed");
  const entity = await native("read-entity", result.project_id, result.entity_id);
  assert.equal(entity.revision, 2);
  assert.equal(entity.data.title, "  Local manuscript  ");
  await native("unload");
  await second.getByText("空领域", { exact: true }).waitFor();
  assert.equal(host.pid, pid);
  assert.equal(host.exitCode, null);
  assert.deepEqual(pageErrors, []);
  console.log(JSON.stringify({ pid, head, image: ready.result.artifact.image_id, execution: result, manual, unloaded: true }));
} catch (error) {
  console.error(hostLog);
  throw error;
} finally {
  if (browser) await browser.close();
  if (host?.exitCode === null) {
    const exited = once(host, "exit");
    host.kill("SIGTERM");
    const timer = setTimeout(() => host.kill("SIGKILL"), 6000);
    try { await exited; } finally { clearTimeout(timer); }
  }
  await rm(tmp, { recursive: true, force: true });
}

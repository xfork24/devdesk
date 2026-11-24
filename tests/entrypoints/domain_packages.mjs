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
const [hostBinary, web, packagedSoftware] = process.argv.slice(2).map((argument) => path.resolve(argument));
assert(hostBinary && web, "Pass core devdesk-host and core Web paths");
const tmp = await mkdtemp(path.join(os.tmpdir(), "devdesk-domains-"));
const fixtures = path.join(root, "tests/fixtures/domain-packages/build.py");
const cli = path.join(root, "tests/entrypoints/domain_client.py");
const repository = path.join(tmp, "repository");
let host, browser, page, second;
const pageErrors = [];
let hostLog = "";
try {
  await execFile("python3", [fixtures, tmp]);
  const gitZip = path.join(tmp, "git-probe.zip");
  const manuscriptZip = path.join(tmp, "manuscript.zip");
  const softwareZip = packagedSoftware ?? path.join(tmp, "software-development.zip");
  if (!packagedSoftware) {
    await execFile("npm", ["--prefix", path.join(root, "web"), "run", "build:software"]);
    await execFile("python3", [path.join(root, "configurations/software-development/build.py"), softwareZip]);
  }
  const digests = await Promise.all([gitZip, manuscriptZip, softwareZip].map(async (file) =>
    createHash("sha256").update(await readFile(file)).digest("hex"),
  ));
  await mkdir(repository);
  const git = async (...args) => (await execFile("git", ["-C", repository, ...args])).stdout.trim();
  await git("init", "-b", "main");
  await writeFile(path.join(repository, "note.txt"), "packaged Git probe\n");
  await writeFile(path.join(repository, "package.json"), JSON.stringify({ scripts: { test: "node --test" } }));
  await git("add", "note.txt", "package.json");
  await git("-c", "user.name=Domain probe", "-c", "user.email=domain@localhost",
    "-c", "commit.gpgsign=false", "-c", "core.hooksPath=/dev/null", "commit", "-m", "probe");
  const head = await git("rev-parse", "HEAD");
  const env = { ...process.env };
  delete env.DEVDESK_ACCESS_TOKEN;
  host = spawn(hostBinary, [
    "--data", path.join(tmp, "data"), "--web-dir", web, "--port", "0",
    ...digests.flatMap((digest) => ["--trust-domain", digest]),
  ], { env, cwd: tmp });
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
  page = await browser.newPage();
  second = await browser.newPage();
  for (const tab of [page, second]) {
    tab.on("pageerror", (error) => pageErrors.push(error.message));
    tab.on("console", (message) => {
      if (message.type() === "error") hostLog += `\nBrowser: ${message.text()}`;
    });
  }
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
    const row = page.getByRole("listitem").filter({
      hasText: `${installReply.result.id} ${installReply.result.version}`,
    });
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
  await writeFile(path.join(repository, "package.json"), JSON.stringify({ scripts: { test: "no test specified" } }));
  await activate(softwareZip);
  const software = await native("software", repository, head);
  const project = software.run.project_id;
  const settingsId = "software-development.engineering-settings";
  const settingsButton = (tab) => tab.getByRole("button", { name: "工程设置", exact: true });
  await settingsButton(page).click();
  await settingsButton(second).click();
  await page.getByRole("heading", { name: "数据与运行" }).waitFor();
  await second.getByRole("heading", { name: "数据与运行" }).waitFor();
  assert.equal(await page.locator(".app").count(), 1);
  assert.equal(await page.locator("iframe").count(), 0);
  await page.getByRole("navigation", { name: "软件开发导航" }).waitFor();
  const style = page.locator('link[href*="/domain-assets/"][href$="/workbench.css"]');
  assert.equal(await style.count(), 1);
  const oldAsset = `/domain-assets/${software.epoch}/${digests[2]}/ui/web/workbench.mjs`;
  assert.equal((await page.request.get(base + oldAsset)).status(), 200);
  const nativeSettings = await native("contribution", String(software.epoch), settingsId, JSON.stringify({ project_id: project }));
  const uiSettings = await page.evaluate(async ({ epoch, contribution, project }) => {
    const response = await fetch("/rpc", { method: "POST", headers: { "content-type": "application/json" }, body: JSON.stringify({
      jsonrpc: "2.0", id: 1, method: "query.domain.contribution",
      params: { epoch, contribution, input: { project_id: project } },
    }) });
    return (await response.json()).result;
  }, { epoch: software.epoch, contribution: settingsId, project });
  assert.deepEqual(uiSettings, nativeSettings);
  assert.deepEqual(software.contributions.map(({ id }) => id), [
    "software-development.branches", "software-development.delivery", settingsId,
  ]);
  let feature;
  const githubRepo = process.env.DEVDESK_DOMAIN_TEST_GITHUB_REPO;
  if (githubRepo) {
    assert.match(githubRepo, /^(?!.*(?:^|\/)\.\.?\/)[a-z0-9_.-]+\/[a-z0-9_.-]*test[a-z0-9_.-]*$/i);
    const rpcReply = (method, matches = () => true) => page.waitForResponse(
      (response) => {
        if (!response.url().endsWith("/rpc")) return false;
        const body = response.request().postDataJSON();
        return body?.method === method && matches(body.params);
      },
      { timeout: 40_000 },
    );
    const accepted = async (pending) => {
      const reply = await (await pending).json();
      assert(!reply.error, JSON.stringify(reply.error));
      return reply.result;
    };
    await page.getByRole("button", { name: "分支队列", exact: true }).click();
    await page.getByRole("navigation", { name: "软件开发导航" }).waitFor();
    await page.getByRole("button", { name: "打开 GitHub 项目", exact: true }).first().click();
    const dialog = page.getByRole("dialog");
    await dialog.getByLabel("GitHub 仓库（owner/repo 或 URL）").fill(githubRepo);
    await dialog.getByLabel("主分支").fill(process.env.DEVDESK_DOMAIN_TEST_GITHUB_BRANCH || "main");
    await dialog.getByLabel("项目名称").fill("P5 domain Web acceptance");
    const creating = rpcReply("domain.project.create");
    await dialog.getByRole("button", { name: "确认", exact: true }).click();
    const connected = await accepted(creating);
    await page.getByLabel("当前项目", { exact: true }).locator(`option[value="${connected.id}"]`).waitFor({ state: "attached" });
    await page.getByText("GitHub 已接入", { exact: true }).waitFor({ timeout: 120_000 });
    await page.getByRole("button", { name: "项目管理", exact: true }).click();
    await page.getByLabel("基础 Standard", { exact: true }).waitFor();
    assert.equal(await page.getByLabel("基础 Standard", { exact: true }).inputValue(),
      "software-development.standard-engineering-delivery:5");
    const saving = rpcReply("project.configuration.update");
    await page.getByRole("button", { name: "保存项目配置", exact: true }).click();
    await accepted(saving);
    await page.getByRole("button", { name: "恢复领域默认配置", exact: true }).click();
    assert.equal(await page.getByLabel("基础 Standard", { exact: true }).inputValue(),
      "software-development.standard-engineering-delivery:5");
    await page.getByText("集成新项目", { exact: true }).click();
    await page.getByLabel("GitHub 仓库", { exact: true }).fill(githubRepo);
    await page.getByLabel("主分支", { exact: true }).fill(process.env.DEVDESK_DOMAIN_TEST_GITHUB_BRANCH || "main");
    const reopening = rpcReply("domain.project.create");
    await page.getByRole("button", { name: "登记项目并配置", exact: true }).click();
    assert.equal((await accepted(reopening)).id, connected.id);
    await page.getByRole("button", { name: "工作总览", exact: true }).click();
    await page.getByRole("button", { name: "新 Feature", exact: true }).click();
    await dialog.getByLabel("Feature 名称").fill("P5 Web domain feature");
    await dialog.getByLabel("初始需求", { exact: true }).fill("验证领域页面创建 Feature 后可查看执行和历史。");
    const creatingFeature = rpcReply("feature.create");
    await dialog.getByRole("button", { name: "确认", exact: true }).click();
    feature = { project: connected.id, id: (await accepted(creatingFeature)).id };
    const branches = rpcReply("query.domain.contribution", (params) => params.contribution === "software-development.branches");
    await page.getByRole("button", { name: "展开 P5 Web domain feature", exact: true }).click();
    const webBranch = await accepted(branches);
    assert.deepEqual(webBranch, await native("contribution", String(software.epoch),
      "software-development.branches", JSON.stringify({ project_id: connected.id, feature_id: feature.id })));
    const history = await native("project-read", "query.history", connected.id, "{}");
    assert(JSON.stringify(history).includes("feature.create"), JSON.stringify(history).slice(0, 300));
  }
  await page.getByRole("button", { name: "Pull Requests 与 Diff", exact: true }).click();
  await page.getByRole("button", { name: "历史记录", exact: true }).click();
  await page.getByRole("heading", { name: "历史记录", exact: true, level: 1 }).waitFor();
  await page.goBack();
  await page.getByRole("heading", { name: "执行与交付" }).waitFor();

  // Reinstallation reuses frozen sources and a completed execution.
  await activate(manuscriptZip);
  await settingsButton(second).waitFor({ state: "detached" });
  await second.getByRole("navigation", { name: "软件开发导航" }).waitFor({ state: "detached" });
  await second.locator('link[href*="/domain-assets/"][href$="/workbench.css"]').waitFor({ state: "detached" });
  assert.equal((await second.request.get(base + oldAsset)).status(), 404);
  assert(await native("stale-contribution", String(software.epoch), settingsId, project));
  const original = await native("read", result.project_id, result.execution_id);
  assert.equal(original.record.id, result.execution_id);
  assert.equal(original.record.data.status, "completed");
  const entity = await native("read-entity", result.project_id, result.entity_id);
  assert.equal(entity.revision, 2);
  assert.equal(entity.data.title, "  Local manuscript  ");
  await page.getByRole("listitem").filter({ hasText: "software-development 0.4.0" })
    .getByRole("button", { name: "激活", exact: true }).click();
  await settingsButton(second).waitFor();
  const preserved = await native("read", project, software.run.execution_id);
  assert.equal(preserved.record.data.status, "completed");
  assert.equal(preserved.domain_submission.entities.test_configuration, software.proof_id);

  await page.getByRole("listitem").filter({ hasText: "software-development 0.4.0" })
    .getByRole("button", { name: "卸载", exact: true }).click();
  await second.getByText("空领域", { exact: true }).waitFor();
  await settingsButton(second).waitFor({ state: "detached" });
  await second.getByRole("button", { name: "核心配置" }).click();
  await second.getByText("Domain smoke model", { exact: false }).first().waitFor();
  assert.equal(host.pid, pid);
  assert.equal(host.exitCode, null);
  assert.deepEqual(pageErrors, []);
  console.log(JSON.stringify({ pid, head, image: ready.result.artifact.image_id, execution: result, manual, software, feature, unloaded: true }));
} catch (error) {
  console.error(hostLog, pageErrors);
  for (const tab of [page, second].filter(Boolean)) {
    console.error(tab.url(), (await tab.locator("body").innerText()).slice(0, 2200));
  }
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

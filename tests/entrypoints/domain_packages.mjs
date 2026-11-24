// Run with a separately built core Host and core Web. Uses an actual Docker worker.
import assert from "node:assert/strict";
import { createHash } from "node:crypto";
import { execFile as call, spawn } from "node:child_process";
import { access, mkdir, mkdtemp, readFile, realpath, rm, writeFile } from "node:fs/promises";
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
    "--web-dir", web, "--port", "0",
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
  const rpc = async (tab, method, params = {}) => {
    const response = await tab.request.post(base + "/rpc", {
      data: { jsonrpc: "2.0", id: crypto.randomUUID(), method, params },
    });
    assert.equal(response.status(), 200, await response.text());
    const reply = await response.json();
    assert(!reply.error, JSON.stringify(reply.error));
    return reply.result;
  };
  await Promise.all([page.goto(base), second.goto(base)]);
  await second.getByLabel("数据目录", { exact: true }).waitFor();
  await page.getByLabel("数据目录", { exact: true }).fill(path.join(tmp, "data"));
  await page.getByRole("button", { name: "初始化 / 打开数据集" }).click();
  await page.getByRole("heading", { name: "基础组件", exact: true }).waitFor();
  await second.getByRole("heading", { name: "基础组件", exact: true }).waitFor();
  await page.getByText("v0.5.0", { exact: true }).waitFor();
  await page.getByText("GPT-6.2 Sol High", { exact: false }).first().waitFor();
  await rpc(page, "model.create", { command_id: crypto.randomUUID(), payload: {
    name: "Domain smoke model", scope: "global", data: {
      provider: "openai", model: "test-core-model", parameters: {},
      capabilities: ["structured-output", "tool-calling"],
    },
  } });
  await page.getByRole("tab", { name: "Internal Tools", exact: true }).click();
  await page.getByRole("button", { name: /Accept Step Outputs/ }).waitFor();
  await page.getByRole("button", { name: "历史记录", exact: true }).click();
  await page.getByRole("heading", { name: "历史记录", exact: true, level: 1 }).waitFor();
  await page.getByText("resource.changed", { exact: true }).first().waitFor();
  await page.getByRole("button", { name: "设置", exact: true }).click();
  await page.getByRole("heading", { name: "数据与运行" }).waitFor();
  assert.equal(await page.getByLabel("默认 Agent Harness", { exact: true }).inputValue(),
    (await rpc(page, "query.settings")).default_harness);
  await page.getByLabel("全局并发额度", { exact: true }).fill("25");
  await page.getByRole("button", { name: "保存额度", exact: true }).click();
  await page.getByText("并发额度已保存", { exact: true }).waitFor();
  assert.equal((await rpc(page, "query.settings")).global_concurrency, 25);
  const ready = { result: await rpc(page, "runtime.prepare_harness", { id: "builtin-harness-codex-managed:2" }) };
  assert.equal(ready.result.artifact.kind, "docker_image");
  assert.match(ready.result.artifact.image_id, /^sha256:[0-9a-f]{64}$/);
  assert.equal((await rpc(page, "query.core.git.head", { path: repository })).head, head);
  await page.getByRole("button", { name: "领域管理", exact: true }).click();
  await page.getByText("当前激活的领域：空领域", { exact: true }).waitFor();

  const activate = async (file) => {
    await page.getByRole("button", { name: "领域管理", exact: true }).click();
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
  await second.getByText("当前激活的领域：空领域", { exact: true }).waitFor();
  await writeFile(path.join(repository, "package.json"), JSON.stringify({ scripts: { test: "no test specified" } }));
  await activate(softwareZip);
  const softwareNav = (tab) => tab.getByRole("navigation", { name: "软件开发导航" });
  for (const tab of [page, second]) {
    await softwareNav(tab).waitFor();
    await tab.getByRole("heading", { name: "工作总览", exact: true }).waitFor();
    for (const name of ["Git", "领域工作流", "分支队列", "Pull Requests 与 Diff", "工程设置"]) {
      assert.equal(await tab.getByRole("button", { name, exact: true }).count(), 0, name);
    }
  }
  await page.getByRole("button", { name: "领域管理", exact: true }).click();
  await page.getByRole("heading", { name: "领域管理", exact: true }).waitFor();
  await page.getByText("当前激活的领域：software-development", { exact: true }).waitFor();
  const workbenchButton = page.getByRole("button", { name: "领域工作台", exact: true });
  assert.equal(await workbenchButton.locator("svg").count(), 1);
  await workbenchButton.click();
  await softwareNav(page).waitFor();
  const active = await rpc(page, "query.domain.active");
  const scope = { domain_epoch: active.epoch };
  const hostRoot = await realpath(path.join(tmp, "data"));
  const softwareRoot = path.join(await realpath(tmp), "software-data");
  const storageQuery = { epoch: active.epoch };
  const initialStorage = await rpc(page, "query.domain.storage", storageQuery);
  assert.equal(initialStorage.data_root, hostRoot);
  assert.equal(initialStorage.configured_root, null);
  await assert.rejects(rpc(page, "domain.storage.configure", {
    ...storageQuery, expected_revision: initialStorage.revision, data_root: repository,
  }), /请使用空目录/);
  await page.getByRole("button", { name: "领域管理", exact: true }).click();
  await page.getByRole("button", { name: "设置", exact: true }).click();
  await page.getByRole("heading", { name: "数据与运行", exact: true }).waitFor();
  assert.equal(await page.getByRole("region", { name: "领域数据目录设置" }).count(), 0);
  await workbenchButton.click();
  await softwareNav(page).getByRole("button", { name: "设置", exact: true }).click();
  const storagePanel = page.locator("main.settings-page").getByRole("region", {
    name: "领域数据目录设置",
  });
  await storagePanel.getByLabel("新项目数据目录", { exact: true }).fill(softwareRoot);
  await page.getByRole("button", { name: "保存领域数据目录", exact: true }).click();
  await page.getByText("领域数据目录已保存", { exact: true }).waitFor();
  const savedStorage = await rpc(page, "query.domain.storage", storageQuery);
  assert.equal(savedStorage.configured_root, softwareRoot);
  await assert.rejects(rpc(page, "domain.storage.configure", {
    ...storageQuery, expected_revision: initialStorage.revision, data_root: null,
  }), /revision_conflict/);
  await softwareNav(page).getByRole("button", { name: "工作总览", exact: true }).click();
  await storagePanel.waitFor({ state: "detached" });
  assert.equal((await rpc(page, "query.project_directory", scope)).total_projects, 0);
  const index = await rpc(page, "query.configuration_index", { ...scope, include_history: true });
  const automation = new Set([
    "software-development.repository-preflight:1",
    "software-development.repository-revision:1",
    "software-development.test-configuration:1",
    "software-development.detect-test-entry:1",
    "software-development.preflight-contract:1",
  ]);
  const indexed = new Set(index.records.map((row) => row.id));
  assert.equal(index.records.filter((row) => row.id.startsWith("software-development.")).length,
    active.sources.length - automation.size);
  for (const source of active.sources)
    assert.equal(indexed.has(source.id), !automation.has(source.id), source.id);
  assert((await rpc(page, "query.workflows", scope)).records.every((row) => !automation.has(row.id)));
  for (let offset = 0; offset < active.sources.length; offset += 64) {
    const expected = active.sources.slice(offset, offset + 64);
    const actual = await rpc(page, "query.configuration_records", { ...scope, refs: expected.map((row) => row.id) });
    assert.deepEqual(actual.records.map(({ id, content_hash }) => [id, content_hash]),
      expected.map(({ id, content_hash }) => [id, content_hash]));
  }
  for (const [group, tab, name] of [
    ["基础组件", "Skills", "Requirements Analysis"],
    ["角色与代理", "Role", "需求分析师"],
    ["角色与代理", "Capability", "Requirements Analysis Capability"],
    ["角色与代理", "Agent", "Requirements Analyst"],
    ["流程与规范", "Workflow", "Feature Development"],
    ["流程与规范", "Delivery", "Source Branch Publication Receipt"],
    ["流程与规范", "Spec", "PRD Specification"],
    ["流程与规范", "Standard", "Engineering Standard"],
  ]) {
    await softwareNav(page).getByRole("button", { name: group, exact: true }).click();
    await page.getByRole("tab", { name: tab, exact: true }).click();
    await page.getByText(name, { exact: false }).filter({ visible: true }).first().waitFor();
  }
  await page.getByText(/不可变规范快照/).waitFor();
  await page.getByText(/使用位置 · 当前 Standard.*（0）/).waitFor();
  const software = await native("software", repository, head);
  const project = software.run.project_id;
  const scopedProject = { ...scope, project_id: project };
  const database = path.join(softwareRoot, "projects", project, "engineering.sqlite3");
  await access(database);
  await assert.rejects(access(path.join(hostRoot, "projects", project, "engineering.sqlite3")));
  assert.equal((await rpc(page, "query.workspace", scopedProject)).project_database, database);
  assert.deepEqual((await rpc(page, "query.domain.storage", storageQuery)).project_roots,
    [{ data_root: softwareRoot, projects: 1 }]);
  const bundledSkill = active.sources.find((row) => row.type === "skill");
  const bundledRole = active.sources.find((row) => row.type === "role");
  assert.equal((await rpc(page, "query.project_directory")).total_projects, 0);
  assert((await rpc(page, "query.project_directory", scope)).projects.some((row) => row.id === project));
  assert((await rpc(page, "query.configuration_index", scopedProject)).records.every((row) => !automation.has(row.id)));
  assert((await rpc(page, "query.workflows", scopedProject)).records.every((row) => !automation.has(row.id)));
  const projectRole = await rpc(page, "role.create", {
    ...scopedProject,
    command_id: crypto.randomUUID(),
    payload: { name: "Software project specialist", scope: `project:${project}`, data: { instructions: "# Project duty" } },
  });
  const coreIndex = await rpc(page, "query.configuration_index", { include_history: true });
  assert(coreIndex.records.every((row) => !row.id.startsWith("software-development.")));
  assert(!coreIndex.records.some((row) => row.id === projectRole.id));
  assert((await rpc(page, "query.configuration_index", scope)).records.some((row) => row.id === projectRole.id));
  await assert.rejects(rpc(page, "query.configuration_records", { refs: [projectRole.id] }), /not_found/);
  const [originalRole] = (await rpc(page, "query.configuration_records", {
    ...scopedProject, refs: [bundledRole.id],
  })).records;
  const revisedRole = await rpc(page, "role.revise", {
    ...scopedProject,
    command_id: crypto.randomUUID(),
    target: originalRole.id,
    expected_revision: originalRole.revision,
    payload: { name: originalRole.name, data: { instructions: "Domain role revision for this project" } },
  });
  assert((await rpc(page, "query.configuration_records", {
    ...scopedProject, refs: [revisedRole.id],
  })).records.some((row) => row.id === revisedRole.id));
  await assert.rejects(rpc(page, "query.configuration_records", { refs: [bundledRole.id, revisedRole.id] }), /not_found/);
  assert(!Object.hasOwn(coreIndex.catalog_version.source_epochs, project));
  assert(Object.hasOwn((await rpc(page, "query.configuration_index", scope)).catalog_version.source_epochs, project));
  const readConfigurationClock = (tab, epoch) => tab.evaluate((epoch) => new Promise((resolve, reject) => {
    const params = new URLSearchParams({ tail: "true", configuration: "true" });
    if (epoch !== null) params.set("domain_epoch", String(epoch));
    const stream = new EventSource("/events?" + params);
    const timer = setTimeout(() => {
      stream.close();
      reject(new Error("configuration clock timed out"));
    }, 8000);
    stream.onmessage = ({ data }) => {
      const version = JSON.parse(data).configuration_version;
      if (!version) return;
      clearTimeout(timer);
      stream.close();
      resolve(version);
    };
  }), epoch);
  const [coreClock, domainClock] = await Promise.all([
    readConfigurationClock(page, null), readConfigurationClock(second, active.epoch),
  ]);
  assert(!Object.hasOwn(coreClock.source_epochs, project));
  assert(Object.hasOwn(domainClock.source_epochs, project));
  await assert.rejects(rpc(page, "query.configuration_records", { refs: [bundledSkill.id] }), /not_found/);
  await assert.rejects(rpc(page, "query.configuration_usage", { id: bundledSkill.id }), /not_found/);
  await assert.rejects(rpc(page, "query.configuration_index", { domain_epoch: "invalid" }), /invalid_params/);
  await assert.rejects(rpc(page, "query.configuration_index", { domain_epoch: active.epoch - 1 }), /domain_epoch_conflict/);
  await softwareNav(page).getByRole("button", { name: "基础组件", exact: true }).click();
  await page.getByRole("tab", { name: "Skills", exact: true }).click();
  await page.getByRole("button", { name: /Requirements Analysis/ }).first().click();
  await page.getByLabel("名称", { exact: true }).fill("Software skill draft");
  await page.getByRole("button", { name: "领域管理", exact: true }).click();
  await page.getByRole("button", { name: "基础组件", exact: true }).click();
  await page.getByRole("tab", { name: "Internal Tools", exact: true, selected: true }).waitFor();
  await page.getByRole("tab", { name: "Skills", exact: true }).click();
  await page.getByRole("heading", { name: "Skill 管理", exact: true }).waitFor();
  assert.equal(await page.getByLabel("名称", { exact: true }).inputValue(), "");
  assert.equal(await page.getByRole("button", { name: /Requirements Analysis/ }).count(), 0);
  await page.getByRole("button", { name: "领域工作台", exact: true }).click();
  await softwareNav(page).waitFor();

  const settingsId = "software-development.engineering-settings";
  for (const tab of [page, second]) {
    await softwareNav(tab).getByRole("button", { name: "设置", exact: true }).click();
    await tab.getByRole("heading", { name: "数据与运行" }).waitFor();
  }
  assert.equal(await page.locator(".app").count(), 1);
  assert.equal(await page.locator("iframe").count(), 0);
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
    await softwareNav(page).getByRole("button", { name: "工作总览", exact: true }).click();
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
      software.standard_ref);
    const saving = rpcReply("project.configuration.update");
    await page.getByRole("button", { name: "保存项目配置", exact: true }).click();
    await accepted(saving);
    await page.getByRole("button", { name: "恢复领域默认配置", exact: true }).click();
    assert.equal(await page.getByLabel("基础 Standard", { exact: true }).inputValue(),
      software.standard_ref);
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
  await softwareNav(page).getByRole("button", { name: "执行与交付", exact: true }).click();
  await page.getByRole("button", { name: "历史记录", exact: true }).click();
  await page.getByRole("heading", { name: "历史记录", exact: true, level: 1 }).waitFor();
  await page.goBack();
  await page.getByRole("heading", { name: "执行与交付" }).waitFor();

  // Reinstallation reuses frozen sources and a completed execution.
  await activate(manuscriptZip);
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
  await page.getByRole("listitem").filter({ hasText: `software-development ${software.version}` })
    .getByRole("button", { name: "激活", exact: true }).click();
  await softwareNav(second).waitFor();
  await page.getByRole("button", { name: "领域管理", exact: true }).click();
  const preserved = await native("read", project, software.run.execution_id);
  assert.equal(preserved.record.data.status, "completed");
  assert.equal(preserved.domain_submission.entities.test_configuration, software.proof_id);
  const updatedEpoch = (await rpc(page, "query.domain.active")).epoch;
  const restoredStorage = await rpc(page, "query.domain.storage", { epoch: updatedEpoch });
  assert.equal(restoredStorage.configured_root, softwareRoot);
  const resetStorage = await rpc(page, "domain.storage.configure", {
    epoch: updatedEpoch, expected_revision: restoredStorage.revision, data_root: null,
  });
  assert.equal(resetStorage.data_root, hostRoot);
  assert(resetStorage.project_roots.some((root) => root.data_root === softwareRoot && root.projects > 0));
  assert.equal((await rpc(page, "query.workspace", { project_id: project, domain_epoch: updatedEpoch })).project_database, database);

  await page.getByRole("listitem").filter({ hasText: `software-development ${software.version}` })
    .getByRole("button", { name: "卸载", exact: true }).click();
  await second.getByText("当前激活的领域：空领域", { exact: true }).waitFor();
  assert.equal((await rpc(second, "query.configuration_index", { include_history: true }))
    .records.some((row) => row.id.startsWith("software-development.")), false);
  await second.getByRole("button", { name: "基础组件", exact: true }).click();
  await second.getByRole("tab", { name: "Model", exact: true }).click();
  await second.getByText("Domain smoke model", { exact: false }).first().waitFor();
  assert.equal(host.pid, pid);
  assert.equal(host.exitCode, null);
  assert.deepEqual(pageErrors, []);
  console.log(JSON.stringify({ pid, head, image: ready.result.artifact.image_id, execution: result, manual, software, feature, configurationSources: active.sources.length, automationSources: automation.size, unloaded: true }));
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

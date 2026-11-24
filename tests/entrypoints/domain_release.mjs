// Exercise the published archives from a clean working directory. A directory
// containing the previous artifacts enables the independent Host and Web upgrades.
import assert from "node:assert/strict";
import { execFile as call, spawn } from "node:child_process";
import { createHash } from "node:crypto";
import { once } from "node:events";
import { access, mkdir, mkdtemp, readFile, readdir, realpath, rename, rm, writeFile } from "node:fs/promises";
import { createRequire } from "node:module";
import os from "node:os";
import path from "node:path";
import { fileURLToPath } from "node:url";
import { promisify } from "node:util";

const execFile = promisify(call);
const root = fileURLToPath(new URL("../..", import.meta.url));
const require = createRequire(path.join(root, "web/package.json"));
const { chromium } = require("playwright");
const [releaseDir, baselineDir] = process.argv.slice(2).map((dir) => path.resolve(dir));
assert(releaseDir, "Pass a directory containing the release archives");
const temp = await mkdtemp(path.join(os.tmpdir(), "devdesk-domain-release-"));
const data = path.join(temp, "data");
const pythonClient = path.join(root, "tests/entrypoints/domain_client.py");
const domainVersion = (await execFile("python3", [
  path.join(root, "plugins/software-development/build.py"), "--version",
])).stdout.trim();
const candidate = path.join(releaseDir, `software-development-${domainVersion}.zip`);
const legacy = path.join(
  baselineDir ?? path.join(root, "tests/fixtures/domain-packages"),
  "software-development-0.3.0.zip",
);
const manuscript = path.join(temp, "manuscript.zip");
const incompatible = path.join(temp, "unsupported.zip");
const repository = path.join(temp, "repository");
const digest = (body) => createHash("sha256").update(body).digest("hex");
const pass = [];
let browser;

async function bytes(zip) {
  return readFile(zip);
}

async function extract(artifacts, cwd, verify) {
  const filenames = await readdir(artifacts);
  const hostPrefix = verify ? "devdesk-core-host-0.5.0-" : "devdesk-core-host-";
  const hosts = filenames.filter((file) =>
    file.startsWith(hostPrefix) && file.endsWith(".tar.gz") && !/-(speed|symbols)\.tar\.gz$/.test(file));
  assert.equal(hosts.length, 1, "Expected one Host archive for the current machine");
  const webArchives = filenames.filter((file) => verify
    ? file === "devdesk-core-web-0.5.0.tar.gz"
    : /^devdesk-core-web(?:-.*)?\.tar\.gz$/.test(file));
  assert.equal(webArchives.length, 1, "Expected one Web Core archive");
  const [web] = webArchives;
  for (const name of [...hosts, web, ...(verify ? [path.basename(candidate)] : [])]) {
    if (verify) {
      const expected = (await readFile(path.join(artifacts, name + ".sha256"), "utf8")).split(/\s/)[0];
      assert.equal(digest(await readFile(path.join(artifacts, name))), expected, name);
    }
    if (name.endsWith(".tar.gz")) {
      await execFile("tar", ["-xzf", path.join(artifacts, name), "-C", cwd]);
    }
  }
  const hostDir = path.join(cwd, hosts[0].replace(/\.tar\.gz$/, ""));
  const host = path.join(hostDir, "devdesk-host");
  if (verify) {
    assert.deepEqual((await readdir(hostDir)).sort(), ["LICENSE.moby", "devdesk-host"]);
    const assets = path.join(root, "permissions/backends");
    const policy = JSON.parse(await readFile(path.join(assets, "manifest.json"), "utf8"));
    const { stdout } = await execFile(host, ["sandbox-policy", "--print"], { cwd });
    assert.equal(digest(stdout), policy.assets["docker/apparmor/devdesk-harness-v1.apparmor"].sha256);
    assert.equal(digest(await readFile(path.join(hostDir, "LICENSE.moby"))),
      policy.assets["docker/upstream/LICENSE.moby"].sha256);
  }
  return {
    host,
    web: path.join(cwd, web.replace(/\.tar\.gz$/, "")),
  };
}

async function withHost(host, web, operation, { crashAfter = false, legacyHost = false, dataset = data } = {}) {
  const env = { ...process.env };
  delete env.DEVDESK_ACCESS_TOKEN;
  const args = ["--data", dataset, "--web-dir", web, "--port", "0"];
  if (legacyHost) {
    // The optional pre-upgrade Host requires the legacy ZIP digest at startup.
    args.push("--trust-domain", digest(await bytes(legacy)));
  }
  const child = spawn(host, args, { env, cwd: temp });
  const exit = once(child, "exit");
  let logs = "";
  let completed = false;
  child.stdout.on("data", (data) => { logs += data; });
  child.stderr.on("data", (data) => { logs += data; });
  try {
    let base;
    for (let attempt = 0; attempt < 150; attempt++) {
      base = logs.match(/http:\/\/127\.0\.0\.1:\d+/)?.[0];
      if (base) break;
      assert.equal(child.exitCode, null, logs);
      await new Promise((resolve) => setTimeout(resolve, 100));
    }
    assert(base, logs);
    const session = await fetch(base + "/session", { method: "POST", headers: { "content-type": "application/json" }, body: "{}" });
    assert(session.ok, await session.text());
    const cookie = session.headers.get("set-cookie")?.split(";")[0];
    assert(cookie);
    const rpc = async (method, params = {}) => {
      const response = await fetch(base + "/rpc", {
        method: "POST",
        headers: { "content-type": "application/json", cookie },
        body: JSON.stringify({ jsonrpc: "2.0", id: crypto.randomUUID(), method, params }),
      });
      if (!response.ok) throw new Error(await response.text());
      return response.json();
    };
    const result = async (method, params) => {
      const reply = await rpc(method, params);
      assert(!reply.error, JSON.stringify(reply.error));
      return reply.result;
    };
    const install = async (zip) => result("domain.install", { archive: (await bytes(zip)).toString("base64") });
    const active = () => result("query.domain.active");
    const activate = async (hash) => result("domain.activate", { digest: hash, epoch: (await active()).epoch });
    const native = async (action, ...args) => JSON.parse(
      (await execFile("python3", [pythonClient, base, action, ...args], { cwd: temp, timeout: 45_000 })).stdout,
    );
    const canRender = async (view, expected) => {
      const page = await browser.newPage();
      try {
        await page.goto(base + "/?view=" + encodeURIComponent(view));
        await expected(page).waitFor({ timeout: 15_000 });
        assert.equal(await page.locator("iframe").count(), 0);
      } finally {
        await page.close();
      }
    };
    const rejected = async (zip) => {
      const before = await active();
      const failure = await rpc("domain.install", { archive: (await bytes(zip)).toString("base64") });
      assert(failure.error, "incompatible ZIP was accepted");
      assert.deepEqual((await active()).active, before.active);
      assert.equal((await active()).epoch, before.epoch);
    };
    const checked = await operation({ result, install, active, activate, native, canRender, rejected });
    completed = true;
    return checked;
  } catch (error) {
    console.error(logs);
    throw error;
  } finally {
    if (child.exitCode === null) child.kill(crashAfter && completed ? "SIGKILL" : "SIGTERM");
    const timer = setTimeout(() => child.kill("SIGKILL"), 7000);
    try { await exit; } finally { clearTimeout(timer); }
  }
}

try {
  const current = await extract(releaseDir, temp, true);
  assert.equal((await execFile(current.host, ["--version"])).stdout.trim(), "devdesk-host 0.5.0");
  const previous = baselineDir
    ? await extract(baselineDir, await mkdtemp(path.join(temp, "previous-")), false)
    : current;
  await readFile(path.join(current.web, "sdk/domain-sdk.d.ts"));
  await readFile(path.join(current.web, "sdk/domain-contributions.schema.json"));
  await execFile("python3", [path.join(root, "tests/fixtures/domain-packages/build.py"), temp]);
  await execFile("python3", ["-c", `
from zipfile import ZipFile, ZIP_DEFLATED
import json, sys
with ZipFile(sys.argv[1]) as original, ZipFile(sys.argv[3]) as legacy:
    old_names = set(legacy.namelist())
    shared = (name for name in original.namelist()
              if name in old_names and name.startswith("configurations/")
              and name not in {"configurations/manifest.json", "configurations/defaults.json", "configurations/usage.json"})
    assert all(original.read(name) == legacy.read(name) for name in shared)
    manifest = json.loads(original.read("manifest.json"))
    assert "software-development.delivery.authorize" in manifest["runtime"]["handlers"]
with ZipFile(sys.argv[1]) as original, ZipFile(sys.argv[2], "w", ZIP_DEFLATED) as rewritten:
    for name in original.namelist():
        body = original.read(name)
        if name == "manifest.json":
            manifest = json.loads(body)
            manifest["format"] = 999
            body = json.dumps(manifest).encode()
        rewritten.writestr(name, body)
`, candidate, incompatible, legacy]);
  await execFile("git", ["init", "-b", "main", repository]);
  await writeFile(path.join(repository, "package.json"), '{"scripts":{"test":"node --test"}}');
  await execFile("git", ["-C", repository, "add", "package.json"]);
  await execFile("git", ["-C", repository, "-c", "user.name=Release test",
    "-c", "user.email=release@localhost", "-c", "commit.gpgSign=false", "-c",
    "core.hooksPath=/dev/null", "commit", "-m", "release test input"]);
  const head = (await execFile("git", ["-C", repository, "rev-parse", "HEAD"])).stdout.trim();
  browser = await chromium.launch();

  const original = await withHost(previous.host, current.web, async (client) => {
    assert.equal((await client.active()).active, null);
    const permissions = await client.result("query.permission_resources");
    assert.equal(permissions.source.is_builtin, true);
    assert(permissions.profiles.some((row) => row.identity === "builtin-permission-harness-read-only"));
    await client.canRender("core", (page) => page.getByText("GPT-6.2 Sol High").first());
    const state = await client.activate((await client.install(legacy)).digest);
    assert.equal(state.active.version, "0.3.0");
    await client.rejected(baselineDir ? candidate : incompatible);
    if (baselineDir) pass.push("Web Core upgrade with previous Host and domain");
    return client.native("software", repository, head);
  }, { legacyHost: Boolean(baselineDir) });

  await withHost(current.host, previous.web, async (client) => {
    assert.equal((await client.active()).active.version, "0.3.0");
    const read = await client.native("read", original.run.project_id, original.run.execution_id);
    assert.equal(read.record.data.status, "completed");
    assert.equal(read.domain_submission.entities.test_configuration, original.proof_id);
    await client.activate((await client.install(manuscript)).digest);
    await client.canRender("manuscript.overview", (page) => page.getByLabel("领域处理器"));
    if (baselineDir) pass.push("Host upgrade with previous Web Core and domain");
  });

  const upgraded = await withHost(current.host, current.web, async (client) => {
    assert.equal((await client.active()).active.id, "manuscript");
    await client.rejected(incompatible);
    const upgraded = await client.install(candidate);
    assert.equal(upgraded.version, domainVersion);
    await client.activate(upgraded.digest);
    await client.canRender("software-development.engineering-settings", (page) => page.getByRole("heading", { name: "数据与运行" }));
    const samples = await Promise.all(Array.from({ length: 100 }, () => client.active()));
    assert(samples.every((state) => state.active.digest === upgraded.digest));
    const result = await client.native("software", repository, head);
    const old = (await client.result("query.domain.installed")).find((row) => row.version === "0.3.0");
    const frozen = await client.activate(old.digest);
    assert.equal(frozen.active.version, "0.3.0");
    assert.equal((await client.native("read", original.run.project_id, original.run.execution_id)).record.id, original.run.execution_id);
    await client.activate(upgraded.digest);
    pass.push("Runtime ZIP activation, version rollback and 100 concurrent directory requests");
    return result;
  }, { crashAfter: true });

  const external = path.join(temp, "external-domain-data");
  const unmounted = path.join(temp, "unmounted-domain-data");
  const replacement = path.join(temp, "new-domain-data");
  const recoveryRepository = "fixture/storage-recovery";
  const saved = await withHost(current.host, current.web, async (client) => {
    const state = await client.active();
    assert.equal(state.active.version, domainVersion);
    const read = await client.native("read", upgraded.run.project_id, upgraded.run.execution_id);
    assert.equal(read.record.data.status, "completed");
    assert.equal(read.domain_submission.entities.test_configuration, upgraded.proof_id);
    await client.canRender("software-development.engineering-settings", (page) => page.getByRole("heading", { name: "数据与运行" }));
    pass.push("Restart after SIGKILL recovered active digest and submitted evidence");

    const scope = { epoch: state.epoch };
    const storage = await client.result("query.domain.storage", scope);
    const configured = await client.result("domain.storage.configure", {
      ...scope, expected_revision: storage.revision, data_root: external,
    });
    const project = await client.result("domain.project.create", {
      ...scope, title: "External storage recovery", command_id: crypto.randomUUID(),
    });
    const database = path.join(configured.data_root, "projects", project.id, "engineering.sqlite3");
    await access(database);
    const projectRoot = path.dirname(database);
    const bare = path.join(projectRoot, "repository");
    const worktree = path.join(projectRoot, "execution/worktrees/mainlines", project.id);
    await execFile("git", ["clone", "--bare", repository, bare]);
    await execFile("git", ["-C", bare, "remote", "set-url", "origin", `https://github.com/${recoveryRepository}.git`]);
    await mkdir(path.dirname(worktree), { recursive: true });
    await execFile("git", ["-C", bare, "worktree", "add", worktree, "main"]);
    const directory = await client.result("query.project_directory", { domain_epoch: state.epoch });
    return { state, project, database, bare, worktree, totalProjects: directory.total_projects };
  });
  await execFile("python3", ["-c", `
import json, sqlite3, sys, uuid
database, project, bare, worktree, repository, head = sys.argv[1:]
operation = str(uuid.uuid4())
with sqlite3.connect(database) as db:
    db.execute(
        "UPDATE projects SET workspace=?, workspace_definition=?, revision=revision+1 WHERE id=?",
        (bare, json.dumps({"path":bare,"repository":repository,"mainline":"main"}), project),
    )
    db.execute(
        "INSERT INTO repository_operations VALUES(?, ?, ?, 'completed', NULL, '', 1)",
        (operation, project, json.dumps({"kind":"connect", "repository":repository, "mainline":"main"})),
    )
    db.execute(
        "INSERT INTO repository_worktrees VALUES(?, ?, ?, 'mainline', 'main', ?, ?, ?)",
        (str(uuid.uuid4()), project, project, worktree, head, operation),
    )
`, saved.database, saved.project.id, saved.bare, saved.worktree, recoveryRepository, head]);
  await rename(external, unmounted);

  const replacementProject = await withHost(current.host, current.web, async (client) => {
    const empty = await client.active();
    assert.equal(empty.active, null);
    assert.equal(empty.epoch, saved.state.epoch + 1);
    assert((await client.result("query.domain.installed")).some(
      (row) => row.digest === saved.state.active.digest));
    await client.canRender("core", (page) => page.getByText("GPT-6.2 Sol High").first());
    const installed = await client.install(candidate);
    assert.equal(installed.digest, saved.state.active.digest);
    const loaded = await client.activate(installed.digest);
    const scope = { epoch: loaded.epoch };
    const settings = await client.result("query.domain.storage", scope);
    assert.equal(settings.available, false);
    await client.canRender("software-development.engineering-settings", (page) =>
      page.getByText("此目录不可用。请恢复原目录，或保存一个可用目录。", { exact: true }));
    assert.equal((await client.result("query.project_directory", { domain_epoch: loaded.epoch })).total_projects,
      saved.totalProjects - 1);
    const create = () => client.result("domain.project.create", {
      ...scope, title: "Replacement storage", command_id: crypto.randomUUID(),
    });
    await assert.rejects(create(), /domain_storage_unavailable/);
    await assert.rejects(access(external), { code: "ENOENT" });
    await assert.rejects(client.result("domain.storage.configure", {
      ...scope, expected_revision: settings.revision, data_root: repository,
    }), /请使用空目录/);
    const configured = await client.result("domain.storage.configure", {
      ...scope, expected_revision: settings.revision, data_root: replacement,
    });
    assert.equal(configured.available, true);
    const project = await create();
    await access(path.join(replacement, "projects", project.id, "engineering.sqlite3"));
    return project;
  });

  await withHost(current.host, current.web, async (client) => {
    const restored = await client.active();
    assert.equal(restored.active.digest, saved.state.active.digest);
    assert.equal((await client.result("query.project_directory", { domain_epoch: restored.epoch })).total_projects,
      saved.totalProjects);
    assert.equal((await client.result("query.workspace", {
      project_id: replacementProject.id, domain_epoch: restored.epoch,
    })).active_project.id, replacementProject.id);
    const relocated = path.join(replacement, "projects", saved.project.id);
    await rename(path.join(unmounted, "projects", saved.project.id), relocated);
    const scope = { epoch: restored.epoch };
    const settings = await client.result("query.domain.storage", scope);
    const configured = await client.result("domain.storage.configure", {
      ...scope, expected_revision: settings.revision, data_root: replacement,
    });
    assert.equal(configured.available, true);
    const opened = await client.result("query.workspace", {
      project_id: saved.project.id, domain_epoch: restored.epoch,
    });
    assert.equal(opened.active_project.id, saved.project.id);
    const canonical = path.join(configured.data_root, "projects", saved.project.id);
    assert.equal(opened.project_database, path.join(canonical, "engineering.sqlite3"));
    assert.equal(opened.active_project.workspace, path.join(canonical, "repository"));
    const worktree = path.join(relocated, "execution/worktrees/mainlines", saved.project.id);
    assert.equal((await execFile("git", ["-C", worktree, "rev-parse", "HEAD"])).stdout.trim(), head);
    const reopened = await client.result("domain.project.create", {
      ...scope, title: "Recovered GitHub project", command_id: crypto.randomUUID(),
      reuse_connection: true, connection: { repository: recoveryRepository, mainline: "main" },
    });
    assert.equal(reopened.id, saved.project.id);
    pass.push("Saving a new domain directory recovers original projects, linked Git worktrees and manual reopening");
  });

  const secondDataset = path.join(temp, "second-host-data");
  const markerPath = path.join(replacement, ".devdesk-domain-storage");
  const previousMarker = await readFile(markerPath, "utf8");
  assert.equal(JSON.parse(previousMarker).host_root, await realpath(data));
  await withHost(current.host, current.web, async (client) => {
    const installed = await client.install(candidate);
    const active = await client.activate(installed.digest);
    const scope = { epoch: active.epoch };
    const settings = await client.result("query.domain.storage", scope);
    const configured = await client.result("domain.storage.configure", {
      ...scope, expected_revision: settings.revision, data_root: replacement,
    });
    assert.equal(configured.available, true);
    assert.equal(await readFile(markerPath, "utf8"), previousMarker);
    const reopened = await client.result("domain.project.create", {
      ...scope, title: "Project from previous Host", command_id: crypto.randomUUID(),
      reuse_connection: true, connection: { repository: recoveryRepository, mainline: "main" },
    });
    assert.equal(reopened.id, saved.project.id);
  }, { dataset: secondDataset });

  await withHost(current.host, current.web, async (client) => {
    const active = await client.active();
    const settings = await client.result("query.domain.storage", { epoch: active.epoch });
    assert.equal(settings.available, true);
    const opened = await client.result("query.workspace", {
      project_id: saved.project.id, domain_epoch: active.epoch,
    });
    assert.equal(opened.active_project.id, saved.project.id);
    assert.equal(opened.project_database, path.join(settings.data_root, "projects", saved.project.id, "engineering.sqlite3"));
    pass.push("A second Host reuses a directory registered by the first Host and reopens the original project after restart");

    await withHost(current.host, current.web, async (other) => {
      const active = await other.active();
      const scope = { epoch: active.epoch };
      const settings = await other.result("query.domain.storage", scope);
      assert.equal(settings.data_root, await realpath(replacement));
      assert.equal((await other.result("domain.storage.configure", {
        ...scope, expected_revision: settings.revision, data_root: replacement,
      })).available, true);
      assert.equal((await other.result("query.workspace", {
        project_id: replacementProject.id, domain_epoch: active.epoch,
      })).active_project.id, replacementProject.id);
      await assert.rejects(other.result("domain.project.create", {
        ...scope, title: "Project held by another Host", command_id: crypto.randomUUID(),
        reuse_connection: true, connection: { repository: recoveryRepository, mainline: "main" },
      }), /dataset_locked/);
    });
    pass.push("Concurrent Hosts can configure the same domain directory, with exclusive writers per project");
  }, { dataset: secondDataset });
  console.log(JSON.stringify({ artifactsVerified: true, checks: pass }));
} finally {
  if (browser) await browser.close();
  await rm(temp, { recursive: true, force: true });
}

// Exercise the published archives from a clean working directory. A directory
// containing the previous artifacts enables the independent Host and Web upgrades.
import assert from "node:assert/strict";
import { execFile as call, spawn } from "node:child_process";
import { createHash } from "node:crypto";
import { once } from "node:events";
import { mkdtemp, readFile, readdir, rm, writeFile } from "node:fs/promises";
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
  const hosts = filenames.filter((file) => verify
    ? file.startsWith("devdesk-core-host-0.5.0-") && file.endsWith(".tar.gz")
    : /^devdesk-core-host-.*\.tar\.gz$/.test(file));
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
  if (verify) {
    const assets = path.join(hostDir, "permissions/backends");
    const policy = JSON.parse(await readFile(path.join(assets, "manifest.json"), "utf8"));
    for (const [name, { sha256 }] of Object.entries(policy.assets)) {
      assert.equal(digest(await readFile(path.join(assets, name))), sha256, name);
    }
    if (process.platform === "linux") {
      await readFile(path.join(hostDir, "scripts/platform/linux/install-linux-sandbox-policy.sh"));
    }
  }
  return {
    host: path.join(hostDir, "devdesk-host"),
    web: path.join(cwd, web.replace(/\.tar\.gz$/, "")),
  };
}

async function withHost(host, web, operation, { crashAfter = false, legacyHost = false } = {}) {
  const env = { ...process.env };
  delete env.DEVDESK_ACCESS_TOKEN;
  const args = ["--data", data, "--web-dir", web, "--port", "0"];
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

  await withHost(current.host, current.web, async (client) => {
    assert.equal((await client.active()).active.version, domainVersion);
    const read = await client.native("read", upgraded.run.project_id, upgraded.run.execution_id);
    assert.equal(read.record.data.status, "completed");
    assert.equal(read.domain_submission.entities.test_configuration, upgraded.proof_id);
    await client.canRender("software-development.engineering-settings", (page) => page.getByRole("heading", { name: "数据与运行" }));
    pass.push("Restart after SIGKILL recovered active digest and submitted evidence");
  });
  console.log(JSON.stringify({ artifactsVerified: true, checks: pass }));
} finally {
  if (browser) await browser.close();
  await rm(temp, { recursive: true, force: true });
}

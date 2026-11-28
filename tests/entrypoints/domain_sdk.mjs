// The SDK artifact's command line tool: the usage index it computes matches what the kernel
// catalogs store, and init, check and build produce what the Host installs.
import assert from "node:assert/strict";
import { execFile } from "node:child_process";
import { existsSync, mkdirSync, mkdtempSync, readFileSync, rmSync, writeFileSync } from "node:fs";
import os from "node:os";
import path from "node:path";
import test from "node:test";
import { fileURLToPath } from "node:url";
import { promisify } from "node:util";

const run = promisify(execFile);
const repo = fileURLToPath(new URL("../..", import.meta.url));
const sdk = path.join(repo, "sdk", "domain");
// The schema checks need jsonschema; the checkout's virtualenv carries it.
const python = process.env.DEVDESK_PYTHON
  ?? (existsSync(path.join(repo, ".venv/bin/python3"))
    ? path.join(repo, ".venv/bin/python3")
    : "python3");

const cli = (arguments_, options = {}) =>
  run("node", [path.join(sdk, "cli", "devdesk-domain.mjs"), ...arguments_], {
    cwd: options.cwd ?? repo,
    encoding: "utf8",
    timeout: 120_000,
    env: { ...process.env, DEVDESK_PYTHON: python },
  });

const usageIndex = (arguments_) =>
  run("node", [path.join(sdk, "cli", "usage-index.mjs"), ...arguments_], {
    cwd: repo,
    encoding: "utf8",
    timeout: 60_000,
  });

test("the SDK's usage index reproduces what every kernel catalog stores", async () => {
  for (const catalog of ["kernel/core", "kernel/engineering"]) {
    const { stdout } = await usageIndex(["--check", path.join(repo, catalog)]);
    assert.match(stdout, /OK/);
  }
});

test("init, check and build produce a package the catalog negotiates", async () => {
  const directory = mkdtempSync(path.join(os.tmpdir(), "devdesk-sdk-test-"));
  try {
    const pack = path.join(directory, "try-shop");
    await cli(["init", "try-shop", pack]);
    await cli(["check", pack, "--sdk", sdk]);
    const out = path.join(directory, "try-shop.zip");
    await cli(["build", pack, "--sdk", sdk, "--out", out]);
    const { unzip } = await import("../../sdk/domain/lib/zip.mjs");
    const manifest = JSON.parse(unzip(readFileSync(out))["manifest.json"].toString("utf8"));
    assert.equal(manifest.format, 2);
    assert.equal(manifest.domain_protocol, 2);
    assert.deepEqual(manifest.requires.handlers, []);
    // A second build is byte-for-byte the same archive.
    const again = path.join(directory, "again.zip");
    await cli(["build", pack, "--sdk", sdk, "--out", again]);
    assert.ok(readFileSync(out).equals(readFileSync(again)), "the build is not reproducible");
  } finally {
    rmSync(directory, { recursive: true, force: true });
  }
});

test("check refuses a domain tool that names a handler the package does not register", async () => {
  const directory = mkdtempSync(path.join(os.tmpdir(), "devdesk-sdk-test-"));
  try {
    const pack = path.join(directory, "try-shop");
    await cli(["init", "try-shop", pack]);
    mkdirSync(path.join(pack, "catalog", "tools"), { recursive: true });
    const manifest = path.join(pack, "catalog", "manifest.json");
    writeFileSync(
      manifest,
      `${JSON.stringify(
        {
          schema_version: 1,
          bundle_version: "try-shop-1",
          types: { tool: ["tools/try-shop.validate-v1.json"] },
          defaults: "defaults.json",
        },
        null,
        2,
      )}\n`,
    );
    writeFileSync(path.join(pack, "catalog", "defaults.json"), "{}\n");
    // The tool names the package's namespace but a handler its worker does not register.
    writeFileSync(
      path.join(pack, "catalog", "tools", "try-shop.validate-v1.json"),
      `${JSON.stringify(
        {
          type: "tool",
          identity: "try-shop.validate",
          revision: 1,
          name: "Validate",
          scope: "global",
          data: {
            qualified_name: "try-shop.validate",
            implementation: "domain",
            handler: "try-shop.missing",
            input_schema: { type: "object" },
            output_schema: { type: "object" },
          },
        },
        null,
        2,
      )}\n`,
    );
    await assert.rejects(
      cli(["check", pack, "--sdk", sdk]),
      /try-shop\.validate:1 需要本包命名空间下的名称和本包注册的处理器/,
    );
    // Registering the handler makes the package pass.
    const domain = path.join(pack, "domain.json");
    const declared = JSON.parse(readFileSync(domain, "utf8"));
    declared.runtime.handlers["try-shop.missing"] = {
      input_schema: { type: "object" },
      output_schema: { type: "object" },
    };
    writeFileSync(domain, `${JSON.stringify(declared, null, 2)}\n`);
    writeFileSync(
      path.join(pack, "runtime", "worker.mjs"),
      `export const handlers = {\n  "try-shop.ping": () => ({ pong: 1 }),\n  "try-shop.missing": () => ({ allowed: true }),\n};\n`,
    );
    await cli(["check", pack, "--sdk", sdk]);
  } finally {
    rmSync(directory, { recursive: true, force: true });
  }
});

// The kernel's domain worker bootstrap, run on the Node of this machine with a temporary entry.
import assert from "node:assert/strict";
import { spawnSync } from "node:child_process";
import { mkdtemp, rm, writeFile } from "node:fs/promises";
import os from "node:os";
import path from "node:path";
import test from "node:test";
import { fileURLToPath } from "node:url";

const bootstrap = fileURLToPath(
  new URL("../../crates/adapters/runtime/domain-worker.mjs", import.meta.url),
);

async function run(entry, request) {
  const dir = await mkdtemp(path.join(os.tmpdir(), "devdesk-domain-worker-"));
  try {
    const file = path.join(dir, "entry.mjs");
    await writeFile(file, entry);
    const result = spawnSync(process.execPath, [bootstrap, file], {
      input: request,
      encoding: "utf8",
    });
    assert.equal(result.status, 0, result.stderr);
    return JSON.parse(result.stdout);
  } finally {
    await rm(dir, { recursive: true, force: true });
  }
}

const describe = JSON.stringify({
  jsonrpc: "2.0",
  id: "7",
  method: "describe",
  params: {},
});
const invoke = (handler, params = {}) =>
  JSON.stringify({
    jsonrpc: "2.0",
    id: "7",
    method: "invoke",
    params: { handler, input: { a: 1 }, core: { head: "abc" }, ...params },
  });
const envelope = (rest) => ({ jsonrpc: "2.0", id: "7", ...rest });
const refusal = (code, message) =>
  envelope({ error: { code: -32000, message, data: { code, refused: true } } });
const failure = (message) =>
  envelope({
    error: { code: -32000, message, data: { code: "worker_failed" } },
  });

test("the handshake lists the handlers the entry exports, in order", async () => {
  const reply = await run(
    'export const handlers = { "sample.two": () => ({}), "sample.one": () => ({}) };',
    describe,
  );
  assert.deepEqual(
    reply,
    envelope({ result: { api: 2, handlers: ["sample.one", "sample.two"] } }),
  );
});

test("a handler receives its input and its kernel input; its result is the reply", async () => {
  const reply = await run(
    'export const handlers = { "sample.one": async function (input, core) { return { input, core, count: arguments.length }; } };',
    invoke("sample.one"),
  );
  assert.deepEqual(
    reply,
    envelope({
      result: { input: { a: 1 }, core: { head: "abc" }, count: 2 },
    }),
  );
});

test("a refusal carries the code and message of the handler", async () => {
  const entry = `export const handlers = {
    "sample.coded": () => { throw Object.assign(new Error("订单结构不合法"), { code: "order_invalid" }); },
    "sample.plain": () => { throw new Error("boom"); },
    "sample.thrown": () => { throw "oops"; },
    "sample.odd": () => { throw Object.assign(new Error("odd"), { code: "Not Valid" }); },
  };`;
  for (const [handler, code, message] of [
    ["sample.coded", "order_invalid", "订单结构不合法"],
    ["sample.plain", "handler_failed", "boom"],
    ["sample.thrown", "handler_failed", "oops"],
    ["sample.odd", "handler_failed", "odd"],
  ]) {
    assert.deepEqual(await run(entry, invoke(handler)), refusal(code, message));
  }
});

test("what is not the handler's doing is a worker failure", async () => {
  const entry = 'export const handlers = { "sample.one": () => ({}) };';
  assert.deepEqual(
    await run(entry, invoke("sample.other")),
    failure("the entry has no handler sample.other"),
  );
  assert.deepEqual(
    await run(
      entry,
      JSON.stringify({ jsonrpc: "2.0", id: "7", method: "reset", params: {} }),
    ),
    failure("unknown method reset"),
  );
  for (const broken of [
    "export const handlers = null;",
    "export const handlers = [];",
    "export const other = {};",
    'export const handlers = { "sample.one": 1 };',
    "this is not javascript",
  ]) {
    const reply = await run(broken, invoke("sample.one"));
    assert.equal(reply.error.data.code, "worker_failed", broken);
    assert.equal(reply.error.data.refused, undefined, broken);
  }
});

test("a result that cannot be serialized is the handler's failure", async () => {
  const reply = await run(
    'export const handlers = { "sample.one": () => BigInt(7) };',
    invoke("sample.one"),
  );
  assert.equal(reply.error.data.refused, true);
  assert.equal(reply.error.data.code, "handler_failed");
  assert.match(reply.error.message, /BigInt/);
});

test("a request that is not JSON gets one failure without an id", async () => {
  // Without a request there is no id to answer with; the Host reports the mismatch.
  const broken = await run("export const handlers = {};", "not json\n");
  assert.equal(Object.hasOwn(broken, "id"), false);
  assert.deepEqual(broken.error.data, { code: "worker_failed" });
});

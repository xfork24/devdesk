import assert from "node:assert/strict";
import { mkdir, mkdtemp, rm, writeFile } from "node:fs/promises";
import os from "node:os";
import path from "node:path";
import test from "node:test";
import { differences, violations } from "../../scripts/check/check-web-boundaries.mjs";

async function fixture() {
  const root = await mkdtemp(path.join(os.tmpdir(), "devdesk-web-boundaries-"));
  const write = async (relative, text) => {
    await mkdir(path.dirname(path.join(root, relative)), { recursive: true });
    await writeFile(path.join(root, relative), text);
  };
  await write("web/src/keep.ts", 'import "./local";\nimport react from "react";\nexport * from "./other";\n');
  await write("web/src/bad.ts", 'import x from "../../plugins/shop/web/entry";\n');
  await write("plugins/shop/web/entry.tsx",
    'import { App } from "../../../web/src/App";\nimport { useDomain } from "../../../web/src/domain-sdk";\n');
  await write("plugins/shop/web/page.ts", 'import "./entry";\nimport react from "react";\n');
  await write("plugins/shop/web/src/deep.ts",
    'const load = () => import("../../../../web/src/components/x");\n');
  await write("plugins/shop/web/node_modules/pkg/index.js", 'import "../../../../web/src/skip";\n');
  return root;
}

test("only imports that cross a boundary are reported", async () => {
  const root = await fixture();
  try {
    assert.deepEqual(await violations(root), {
      "core-to-domain": ["web/src/bad.ts -> plugins/shop/web/entry"],
      "domain-to-core": [
        "plugins/shop/web/entry.tsx -> web/src/App",
        "plugins/shop/web/entry.tsx -> web/src/domain-sdk",
        "plugins/shop/web/src/deep.ts -> web/src/components/x",
      ],
    });
  } finally {
    await rm(root, { recursive: true, force: true });
  }
});

test("a domain without web source and a missing core directory report nothing", async () => {
  const root = await mkdtemp(path.join(os.tmpdir(), "devdesk-web-boundaries-"));
  try {
    assert.deepEqual(await violations(root), { "core-to-domain": [], "domain-to-core": [] });
  } finally {
    await rm(root, { recursive: true, force: true });
  }
});

test("new imports fail and removed imports must leave the record", () => {
  const recorded = { "core-to-domain": [], "domain-to-core": ["a -> b", "a -> c"] };
  assert.deepEqual(differences(recorded, { "core-to-domain": [], "domain-to-core": ["a -> b"] }),
    { "core-to-domain": { added: [], removed: [] }, "domain-to-core": { added: [], removed: ["a -> c"] } });
  assert.deepEqual(differences(recorded, { "core-to-domain": ["x -> y"], "domain-to-core": ["a -> b", "a -> d"] }),
    { "core-to-domain": { added: ["x -> y"], removed: [] },
      "domain-to-core": { added: ["a -> d"], removed: ["a -> c"] } });
  assert.deepEqual(differences(recorded, recorded),
    { "core-to-domain": { added: [], removed: [] }, "domain-to-core": { added: [], removed: [] } });
});

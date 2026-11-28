// A domain package is a directory. domain.json says what it is, catalog/ holds its
// configuration sources, and the other files it carries are taken as they are.
import { existsSync, readdirSync, readFileSync } from "node:fs";
import path from "node:path";
import { createHash } from "node:crypto";
import { Failure, readJson } from "./sdk.mjs";
import { usageIndex } from "./usage-index.mjs";

// Where a source keeps its references to other sources.
export const REFERENCE_FIELDS = [
  "capability_ref",
  "role_ref",
  "model_ref",
  "harness_ref",
  "sandbox_ref",
  "model_catalog_ref",
  "spec_refs",
  "skills",
  "tools",
  "model_providers",
  "root_workflow_refs",
];

const json = (value) => Buffer.from(`${JSON.stringify(value, null, 2)}\n`);
const byName = (a, b) => (a < b ? -1 : a > b ? 1 : 0);
const posix = (file) => file.split(path.sep).join("/");

function addTree(files, root, prefix, base) {
  if (!existsSync(root)) throw new Failure(`目录不存在：${root}`);
  for (const entry of readdirSync(root, { withFileTypes: true })) {
    if (entry.name.startsWith(".") || entry.name === "node_modules") continue;
    const file = path.join(root, entry.name);
    if (entry.isDirectory()) addTree(files, file, prefix, base);
    else if (entry.isFile())
      files.set(path.posix.join(prefix, posix(path.relative(base, file))), file);
  }
}

// The package's files on disk, by their path inside the package. `assets` bring in what is
// built elsewhere, such as a Web bundle, under a fixed prefix.
export function collectFiles(directory, assets = []) {
  const files = new Map();
  addTree(files, directory, "", directory);
  for (const asset of assets)
    addTree(files, path.resolve(directory, asset.directory), asset.prefix, path.resolve(directory, asset.directory));
  files.delete("domain.json");
  files.delete("README.md");
  return files;
}

// The configuration sources as the kernel reads them: one file per source revision with
// the bodies inlined, a bundle manifest, the defaults and the usage index.
export function readCatalog(directory, types) {
  const root = path.join(directory, "catalog");
  const manifest = readJson(path.join(root, "manifest.json"));
  const files = new Map();
  const sources = [];
  const owned = new Set(["manifest.json", manifest.defaults]);
  const byType = Object.fromEntries(types.map((type) => [type, []]));
  const identities = new Set();
  for (const [type, listed] of Object.entries(manifest.types)) {
    if (!types.includes(type))
      throw new Failure(`catalog/manifest.json 列出了未知的源类型 ${type}`);
    for (const relative of listed) {
      const file = path.join(root, relative);
      owned.add(relative);
      const source = readJson(file);
      // A long body lives next to its source and is inlined from a `*_file` field.
      for (const [field, value] of Object.entries(source.data)) {
        if (!field.endsWith("_file")) continue;
        const body = path.resolve(path.dirname(file), value);
        owned.add(posix(path.relative(root, body)));
        delete source.data[field];
        source.data[field.slice(0, -"_file".length)] = readFileSync(body, "utf8");
      }
      if (source.type !== type)
        throw new Failure(`catalog/${relative} 的类型是 ${source.type}，清单把它列在 ${type} 下`);
      const revision = `${source.identity}:${source.revision}`;
      if (!identities.add(revision))
        throw new Failure(`catalog 里有两份 ${revision}`);
      sources.push(source);
      byType[type].push(`${source.identity}-v${source.revision}.json`);
      files.set(`configurations/${source.identity}-v${source.revision}.json`, json(source));
    }
  }
  if (manifest.usage_index) owned.add(manifest.usage_index);
  const present = new Map();
  addTree(present, root, "", root);
  for (const name of present.keys())
    if (!owned.has(name)) throw new Failure(`catalog/${name} 没有被 manifest.json 收录`);
  files.set(
    "configurations/manifest.json",
    json({
      schema_version: 1,
      bundle_version: manifest.bundle_version,
      types: byType,
      defaults: "defaults.json",
      usage_index: "usage.json",
    }),
  );
  files.set("configurations/defaults.json", json(readJson(path.join(root, manifest.defaults))));
  const usage = { schema_version: 1, references: usageIndex(sources) };
  files.set("configurations/usage.json", json(usage));
  return {
    sources,
    files,
    usage,
    declaredUsage: manifest.usage_index
      ? readJson(path.join(root, manifest.usage_index))
      : null,
  };
}

// The manifest the Host installs, with what a package leaves to its build filled in: the
// format, the platform handlers its tools reach, and the digest of every file it carries.
export function assemble(directory, sdk) {
  const declared = readJson(path.join(directory, "domain.json"));
  const files = new Map(
    [...collectFiles(directory, declared.assets)].map(([name, file]) => [name, readFileSync(file)]),
  );
  let catalog = { sources: [], usage: null, declaredUsage: null };
  if (files.has("catalog/manifest.json")) {
    const types = readJson(path.join(sdk.schemas, "template-manifest.schema.json"))
      .properties.types.required;
    catalog = readCatalog(directory, types);
    for (const name of [...files.keys()]) if (name.startsWith("catalog/")) files.delete(name);
    for (const [name, body] of catalog.files) files.set(name, body);
  }
  const requires = declared.requires ?? {};
  const { assets, ...described } = declared;
  const manifest = {
    format: sdk.catalog.package_format,
    id: declared.id,
    version: declared.version,
    domain_protocol: sdk.catalog.domain_protocol,
    requires: {
      host: requires.host ?? sdk.catalog.host_version,
      handlers: adoptedHandlers(catalog.sources, sdk).sort(byName),
      extensions: [...(requires.extensions ?? [])].sort(byName),
      ...(requires.rpc ? { rpc: [...requires.rpc].sort(byName) } : {}),
    },
    ...Object.fromEntries(
      ["entity_types", "extensions", "runtime", "ui"].filter((key) => key in described).map((key) => [key, described[key]]),
    ),
    files: Object.fromEntries(
      [...files].map(([name, body]) => [
        name,
        createHash("sha256").update(body).digest("hex"),
      ]),
    ),
  };
  files.set("manifest.json", json(manifest));
  return { manifest, files, sources: catalog.sources, usage: catalog.usage, declaredUsage: catalog.declaredUsage };
}

// The platform handlers the package reaches through the tools it publishes or adopts.
// Kernel tools are known to the SDK; the package's own tools are in its catalog.
function adoptedHandlers(sources, sdk) {
  const tools = new Map(sdk.tools);
  for (const source of sources)
    if (source.type === "tool") tools.set(`${source.identity}:${source.revision}`, source.data);
  const references = new Set();
  const visit = (value) => {
    if (Array.isArray(value)) {
      value.forEach(visit);
    } else if (value && typeof value === "object") {
      for (const [key, item] of Object.entries(value)) {
        if (key === "tool_ref" && typeof item === "string") references.add(item);
        else if (["tools", "tool_refs"].includes(key) && Array.isArray(item))
          item.forEach((tool) => typeof tool === "string" && references.add(tool));
        else visit(item);
      }
    }
  };
  sources.forEach(visit);
  const handlers = new Set();
  for (const reference of references) {
    const tool = tools.get(reference);
    if (tool?.implementation === "platform") handlers.add(tool.qualified_name);
  }
  return [...handlers];
}

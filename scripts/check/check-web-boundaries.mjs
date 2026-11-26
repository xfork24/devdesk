#!/usr/bin/env node
// Web import boundaries: Web Core source never imports a domain package, and a domain's
// web source never imports Web Core source. Current violations are recorded next to this
// script. A new one fails the check; a removed one must also leave the record.
import { existsSync } from "node:fs";
import { readdir, readFile } from "node:fs/promises";
import { createRequire } from "node:module";
import path from "node:path";
import { fileURLToPath } from "node:url";

const root = fileURLToPath(new URL("../..", import.meta.url));
const require = createRequire(path.join(root, "web", "package.json"));
// [name, source directory, directories the source must not import]; * matches one level.
export const boundaries = [
  ["core-to-domain", "web/src", ["plugins"]],
  ["domain-to-core", "plugins/*/web", ["web/src", "web/dist"]],
];

async function expand(directory, parts) {
  if (!parts.length) return existsSync(directory) ? [directory] : [];
  const [part, ...rest] = parts;
  if (part !== "*") return expand(path.join(directory, part), rest);
  if (!existsSync(directory)) return [];
  const found = [];
  for (const entry of await readdir(directory, { withFileTypes: true })) {
    if (entry.isDirectory() && !entry.name.startsWith(".")) {
      found.push(...await expand(path.join(directory, entry.name), rest));
    }
  }
  return found;
}

async function sourceFiles(directory) {
  const found = [];
  for (const entry of await readdir(directory, { withFileTypes: true })) {
    if (["node_modules", "dist"].includes(entry.name)) continue;
    const full = path.join(directory, entry.name);
    if (entry.isDirectory()) found.push(...await sourceFiles(full));
    else if (/\.tsx?$/.test(entry.name)) found.push(full);
  }
  return found.sort();
}

// Static imports, re-exports, require and dynamic import(): every path a module reaches.
function importedPaths(text) {
  return require("typescript").preProcessFile(text, true, true).importedFiles.map(
    (reference) => reference.fileName);
}

export async function violations(root, definitions = boundaries) {
  const found = {};
  for (const [name, source, forbidden] of definitions) {
    found[name] = [];
    const prefixes = forbidden.map((directory) => path.join(root, directory) + path.sep);
    for (const directory of await expand(root, source.split("/"))) {
      for (const file of await sourceFiles(directory)) {
        for (const specifier of importedPaths(await readFile(file, "utf8"))) {
          if (!specifier.startsWith(".")) continue;
          const target = path.resolve(path.dirname(file), specifier);
          if (!prefixes.some((prefix) => target.startsWith(prefix))) continue;
          found[name].push(`${path.relative(root, file)} -> ${path.relative(root, target)}`);
        }
      }
    }
    found[name].sort();
  }
  return found;
}

export function differences(recorded, current) {
  return Object.fromEntries(Object.keys(current).map((name) => [name, {
    added: current[name].filter((violation) => !recorded[name]?.includes(violation)),
    removed: (recorded[name] ?? []).filter((violation) => !current[name].includes(violation)),
  }]));
}

if (process.argv[1] === fileURLToPath(import.meta.url)) {
  const recorded = JSON.parse(await readFile(new URL("./web-boundaries.json", import.meta.url), "utf8"));
  const current = await violations(root);
  const changes = differences(recorded, current);
  const changed = Object.values(changes).some(({ added, removed }) => added.length || removed.length);
  if (changed) {
    for (const [name, { added, removed }] of Object.entries(changes)) {
      for (const violation of added) console.error(`web-boundaries: new ${name} import: ${violation}`);
      for (const violation of removed) {
        console.error(`web-boundaries: recorded ${name} import is gone: ${violation}`);
      }
    }
    console.error("web-boundaries: update scripts/check/web-boundaries.json to the current imports");
    process.exit(1);
  }
  const summary = Object.entries(current).map(([name, list]) => `${name} ${list.length}`);
  console.log(`web-boundaries: OK — ${summary.join(", ")}`);
}

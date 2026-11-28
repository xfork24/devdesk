// One-shot: compute a configuration catalog's reverse-reference index.
//
//   usage-index.mjs <catalog-directory>          print the index JSON
//   usage-index.mjs --check <catalog-directory>  exit 0 when the stored index is current
//   usage-index.mjs --write <catalog-directory>  write the index to the manifest's file
//
// The index is the one implementation the kernel's checks and a package's build share.
import { readFileSync, writeFileSync } from "node:fs";
import path from "node:path";
import process from "node:process";
import { usageIndex } from "../lib/usage-index.mjs";

const [command, target] = process.argv.slice(2);
const inline = (file) => {
  const source = JSON.parse(readFileSync(file, "utf8"));
  // A long body lives next to its source in a `*_file` field. A JSON body is inlined
  // parsed, so a schema keeps its shape; everything else is the file's text.
  for (const [field, value] of Object.entries(source.data))
    if (field.endsWith("_file")) {
      delete source.data[field];
      const text = readFileSync(path.resolve(path.dirname(file), value), "utf8");
      source.data[field.slice(0, -"_file".length)] = text.trimStart().startsWith("{")
        ? JSON.parse(text)
        : text;
    }
  return source;
};
let index;
let stored = null;
let root = null;
if (command === "--stdin") {
  index = {
    schema_version: 1,
    references: usageIndex(JSON.parse(readFileSync(0, "utf8"))),
  };
} else {
  if (!target) {
    console.error("用法：usage-index.mjs [--check|--write|--stdin] <catalog-directory>");
    process.exit(2);
  }
  root = path.resolve(target);
  const manifest = JSON.parse(readFileSync(path.join(root, "manifest.json"), "utf8"));
  stored = manifest.usage_index ? path.join(root, manifest.usage_index) : null;
  index = {
    schema_version: 1,
    references: usageIndex(
      Object.values(manifest.types)
        .flat()
        .map((file) => inline(path.join(root, file))),
    ),
  };
}
if (command === "--stdin") {
  console.log(JSON.stringify(index, null, 2));
} else {
  const current = stored
    ? JSON.parse(readFileSync(stored, "utf8"))
    : { schema_version: 1, references: {} };
  if (command === "--write") {
    if (!stored) throw new Error(`${root} 的 manifest.json 没有声明 usage_index`);
    writeFileSync(stored, `${JSON.stringify(index, null, 2)}\n`);
    console.log(`usage-index: 已写入 ${path.relative(process.cwd(), stored)}`);
  } else if (command === "--check") {
    const same = JSON.stringify(index) === JSON.stringify(current);
    console.log(
      same
        ? `usage-index: OK — ${root}，${Object.keys(index.references).length} 条`
        : `usage-index: 已过期 — ${root}；运行 --write 刷新`,
    );
    process.exitCode = same ? 0 : 1;
  } else {
    console.log(JSON.stringify(index, null, 2));
  }
}

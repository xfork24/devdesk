#!/usr/bin/env node
// devdesk-domain: init, check, build and verify a domain package with the SDK artifact.
//
//   devdesk-domain init <id>                 write a directory skeleton
//   devdesk-domain check [DIR] [--write]     schema, namespace, closure, handlers; refresh
//                                            the usage index with --write
//   devdesk-domain build [DIR] [--out FILE]  check, then write the ZIP
//   devdesk-domain verify [DIR] --url URL    read a running Host's catalog, check requires
//
// Build independence: this command needs the SDK artifact, the package directory, Node and
// Python (with jsonschema) for the schema checks. It needs no kernel sources.
import { spawnSync } from "node:child_process";
import { existsSync, mkdirSync, mkdtempSync, rmSync, writeFileSync } from "node:fs";
import path from "node:path";
import process from "node:process";
import { Failure, loadSdk, negotiate, readJson } from "../lib/sdk.mjs";
import { assemble, collectFiles } from "../lib/package.mjs";
import { zip } from "../lib/zip.mjs";

const fail = (message) => {
  console.error(`devdesk-domain: ${message}`);
  process.exitCode = 1;
};
const option = (name) => {
  const at = process.argv.indexOf(name);
  return at > 0 ? process.argv[at + 1] : undefined;
};
const flag = (name) => process.argv.includes(name);
// --asset DIR:PREFIX (repeatable) brings in what was built elsewhere. --strip ui removes the
// presentation target and its files, for a runtime-only package.
const assetOptions = () =>
  process.argv.flatMap((argument, at) =>
    argument === "--asset" ? [process.argv[at + 1]] : [],
  ).map((given) => {
    const split = given.lastIndexOf(":");
    if (split <= 0) throw new Failure(`--asset 需要 目录:前缀 形式：${given}`);
    return { directory: given.slice(0, split), prefix: given.slice(split + 1) };
  });
const stripped = () =>
  ["ui"].filter((block) => flag(`--no-${block}`));
const [command, ...rest] = process.argv.slice(2);
const positional = () => rest.find((argument, at) => !argument.startsWith("--") && !(at && rest[at - 1].startsWith("--")));
const ordered = (left, right) => (left < right ? -1 : left > right ? 1 : 0);

// The schema checks run in one Python process; the SDK carries the same schemas the Host
// validates a package with at install.
const schemaChecks = (sdk, items) => {
  const temporary = mkdtempSync(path.join(process.env.TEMPDIR ?? process.env.TMPDIR ?? "/tmp", "devdesk-domain-"));
  try {
    const run = spawnSync(
      process.env.DEVDESK_PYTHON ?? "python3",
      [path.join(sdk.root, "lib", "validate.py"), sdk.schemas],
      { encoding: "utf8", timeout: 60_000, input: JSON.stringify(items) },
    );
    if (run.error) throw new Failure(`无法运行 Schema 校验器：${run.error.message}`);
    if (run.status !== 0) throw new Failure(`Schema 校验器失败：${run.stderr}`);
    return JSON.parse(run.stdout);
  } finally {
    rmSync(temporary, { recursive: true, force: true });
  }
};

// Tools the package publishes or adopts, with each tool's data: its own and the kernel's.
function toolsOf(assembled, sdk) {
  const tools = new Map(sdk.tools);
  for (const source of assembled.sources)
    if (source.type === "tool") tools.set(`${source.identity}:${source.revision}`, source.data);
  const references = new Set();
  const visit = (value) => {
    if (Array.isArray(value)) value.forEach(visit);
    else if (value && typeof value === "object")
      for (const [key, item] of Object.entries(value)) {
        if (key === "tool_ref" && typeof item === "string") references.add(item);
        else if (["tools", "tool_refs"].includes(key) && Array.isArray(item))
          item.forEach((tool) => typeof tool === "string" && references.add(tool));
        else visit(item);
      }
  };
  // A published tool is itself a candidate, not only a reference to one.
  for (const source of assembled.sources)
    if (source.type === "tool") references.add(`${source.identity}:${source.revision}`);
  assembled.sources.forEach(visit);
  // A Set iterates in insertion order; sort so the messages are stable.
  return [...references]
    .sort(ordered)
    .map((reference) => [reference, tools.get(reference)])
    .filter(([, data]) => data);
}

// What the Host checks again at install, said here so a build fails locally:
// - a domain tool is in the package's namespace and names a handler the package registers;
// - a platform tool names a handler this kernel has, and an engineering tool only with the
//   group it belongs to required.
function toolProblems(assembled, sdk) {
  const problems = [];
  const prefix = `${assembled.manifest.id}.`;
  const handlers = new Set(Object.keys(assembled.manifest.runtime?.handlers ?? {}));
  const groups = new Set(assembled.manifest.requires.extensions);
  const offered = new Set(
    sdk.catalog.extensions
      .filter((group) => groups.has(`${group.id}/${group.version}`))
      .flatMap((group) => group.handlers),
  );
  for (const [reference, data] of toolsOf(assembled, sdk)) {
    if (data.implementation === "domain") {
      if (!data.qualified_name.startsWith(prefix) || !handlers.has(data.handler))
        problems.push(
          `工具 ${reference} 需要本包命名空间下的名称和本包注册的处理器（${data.handler ?? "缺少 handler"}）`,
        );
    } else if (data.implementation === "platform") {
      if (!sdk.catalog.platform_handlers.includes(data.qualified_name))
        problems.push(`领域包发布了本 Host 没有的平台处理器 ${data.qualified_name} 上的工具`);
      if (
        offered.size &&
        !offered.has(data.qualified_name) &&
        sdk.catalog.extensions.some((group) => group.handlers.includes(data.qualified_name))
      )
        problems.push(
          `平台工具 ${data.qualified_name} 由扩展组 ${sdk.catalog.extensions
            .find((group) => group.handlers.includes(data.qualified_name))
            ?.id} 提供，领域包没有声明依赖`,
        );
    }
  }
  return problems;
}

try {
  if (command === "init") {
    const id = positional();
    if (!id || !/^[A-Za-z0-9][A-Za-z0-9._-]{0,139}$/.test(id))
      throw new Failure("init 需要一个包 ID（字母数字与 . _ -）");
    const directory = path.resolve(rest.find((argument) => !argument.startsWith("--") && argument !== id) ?? id);
    if (existsSync(path.join(directory, "domain.json")))
      throw new Failure(`${directory} 已经是一个领域包`);
    mkdirSync(path.join(directory, "runtime"), { recursive: true });
    const handler = `${id}.ping`;
    writeFileSync(
      path.join(directory, "domain.json"),
      `${JSON.stringify(
        {
          id,
          version: "0.1.0",
          runtime: {
            entry: "runtime/worker.mjs",
            handlers: {
              [handler]: {
                input_schema: { type: "object", additionalProperties: false },
                output_schema: {
                  type: "object",
                  required: ["pong"],
                  additionalProperties: false,
                  properties: { pong: { const: handler } },
                },
              },
            },
          },
        },
        null,
        2,
      )}\n`,
    );
    writeFileSync(
      path.join(directory, "runtime", "worker.mjs"),
      `// ${id} 的处理器。入口是纯模块，只导出 handlers；拒绝时抛出带 code 的 Error。\nexport const handlers = {\n  "${handler}": () => ({ pong: "${handler}" }),\n};\n`,
    );
    console.log(`devdesk-domain: 骨架已写入 ${directory}`);
  } else if (command === "check" || command === "build") {
    const sdk = loadSdk({ flag: option("--sdk") });
    const directory = path.resolve(positional() ?? ".");
    const assembled = assemble(directory, sdk, { assets: assetOptions(), strip: stripped() });
    // Format and protocol are answered before the schema, so a package written for another
    // generation learns what this SDK speaks instead of a list of schema errors.
    const problems = [];
    if (assembled.manifest.format !== sdk.catalog.package_format)
      problems.push(
        `领域包格式 ${assembled.manifest.format}；本 SDK 生成格式 ${sdk.catalog.package_format}`,
      );
    if (assembled.manifest.domain_protocol !== sdk.catalog.domain_protocol)
      problems.push(
        `领域包协议 ${assembled.manifest.domain_protocol}；本 SDK 生成协议 ${sdk.catalog.domain_protocol}`,
      );
    const asked = [
      { schema: "domain-package.schema.json", instance: assembled.manifest, label: "manifest" },
    ];
    if (assembled.manifest.entity_types)
      asked.push({
        schema: "domain-entity-types.schema.json",
        instance: JSON.parse(assembled.files.get(assembled.manifest.entity_types).toString("utf8")),
        label: "entity_types",
      });
    if (assembled.declaredUsage && !deepEqual(assembled.usage, assembled.declaredUsage))
      problems.push("catalog 的引用索引已过期；用 devdesk-domain check --write 刷新后重新构建");
    for (const problem of schemaChecks(sdk, asked)) if (problem) problems.push(problem);
    problems.push(...negotiate(sdk.catalog, assembled.manifest.requires), ...toolProblems(assembled, sdk));
    if (problems.length) throw new Failure(`\n  - ${problems.join("\n  - ")}`);
    if (command === "check") {
      if (flag("--write") && assembled.declaredUsage) {
        const written = path.join(
          directory,
          "catalog",
          readJson(path.join(directory, "catalog", "manifest.json")).usage_index,
        );
        writeFileSync(written, `${JSON.stringify(assembled.usage, null, 2)}\n`);
        console.log(`devdesk-domain: 引用索引已刷新 ${path.relative(directory, written)}`);
      }
      console.log(
        `devdesk-domain: OK — ${assembled.sources.length} 个源定义，${assembled.manifest.requires.handlers.length} 个平台处理器，${assembled.manifest.requires.extensions.length} 个扩展组`,
      );
    } else {
      const target = path.resolve(option("--out") ?? `${assembled.manifest.id}-${assembled.manifest.version}.zip`);
      const archive = zip(Object.fromEntries(assembled.files));
      writeFileSync(target, archive);
      console.log(
        `devdesk-domain: ${target} — ${assembled.files.size} 个文件，${archive.length} 字节`,
      );
    }
  } else if (command === "verify") {
    const sdk = loadSdk({ flag: option("--sdk") });
    const url = option("--url");
    if (!url) throw new Failure("verify 需要 --url <host>");
    const directory = path.resolve(positional() ?? ".");
    const { manifest } = assemble(directory, sdk);
    const response = await fetch(new URL("/capabilities", url));
    if (!response.ok) throw new Failure(`Host 返回 HTTP ${response.status}`);
    const problems = negotiate(await response.json(), manifest.requires);
    if (problems.length) throw new Failure(`\n  - ${problems.join("\n  - ")}`);
    console.log(`devdesk-domain: ${url} 提供此包需要的全部能力`);
  } else {
    throw new Failure(
      "用法：devdesk-domain init <id> | check [DIR] [--write] | build [DIR] [--out FILE] | verify [DIR] --url <host>",
    );
  }
} catch (error) {
  fail(error instanceof Failure ? error.message : (error.stack ?? String(error)));
}

function deepEqual(left, right) {
  return JSON.stringify(left) === JSON.stringify(right);
}

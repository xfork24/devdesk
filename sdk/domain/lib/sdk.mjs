// Where the SDK is, and what it knows about the kernel it belongs to.
//
// The SDK artifact is one directory: this package (cli/, lib/, package.json), schemas/,
// kernel/ (the kernel's catalogs), worker/bootstrap.mjs and catalog.json. In the kernel's
// own checkout the same pieces are the sdk/ directory, the repository's schemas/ and kernel/,
// and the bootstrap module in the adapters crate.
import { existsSync, readFileSync } from "node:fs";
import path from "node:path";
import { fileURLToPath } from "node:url";

export const PACKAGE_FORMAT = 2;
export const DOMAIN_PROTOCOL = 2;

export class Failure extends Error {}

// The catalog a Host prints with `devdesk-host capabilities --print`.
export function loadSdk({ flag, env = process.env } = {}) {
  const here = path.resolve(fileURLToPath(new URL("../..", import.meta.url)));
  for (const candidate of [flag, env.DEVDESK_SDK, here, process.cwd()]) {
    if (candidate && existsSync(path.join(path.resolve(candidate), "catalog.json"))) {
      const root = path.resolve(candidate);
      const beside = (...names) =>
        names
          .flatMap((name) => [0, 1, 2].map((up) => path.join(root, ...Array(up).fill(".."), name)))
          .find((file) => existsSync(file));
      const schemas = beside("schemas");
      const kernel = beside("kernel");
      const bootstrap = beside(
        path.join("worker", "bootstrap.mjs"),
        path.join("crates", "adapters", "runtime", "domain-worker.mjs"),
      );
      if (!schemas || !kernel || !bootstrap)
        throw new Failure(`SDK 目录 ${root} 不完整：需要 schemas/、kernel/ 和处理器引导模块`);
      const catalog = JSON.parse(readFileSync(path.join(root, "catalog.json"), "utf8"));
      const tools = new Map();
      // The kernel publishes its handlers' tools, and the engineering extension the tools of
      // its groups. A package adopts them; a build maps an adopted tool to its handler.
      for (const bundle of ["core", "engineering"]) {
        const manifest = JSON.parse(
          readFileSync(path.join(kernel, bundle, "manifest.json"), "utf8"),
        );
        for (const file of manifest.types.tool) {
          const source = JSON.parse(
            readFileSync(path.join(kernel, bundle, file), "utf8"),
          );
          tools.set(`${source.identity}:${source.revision}`, source.data);
        }
      }
      return { root, schemas, kernel, bootstrap, catalog, tools };
    }
  }
  throw new Failure(
    "找不到 SDK 目录（其中应有 catalog.json）；用 --sdk 或环境变量 DEVDESK_SDK 指定",
  );
}

export const readJson = (file) => JSON.parse(readFileSync(file, "utf8"));

// Every input an extension group lets a package declare for its handlers.
export function coreInputs(catalog) {
  return [
    ...catalog.core_inputs,
    ...catalog.extensions.flatMap((group) => group.core_inputs),
  ];
}

// What a Host provides for the capability catalog of a package's `requires`.
export function negotiate(catalog, requires) {
  const problems = [];
  const missing = (wanted, provided, what) => {
    const absent = wanted.filter((name) => !provided.includes(name));
    if (absent.length) problems.push(`本 Host 没有这些${what}：${absent.join("、")}`);
  };
  if (requires.host && !atLeast(catalog.host_version, requires.host))
    problems.push(
      `领域包要求 Host 版本不低于 ${requires.host}；本 Host 是 ${catalog.host_version}`,
    );
  missing(requires.handlers ?? [], catalog.platform_handlers, "平台处理器");
  missing(
    requires.extensions ?? [],
    catalog.extensions.map((group) => `${group.id}/${group.version}`),
    "扩展组",
  );
  missing(requires.rpc ?? [], catalog.rpc, "RPC");
  return problems;
}

// Numeric `major.minor.patch` comparison; anything else cannot be negotiated.
function atLeast(actual, minimum) {
  const numbers = (version) => {
    const core = version.split(/[-+]/)[0].split(".");
    return core.length <= 3 && core.every((part) => /^\d+$/.test(part))
      ? core.map(Number)
      : null;
  };
  const left = numbers(actual);
  const right = numbers(minimum);
  if (!left || !right) return false;
  for (const index of [0, 1, 2]) {
    const difference = (left[index] ?? 0) - (right[index] ?? 0);
    if (difference) return difference > 0;
  }
  return true;
}

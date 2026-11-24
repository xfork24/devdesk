import { readFileSync } from "node:fs";
import { fileURLToPath } from "node:url";

const discover = "software-development.tests.discover";
const planBranch = "software-development.branch.plan";
const connect = "software-development.project.connect";
const createPullRequest = "software-development.pull-request.create";

function regression(runner, files, argv = null) {
  return {
    runner,
    files,
    timeout_seconds: 1800,
    command: argv && { argv, cwd: ".", environment: {} },
    preparation: [],
  };
}

export function discoverTests({ revision, files, paths, omitted, matches }) {
  const result = {
    revision,
    source: "unconfigured",
    files: [],
    notes: omitted.map((path) => `${path} 超出识别大小限制，使用默认测试入口。`),
    configuration: {
      language: "JavaScript / Node.js",
      independent_runner: "node_test",
      regression: null,
    },
  };
  function command(file, language, argv, note) {
    result.source = "repository";
    result.files = [file];
    result.notes = [note];
    result.configuration.language = language;
    result.configuration.regression = regression("command", [file], argv);
    return result;
  }
  if (files[".devdesk/tests.json"] !== undefined) {
    let configured;
    try {
      configured = JSON.parse(files[".devdesk/tests.json"]);
    } catch {
      throw new Error(".devdesk/tests.json 格式无效");
    }
    if (configured?.regression?.image && !/^sha256:[0-9a-f]{64}$/.test(configured.regression.image)) {
      throw new Error(".devdesk/tests.json 的镜像标识无效");
    }
    if (configured?.regression) delete configured.regression.image;
    return {
      ...result,
      configuration: configured,
      source: "repository",
      files: [".devdesk/tests.json"],
      notes: [],
    };
  }
  if (files["package.json"] !== undefined) {
    try {
      const script = JSON.parse(files["package.json"])?.scripts?.test;
      if (typeof script === "string" && script.trim() && !script.includes("no test specified")) {
        return command("package.json", "JavaScript / TypeScript / Node.js", ["npm", "test"],
          "项目依赖需在运行环境中准备；识别过程不会执行仓库脚本。");
      }
    } catch {
      result.notes.push("package.json 格式无效，使用可识别的其他入口。");
    }
  }
  if (files["Cargo.toml"] !== undefined) {
    result.configuration.language = "Rust";
    if (matches.rust_tests || files["Cargo.toml"].includes("[[test]]")
      || paths.some((path) => path.endsWith(".rs") && path.split("/").includes("tests"))) {
      return command("Cargo.toml", "Rust", ["cargo", "test", "--workspace"],
        "运行环境需要 Rust 工具链与项目依赖。");
    }
    return result;
  }
  if (files["go.mod"] !== undefined) {
    result.configuration.language = "Go";
    return paths.some((path) => path.endsWith("_test.go"))
      ? command("go.mod", "Go", ["go", "test", "./..."], "运行环境需要 Go 工具链与项目依赖。")
      : result;
  }
  const pythonManifests = ["pytest.ini", "pyproject.toml", "setup.cfg"];
  if (pythonManifests.some((path) => files[path] !== undefined) || paths.some((path) => path.endsWith(".py"))) {
    result.configuration.language = "Python 3";
    result.configuration.independent_runner = "python_unittest";
    if (files["pytest.ini"] !== undefined || files["pyproject.toml"]?.includes("[tool.pytest.ini_options]")
      || files["setup.cfg"]?.includes("[tool:pytest]")) {
      return command(pythonManifests.find((path) => files[path] !== undefined), "Python 3",
        ["python3", "-m", "pytest"], "运行环境需要 pytest 与项目依赖。");
    }
    const unittest = paths.filter((path) => {
      const name = path.split("/").at(-1);
      return name.startsWith("test") && name.endsWith(".py") && (path.startsWith("tests/") || !path.includes("/"));
    }).slice(0, 100);
    if (unittest.length) {
      result.configuration.regression = regression("python_unittest", unittest);
      result.source = "repository";
      result.files = unittest;
      result.notes = ["自动发现 Python unittest 测试文件。"];
    }
  }
  return result;
}

export function worktreeBranch({ kind, owner }) {
  const directory = { feature: "features", task: "tasks", candidate: "candidates" }[kind];
  if (!directory) throw new Error("unknown worktree kind");
  return { owner, kind, branch: `devdesk/${directory}/${owner}` };
}

export function connectProject(input) {
  let repository = input.repository.trim();
  if (repository.startsWith("https://github.com/")) {
    repository = repository.slice("https://github.com/".length);
  }
  repository = repository.replace(/\/$/, "").replace(/[.]git$/, "");
  if (!/^[A-Za-z0-9_.-]+\/[A-Za-z0-9_.-]+$/.test(repository)
      || repository.split("/").some((part) => part === "." || part === "..")) {
    throw new Error("Expected a GitHub owner/repository");
  }
  const mainline = (input.mainline ?? "main").trim();
  if (!mainline || mainline.startsWith("-") || /\s/.test(mainline)) {
    throw new Error("Invalid target branch");
  }
  return { repository, mainline };
}

export function pullRequestCreation({ request, marker }) {
  const projection = request.projection;
  const source = projection?.source_revision ?? request.candidate;
  const publication = projection
    ? "A code-only Feature delivery is used. Workflow validation changes are excluded at: "
      + projection.validation_paths.join(", ")
      + ". The original Feature and Task branches retain their materials."
    : "The Task source branch is used directly; its exact PR test-merge commit is retained in the owning DevDesk workflow.";
  return {
    title: "DevDesk delivery " + request.merge,
    head: request.head_branch,
    base: request.base_branch,
    body: [
      marker,
      "Source revision: `" + source + "`\nDelivery revision: `" + request.candidate
        + "`\nValidated target: `" + request.expected_target + "`",
      publication,
      "Reviews, exact test receipts and the final Human decision remain bound to these versions. Existing repository merge requirements apply.",
    ].join("\n\n"),
  };
}

if (process.argv[1] === fileURLToPath(import.meta.url)) {
  const request = JSON.parse(readFileSync(0, "utf8"));
  const handlers = new Map([
    [planBranch, ({ input }) => worktreeBranch(input)],
    ['software-development.delivery.authorize', ({ input }) => authorizeDelivery(input)],
    [connect, ({ input }) => connectProject(input)],
    [createPullRequest, ({ input }) => pullRequestCreation(input)],
    [discover, ({ core }) => discoverTests(core)],
  ]);
  try {
    const handler = handlers.get(request.params?.handler);
    let result;
    if (request.method === "describe") {
      result = { api: 1, handlers: [...handlers.keys()] };
    } else if (request.method === "invoke" && handler) {
      result = handler(request.params);
    } else {
      throw new Error("unknown software development operation");
    }
    process.stdout.write(JSON.stringify({ jsonrpc: "2.0", id: request.id, result }));
  } catch (error) {
    process.stdout.write(JSON.stringify({ jsonrpc: "2.0", id: request.id,
      error: { code: -32000, message: error.message } }));
  }
}

export function authorizeDelivery(input) {
  const { operation, source_revision, source_branch, destination, candidate_revision,
    regression_configured, facts } = input;
  const result = (name) => facts[name]?.result;
  const deny = (reason) => ({ allowed: false, reason });
  const tested = (name, revision, projectRegression) => {
    const fact = facts[name];
    const value = fact?.result;
    return fact?.platform_operation === (projectRegression ? "tests.project" : "tests.run")
      && value?.status === "succeeded" && value.candidate === revision
      && value.expected_failure === false && value.writer_revoked === true
      && (value.skipped !== true
        || (projectRegression && !regression_configured && value.reason === "no_regression_configured"));
  };
  if (!result("review") || facts.review.workspace_revision !== source_revision) {
    return deny("独立代码评审必须绑定确切源版本");
  }
  if (operation === "workspace.target") return { allowed: true };
  if (!["git.source.publish", "git.delivery.open", "git.delivery.merge"].includes(operation)) {
    return deny("未知交付操作");
  }
  if (!tested("source_regression", source_revision, true)) {
    return deny("源版本需要真实、已停止的项目回归");
  }
  if (operation === "git.source.publish") return { allowed: true };
  const publication = result("source_publication");
  if (facts.source_publication?.platform_operation !== "git.source.publish"
      || publication?.branch !== source_branch || publication.revision !== source_revision) {
    return deny("候选必须来自已推送的确切源分支");
  }
  if (!["mainline", "parent_work"].includes(destination)) return deny("未知交付上级");
  if (destination === "mainline" && !result("children")) {
    return deny("正式交付需要已接纳并同步的 Task 汇合");
  }
  if (operation === "git.delivery.open") return { allowed: true };
  if (!tested("candidate_regression", candidate_revision, true)) {
    return deny("合入需要确切候选版本的真实、已停止回归");
  }
  if (destination === "mainline") {
    if (!facts.approval?.human_confirmation) return deny("正式合入需要确切候选的真实人工批准");
  } else if (!tested("green", source_revision, false)) {
    return deny("Task 自动合入需要同一源版本的独立 Green");
  }
  return { allowed: true };
}

import { appendFileSync } from "node:fs";
import { join } from "node:path";
import { performance } from "node:perf_hooks";
import { chromium } from "../../web/node_modules/playwright-core/index.mjs";

const [
  origin,
  output,
  samples,
  project,
  feature,
  switchFeature,
  approvalRoot,
  only,
] = process.argv.slice(2);
const browser = await chromium.launch({ channel: "chromium", headless: true });
const timeout = 12_000;
const matrix = [
  ["overview", "overview", "query.workspace"],
  ["manager", "execution", "query.orchestration", "manager"],
  ["workflow", "execution", "query.workflow_instance", "overview"],
  ["materials", "execution", "query.workflow_materials", "documents"],
  ["contexts", "execution", "query.workflow_contexts", "context"],
  ["records", "execution", "query.workflow_executions", "records"],
  ["discussion", "discussion", "query.ui_bootstrap"],
  ["projects", "projects", "query.project_directory"],
  ["workflows", "workflows", "query.configuration_index"],
  ["actors", "actors", "query.configuration_index"],
  ["components", "components", "query.configuration_index"],
  ["history", "history", "query.history"],
  ["settings", "settings", "query.workspace"],
];

function url(tab, detail) {
  const params = new URLSearchParams({ tab, project_id: project });
  if (detail) {
    params.set("detail", detail);
    params.set("feature_id", feature);
  }
  return origin + "/?" + params;
}

function call(request) {
  if (!request.url().endsWith("/rpc")) return undefined;
  try {
    return request.postDataJSON();
  } catch {
    return undefined;
  }
}

function save(file, row) {
  appendFileSync(join(output, file), JSON.stringify(row) + "\n");
}

async function timed(caseName, cohort, sample, action) {
  const start = performance.now();
  let values = {};
  let error = null;
  try {
    values = await action(start);
  } catch (failure) {
    error = String(failure.message || failure)
      .split("\n")[0]
      .slice(0, 300);
  }
  save("browser.jsonl", {
    case: caseName,
    cohort,
    sample,
    host: "read",
    ms: performance.now() - start,
    error,
    ...values,
  });
}

async function afterPaint(page) {
  await page.evaluate(
    () =>
      new Promise((resolve) =>
        requestAnimationFrame(() => requestAnimationFrame(resolve)),
      ),
  );
}

function monitor(page, phase) {
  page.setDefaultTimeout(timeout);
  const requests = new WeakMap();
  page.on("request", (request) => {
    if (call(request)) requests.set(request, { ...phase() });
  });
  page.on("pageerror", (error) =>
    save("browser-errors.jsonl", { ...phase(), error: error.name }),
  );
  page.on("requestfinished", async (request) => {
    const rpc = call(request);
    if (!rpc) return;
    const scope = requests.get(request) || {};
    try {
      const response = await request.response();
      const timing = request.timing();
      const value = await response.json();
      save("browser-rpc.jsonl", {
        ...scope,
        method: rpc.method,
        view: rpc.params?.view,
        request_id: await response.headerValue("x-devdesk-request-id"),
        start_ms: timing.startTime,
        ms: timing.responseEnd,
        error: value.error?.data?.code || value.error?.code || null,
      });
    } catch {
      save("browser-rpc.jsonl", {
        ...scope,
        method: rpc.method,
        error: "unreadable",
      });
    }
  });
}

async function resources(page) {
  return page.evaluate(() => {
    const script = performance
      .getEntriesByType("resource")
      .filter((entry) => new URL(entry.name).pathname.endsWith(".js"));
    return {
      javascript_transfer_bytes: script.reduce(
        (sum, entry) => sum + entry.transferSize,
        0,
      ),
      javascript_encoded_bytes: script.reduce(
        (sum, entry) => sum + entry.encodedBodySize,
        0,
      ),
      long_tasks: window.workbenchLongTasks,
    };
  });
}

async function newContext() {
  const context = await browser.newContext({
    viewport: { width: 1440, height: 1000 },
  });
  await context.addInitScript(() => {
    window.workbenchLongTasks = [];
    new PerformanceObserver((list) => {
      for (const entry of list.getEntries()) {
        if (window.workbenchLongTasks.length < 64)
          window.workbenchLongTasks.push({
            start_ms: entry.startTime,
            ms: entry.duration,
          });
      }
    }).observe({ type: "longtask", buffered: true });
  });
  return context;
}

function rootManager(page) {
  return page.locator(".manager-workflow-summary").getByRole("region", {
    name: "项目经理协作",
    exact: true,
  });
}

async function scriptMetrics(session) {
  const { metrics } = await session.send("Performance.getMetrics");
  const value = (name) => metrics.find((item) => item.name === name)?.value;
  return {
    compile: value("V8CompileDuration"),
    execution: value("ScriptDuration"),
  };
}

async function configurationReady(page, name) {
  if (name === "projects")
    await page
      .getByRole("heading", { name: "项目管理", exact: true, level: 2 })
      .waitFor();
  if (name === "workflows")
    await page
      .getByRole("region", { name: "工作流程管理", exact: true })
      .waitFor();
  if (name === "components")
    await page
      .getByRole("region", { name: "Model 管理", exact: true })
      .waitFor();
}

async function measureConfigurationNavigation(page, route, start) {
  const session = await page.context().newCDPSession(page);
  try {
    await session.send("Performance.enable");
    const before = await scriptMetrics(session);
    const sample = await navigation(page, route, start);
    const after = await scriptMetrics(session);
    const duration = (kind) =>
      typeof after[kind] === "number"
        ? (after[kind] >= (before[kind] ?? 0)
            ? after[kind] - (before[kind] ?? 0)
            : after[kind]) * 1000
        : null;
    return {
      ...sample,
      script_compile_ms: duration("compile"),
      script_duration_ms: duration("execution"),
    };
  } finally {
    await session.detach();
  }
}

async function measureBackgroundParsing(page, route, start) {
  const session = await page.context().newCDPSession(page);
  let parsing = 0;
  let compilation = 0;
  const assets = new Set();
  session.on("Tracing.dataCollected", ({ value }) => {
    for (const event of value) {
      if (
        event.name !== "v8.parseOnBackground" &&
        event.name !== "v8.compileModule"
      )
        continue;
      const url = event.args?.data?.url;
      if (!url?.startsWith(origin + "/assets/")) continue;
      assets.add(new URL(url).pathname);
      if (event.name === "v8.parseOnBackground") parsing += event.dur || 0;
      else compilation += event.dur || 0;
    }
  });
  const complete = new Promise((resolve) =>
    session.once("Tracing.tracingComplete", resolve),
  );
  try {
    await session.send("Tracing.start", {
      categories: "v8,devtools.timeline,disabled-by-default-v8.compile",
      transferMode: "ReportEvents",
    });
    let sample;
    try {
      sample = {
        ...(await navigation(page, route, start)),
        ms: performance.now() - start,
      };
    } finally {
      await session.send("Tracing.end");
      await complete;
    }
    return {
      ...sample,
      script_background_parse_ms: parsing / 1000,
      script_module_compile_ms: compilation / 1000,
      script_assets: [...assets].sort(),
    };
  } finally {
    await session.detach();
  }
}

async function navigation(page, route, start) {
  const [name, tab, method, detail] = route;
  const expected = page
    .waitForResponse(
      (response) => {
        const request = call(response.request());
        if (request?.method !== method) return false;
        if (["materials", "contexts", "records"].includes(name))
          return request.params?.owner_id === feature;
        return true;
      },
      { timeout },
    )
    .catch(() => null);
  await page.goto(url(tab, detail), { waitUntil: "domcontentloaded", timeout });
  const response = await expected;
  if (!response) throw Error("Expected page RPC did not complete");
  const value = await response.json();
  if (value.error) throw Error(value.error.data?.code || value.error.code);
  const content = performance.now() - start;
  if (name === "manager") {
    const section = rootManager(page);
    await section.waitFor({ state: "visible" });
    await section
      .getByText("正在读取经理状态…", { exact: true })
      .waitFor({ state: "hidden" });
  }
  await configurationReady(page, name);
  await afterPaint(page);
  return {
    content_ms: content,
    request_id: await response.headerValue("x-devdesk-request-id"),
    ...(await resources(page)),
  };
}

async function followups(page, index) {
  await page.goto(url("execution", "manager"), {
    waitUntil: "domcontentloaded",
    timeout,
  });
  const manager = rootManager(page);
  await manager.waitFor();
  await manager
    .getByText("正在读取经理状态…", { exact: true })
    .waitFor({ state: "hidden" });
  const history = manager.getByText("审批历史", { exact: true });
  if (await history.count()) {
    const records = manager.getByRole("list", { name: "已处理的决定" });
    await timed(
      "browser-manager-history-list",
      "warm-document",
      index,
      async () => {
        await history.click();
        await records.waitFor();
        await afterPaint(page);
      },
    );
    await timed(
      "browser-manager-history-expand",
      "warm-document",
      index,
      async () => {
        const pending = page
          .waitForResponse(
            (response) => {
              const rpc = call(response.request());
              return (
                rpc?.method === "query.orchestration" &&
                rpc.params?.view === "decision"
              );
            },
            { timeout },
          )
          .catch(() => null);
        await records.getByRole("button").first().click();
        const response = await pending;
        if (!response || (await response.json()).error)
          throw Error("Original decision request failed");
        await records.getByRole("article").first().waitFor();
        await afterPaint(page);
        return {
          request_id: await response.headerValue("x-devdesk-request-id"),
        };
      },
    );
  } else {
    await timed(
      "browser-manager-history-expand",
      "warm-document",
      index,
      async () => {
        const settled = page
          .locator(".manager-interaction summary")
          .filter({
            hasText: /已确认|已退回|已撤回|例外批准|已过期/,
          })
          .first();
        await settled.waitFor();
        const start = performance.now();
        await settled.click();
        await settled.locator("..").waitFor();
        if ((await settled.locator("..").getAttribute("open")) === null)
          throw Error("History decision was not expanded");
        await afterPaint(page);
        return { ms: performance.now() - start };
      },
    );
  }
  await timed("browser-material-body", "warm-document", index, async () => {
    await page.goto(url("execution", "documents"), {
      waitUntil: "domcontentloaded",
      timeout,
    });
    const material = page
      .getByRole("navigation", { name: "材料目录" })
      .getByRole("button")
      .first();
    await material.waitFor();
    const start = performance.now();
    await material.click();
    await page
      .getByRole("article", { name: "选中材料正文" })
      .getByRole("heading")
      .first()
      .waitFor();
    await afterPaint(page);
    return { ms: performance.now() - start };
  });
  if (switchFeature) {
    const picker = page.getByRole("combobox", { name: "当前 Feature" });
    await timed("browser-feature-switch", "warm-document", index, async () => {
      const expected = page
        .waitForResponse(
          (response) => {
            const request = call(response.request());
            return (
              request?.method === "query.workflow_materials" &&
              request.params?.owner_id === switchFeature
            );
          },
          { timeout },
        )
        .catch(() => null);
      await picker.selectOption(switchFeature);
      const response = await expected;
      if (!response || (await response.json()).error)
        throw Error("Switched Feature material request failed");
      await page.getByText(/共 [1-9]\d* 项工作流产物/).waitFor();
      await afterPaint(page);
    });
    await timed("browser-feature-clear", "warm-document", index, async () => {
      await picker.selectOption("");
      await page
        .getByRole("status")
        .filter({ hasText: /^请选择 Feature$/ })
        .waitFor();
      if (await page.getByRole("navigation", { name: "材料目录" }).count())
        throw Error("Previous Feature materials remained visible");
      await afterPaint(page);
    });
  }
}

async function pendingApproval() {
  const context = await newContext();
  const page = await context.newPage();
  const scope = {
    case: "browser-pending-approval",
    cohort: "warm-browser",
    sample: 0,
  };
  monitor(page, () => ({ ...scope }));
  const params = new URLSearchParams({
    tab: "execution",
    detail: "manager",
    project_id: project,
    instance_id: approvalRoot,
    feature_id: feature,
  });
  const card = page.getByRole("article", { name: "确认基准审批", exact: true });
  const confirm = card.getByRole("button", {
    name: "确认：确认基准审批",
    exact: true,
  });
  const open = async () => {
    await page.goto(origin + "/?" + params, {
      waitUntil: "domcontentloaded",
      timeout,
    });
    await confirm.waitFor();
    if (!(await confirm.isEnabled()))
      throw Error("Current approval action was disabled");
    await afterPaint(page);
  };
  // A fixture that cannot expose the approval action has no browser timing.
  try {
    await open();
  } catch (error) {
    await page.screenshot({
      path: join(output, "approval-fixture-failure.png"),
    });
    throw error;
  }
  for (let sample = 0; sample < Number(samples); sample++) {
    scope.sample = sample;
    await timed(scope.case, scope.cohort, sample, open);
  }
  await context.close();
}

async function parsingWorkbench() {
  const routes = matrix.filter(([name]) =>
    ["overview", "manager", "projects", "workflows", "components"].includes(
      name,
    ),
  );
  for (let sample = 0; sample < Number(samples); sample++) {
    for (const route of routes) {
      const context = await newContext();
      try {
        const page = await context.newPage();
        page.setDefaultTimeout(timeout);
        await timed(
          "browser-" + route[0] + "-parsing",
          "cold-browser",
          sample,
          (start) => measureBackgroundParsing(page, route, start),
        );
      } finally {
        await context.close();
      }
    }
  }
}

async function workbench() {
  const warm = await newContext();
  const warmPage = await warm.newPage();
  let phase = {};
  monitor(warmPage, () => phase);
  await warmPage.goto(url("overview"), {
    waitUntil: "domcontentloaded",
    timeout,
  });
  const configuration = only === "configuration-pages";
  const routes = configuration
    ? matrix.filter(([name]) =>
        ["overview", "manager", "projects", "workflows", "components"].includes(
          name,
        ),
      )
    : only
      ? matrix.filter(([name]) => name === only)
      : matrix;
  const open = configuration ? measureConfigurationNavigation : navigation;
  if (!routes.length && only !== "manager-details")
    throw Error("Unknown browser case");
  for (let sample = 0; sample < Number(samples); sample++) {
    for (const route of routes) {
      phase = { sample, case: "browser-" + route[0], cohort: "warm-browser" };
      await timed(phase.case, phase.cohort, sample, (start) =>
        open(warmPage, route, start),
      );
      // Each navigation needs its own empty browser cache.
      const context = await newContext();
      const page = await context.newPage();
      const scope = { sample, case: phase.case, cohort: "cold-browser" };
      monitor(page, () => scope);
      try {
        await timed(scope.case, scope.cohort, sample, (start) =>
          open(page, route, start),
        );
      } finally {
        await context.close();
      }
    }
    phase = { sample, cohort: "warm-document", case: "execution-followups" };
    if (!only || only === "manager-details") await followups(warmPage, sample);
    if (only && only !== "manager") continue;
    phase = { sample, cohort: "warm-ui", case: "browser-manager" };
    await timed(phase.case, phase.cohort, sample, async () => {
      await warmPage.goto(url("execution", "manager"), {
        waitUntil: "domcontentloaded",
        timeout,
      });
      const manager = rootManager(warmPage);
      await manager.waitFor();
      await warmPage
        .getByRole("button", { name: "项目管理", exact: true })
        .click();
      await warmPage
        .getByRole("heading", { name: "项目管理", exact: true, level: 1 })
        .waitFor();
      const start = performance.now();
      await warmPage
        .getByRole("button", { name: "执行与交付", exact: true })
        .click();
      await manager.waitFor();
      await manager
        .getByText("正在读取经理状态…", { exact: true })
        .waitFor({ state: "hidden" });
      await afterPaint(warmPage);
      return { ms: performance.now() - start };
    });
  }
  await warm.close();
}

try {
  if (approvalRoot) await pendingApproval();
  else if (only === "configuration-parsing") await parsingWorkbench();
  else await workbench();
} finally {
  await browser.close();
}

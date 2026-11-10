import { test as base, expect, type Page } from "@playwright/test";
import { spawn } from "node:child_process";
import { createInterface } from "node:readline";
import { mkdtempSync, rmSync } from "node:fs";
import { tmpdir } from "node:os";
import { join, resolve } from "node:path";
// This test creates a real project; keep it outside the suite's shared dataset.
const test = base.extend<{ stageKernel: string }>({
  stageKernel: async ({}, use) => {
    const directory = mkdtempSync(join(tmpdir(), "devdesk-stage-rows-"));
    const kernelEnv = { ...process.env };
    delete kernelEnv.DEVDESK_ACCESS_TOKEN;
    const child = spawn(
      resolve(
        import.meta.dirname,
        "../../target",
        process.env.DEVDESK_E2E_PROFILE ?? "debug",
        "devdesk-host",
      ),
      ["--data", directory, "--port", "0"],
      {
        env: kernelEnv,
        stdio: ["ignore", "pipe", "pipe"],
      },
    );
    let diagnostics = "";
    child.stderr.on("data", (data) => {
      diagnostics = (diagnostics + data).slice(-4000);
    });
    try {
      const origin = await new Promise<string>((done, fail) => {
        const lines = createInterface({ input: child.stdout });
        const timer = setTimeout(
          () => fail(new Error("Stage kernel startup timed out")),
          15000,
        );
        child.once("error", (error) => {
          clearTimeout(timer);
          fail(error);
        });
        child.once("exit", () => {
          clearTimeout(timer);
          fail(new Error(diagnostics));
        });
        lines.on("line", (line) => {
          if (line.startsWith("http://")) {
            clearTimeout(timer);
            lines.close();
            done(line.trim());
          }
        });
      });
      await use(origin);
    } finally {
      if (child.exitCode === null)
        await new Promise<void>((done) => {
          const timer = setTimeout(() => child.kill("SIGKILL"), 5000);
          child.once("exit", () => {
            clearTimeout(timer);
            done();
          });
          child.kill("SIGINT");
        });
      rmSync(directory, { recursive: true, force: true });
    }
  },
  baseURL: async ({ stageKernel }, use) => use(stageKernel),
});
async function rpc(page: Page, method: string, params: any = {}) {
  const r = await page.request
    .post("/rpc", { data: { jsonrpc: "2.0", id: 1, method, params } })
    .then((r) => r.json());
  expect(r.error, JSON.stringify(r)).toBeUndefined();
  return r.result;
}
async function setup(page: Page) {
  await page.goto("/");
  if ((await page.request.get("/capabilities").then((r) => r.json())).setup_required) {
    await page
      .getByLabel("数据目录")
      .fill(mkdtempSync(join(tmpdir(), "devdesk-stage-rows-")));
    await page.getByRole("button", { name: "初始化 / 打开数据集" }).click();
  }
}
test.afterEach(async ({ page }) => {
  await page.unrouteAll({ behavior: "ignoreErrors" });
});
test("执行与交付每行一个阶段，调用的子流程在阶段内展开且折叠状态可恢复", async ({
  page,
}, info) => {
  const errors: string[] = [];
  page.on("pageerror", (e) => errors.push(e.message));
  await setup(page);
  const publish = async (name: string, document: any) => {
    const draft = await rpc(page, "workflow.create", {
      command_id: crypto.randomUUID(),
      payload: { name, document },
    });
    return rpc(page, "workflow.publish", {
      command_id: crypto.randomUUID(),
      target: draft.id,
      expected_revision: draft.revision,
      payload: {},
    });
  };
  const child = await publish("阶段行子流程", {
    schema_version: 3,
    process_kind: "subworkflow",
    stages: [{ key: "check", name: "子流程检查" }],
    steps: [
      {
        key: "snapshot",
        name: "子流程快照节点",
        stage: "check",
        kind: "snapshot.capture",
        advance_mode: "human_confirmation",
      },
    ],
  });
  const defaults = await rpc(page, "query.configuration_defaults");
  const template = (
    await rpc(page, "query.workflow", { id: defaults.workflow_ref })
  ).data;
  const phaseKeys = template.stages.map((stage: any) => stage.key);
  const originalSteps = template.steps.map((step: any) => ({
    ...step,
    stage:
      phaseKeys.indexOf(step.stage) < 2
        ? "requirements"
        : phaseKeys.indexOf(step.stage) < 5
          ? "implementation"
          : "release",
    dependencies: step.dependencies.length
      ? step.dependencies
      : ["layout_child"],
  }));
  const workflow = await publish("单行阶段流程", {
    ...template,
    stages: [
      { key: "requirements", name: "需求阶段" },
      { key: "implementation", name: "实现阶段" },
      { key: "release", name: "交付阶段" },
    ],
    steps: [
      {
        key: "layout_child",
        name: "需求内嵌子流程",
        stage: "requirements",
        kind: "workflow.call",
        subworkflow_ref: child.id,
      },
      ...originalSteps,
    ],
  });
  const p = await rpc(page, "project.create", {
    command_id: crypto.randomUUID(),
    payload: { title: "阶段布局", repository: "fixture/stage-layout" },
  });
  const before = await rpc(page, "query.workspace", { project_id: p.id });
  await rpc(page, "project.configuration.update", {
    project_id: p.id,
    command_id: crypto.randomUUID(),
    target: p.id,
    expected_revision: before.active_project.revision,
    payload: {
      workflow_ref: workflow.id,
      role_agent_bindings: defaults.role_agent_bindings,
    },
  });
  const feature = await rpc(page, "feature.create", {
    project_id: p.id,
    command_id: crypto.randomUUID(),
    payload: { project_id: p.id, title: "阶段行验证" },
  });
  const workspace = await rpc(page, "query.workspace", {
    project_id: p.id,
    feature_id: feature.id,
  });
  const runtime = workspace.configurations.find(
    (r: any) =>
      r.kind === "workflow_instance" && r.definition_id === feature.id,
  );
  const childId = crypto.randomUUID();
  const parentStep = runtime.data.steps[0].id;
  await page.route("**/rpc", async (route) => {
    const c = route.request().postDataJSON();
    if (c.method === "query.workflow_instance" && c.params.id === childId) {
      return route.fulfill({
        json: {
          jsonrpc: "2.0",
          id: c.id,
          result: {
            record: {
              id: childId,
              revision: 1,
              data: {
                owner_id: crypto.randomUUID(),
                status: "waiting",
                paused: true,
                task_members: [],
                steps: [
                  {
                    id: "child-step",
                    key: "inspect",
                    round: 1,
                    status: "pending",
                    outputs: {},
                    revision: 1,
                  },
                ],
              },
            },
            plan: {
              stages: [{ key: "work", name: "Task 阶段" }],
              steps: [
                {
                  key: "inspect",
                  name: "Task 子流程节点",
                  stage: "work",
                  kind: "snapshot.capture",
                  interaction: {},
                },
              ],
            },
            configuration: {},
            actions: [],
            children: [],
          },
        },
      });
    }
    if (!["query.workspace", "query.workflow_instance"].includes(c.method))
      return route.continue();
    const response = await route.fetch();
    const body = await response.json();
    if (c.method === "query.workspace") body.result.repository_ready = true;
    else if (c.params.id === runtime.id)
      body.result.children = [
        {
          record: {
            id: childId,
            revision: 1,
            data: { status: "waiting", paused: true, task_members: [] },
          },
          title: "任务子流程",
          parent_step_id: parentStep,
        },
      ];
    return route.fulfill({ response, json: body });
  });
  await page.goto(
    `/?tab=execution&feature_id=${feature.id}&project_id=${p.id}`,
  );
  const stages = page
    .locator(".workflow-runtime-stages")
    .first()
    .locator(":scope > .workflow-runtime-stage");
  await expect(stages).toHaveCount(3);
  const boxes = await stages.evaluateAll((nodes) =>
    nodes.map((n) => {
      const b = n.getBoundingClientRect();
      return { x: b.x, y: b.y, width: b.width, bottom: b.bottom };
    }),
  );
  expect(boxes[0].x).toBe(boxes[1].x);
  expect(boxes[0].width).toBe(boxes[2].width);
  expect(boxes[1].y).toBeGreaterThanOrEqual(boxes[0].bottom);
  expect(boxes[2].y).toBeGreaterThanOrEqual(boxes[1].bottom);
  const first = page.getByRole("button", {
    name: "阶段 需求阶段",
    exact: true,
  });
  if ((await first.getAttribute("aria-expanded")) === "false")
    await first.click();
  await expect(
    stages
      .first()
      .getByRole("button", { name: /^子流程 · 子流程检查 子流程快照节点/ }),
  ).toBeVisible();
  await expect(stages.first()).toContainText("子流程 · 子流程检查");
  await stages
    .first()
    .getByRole("button", {
      name: /子流程 · 任务子流程.*展开/,
    })
    .click();
  await expect(
    stages.first().getByRole("button", { name: /Task 子流程节点/ }),
  ).toBeVisible();
  await first.click();
  await expect(first).toHaveAttribute("aria-expanded", "false");
  await expect(
    page.getByRole("button", { name: /^子流程 · 子流程检查 子流程快照节点/ }),
  ).toHaveCount(0);
  await page.reload();
  await expect(first).toHaveAttribute("aria-expanded", "false");
  await first.click();
  await page.screenshot({
    path: info.outputPath("execution-stage-rows.png"),
    fullPage: true,
  });
  expect(errors).toEqual([]);
});

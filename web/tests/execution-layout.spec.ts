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
  if (
    (await page.request.get("/capabilities").then((r) => r.json()))
      .setup_required
  ) {
    await page
      .getByLabel("数据目录")
      .fill(mkdtempSync(join(tmpdir(), "devdesk-stage-rows-")));
    await page.getByRole("button", { name: "初始化 / 打开数据集" }).click();
  }
}
test.afterEach(async ({ page }) => {
  await page.unrouteAll({ behavior: "ignoreErrors" });
});

test("通用工作流按阶段排列，真实子流程内联展开且折叠可恢复", async ({
  page,
}, info) => {
  await setup(page);
  const command = (
    method: string,
    payload: any,
    target?: string,
    revision?: number,
  ) =>
    rpc(page, method, {
      command_id: crypto.randomUUID(),
      target,
      expected_revision: revision,
      payload,
    });
  async function publish(name: string, document: any) {
    const draft = await command("workflow.create", { name, document });
    return command("workflow.publish", {}, draft.id, draft.revision);
  }
  const child = await publish("可复用确认", {
    schema_version: 5,
    stages: [{ key: "inside", name: "内部阶段" }],
    steps: [
      {
        key: "inspect",
        name: "检查原始材料",
        stage: "inside",
        executor: { type: "control" },
        confirmation: "human",
      },
    ],
  });
  const parent = await publish("布局流程", {
    schema_version: 5,
    stages: [
      { key: "one", name: "业务阶段一" },
      { key: "two", name: "业务阶段二" },
    ],
    steps: [
      {
        key: "nested",
        name: "处理子流程",
        stage: "one",
        executor: { type: "workflow", workflow_ref: child.id },
      },
      {
        key: "done",
        name: "结束处理",
        stage: "two",
        executor: { type: "control" },
        dependencies: ["nested"],
      },
    ],
  });
  const project = await command("project.create", { title: "布局项目" });
  const started = await command(
    "workflow.start",
    { workflow_ref: parent.id, role_agent_bindings: {} },
    project.id,
    (await rpc(page, "query.workspace", { project_id: project.id }))
      .active_project.revision,
  );
  const current = await rpc(page, "query.workflow_instance", {
    project_id: project.id,
    id: started.id,
  });
  await rpc(page, "workflow.step.dispatch", {
    project_id: project.id,
    command_id: crypto.randomUUID(),
    target: started.id,
    expected_revision: current.record.revision,
    payload: {
      step: "nested",
      plan_revision_id: current.runtime.active_plan_revision_id,
      control_version: current.runtime.control_version,
    },
  });
  await page.goto(`/?tab=execution&project_id=${project.id}`);
  const runtime = page.getByRole("region", { name: "当前工作流", exact: true });
  await expect(
    runtime.getByRole("heading", { name: "业务阶段一" }),
  ).toBeVisible();
  await expect(
    runtime.getByRole("heading", { name: "业务阶段二" }),
  ).toBeVisible();
  const stages = await runtime
    .locator(":scope > .workflow-stage-row")
    .first()
    .boundingBox();
  expect(stages).toBeTruthy();
  await expect(
    runtime.getByRole("button", { name: /子流程 ·.*展开/ }),
  ).toBeVisible();
  await runtime.getByRole("button", { name: /子流程 ·.*展开/ }).click();
  await expect(
    page.getByRole("region", { name: "子流程进度", exact: true }),
  ).toBeVisible();
  await expect(
    page.getByRole("button", { name: /检查原始材料/ }),
  ).toBeVisible();
  await runtime
    .getByRole("button", { name: "折叠阶段", exact: true })
    .first()
    .click();
  await expect(page.getByRole("button", { name: /检查原始材料/ })).toHaveCount(
    0,
  );
  await page.reload();
  await expect(
    runtime.getByRole("button", { name: "展开阶段", exact: true }),
  ).toBeVisible();
  await runtime.getByRole("button", { name: "展开阶段", exact: true }).click();
  await expect(
    page.getByRole("button", { name: /检查原始材料/ }),
  ).toBeVisible();
  await page.screenshot({
    path: info.outputPath("workflow-stages.png"),
    fullPage: true,
  });
});

test("直接调整运行计划、复用成果、绕过与移动端刷新", async ({ page }, info) => {
  await setup(page);
  const command = async (method: string, payload: any, target?: string) => {
    const revision = target
      ? (
          await rpc(page, "query.workflow_instance", { id: target }).catch(
            () => undefined,
          )
        )?.record.revision
      : undefined;
    return rpc(page, method, {
      command_id: crypto.randomUUID(),
      target,
      expected_revision: revision,
      payload,
    });
  };
  const draft = await command("workflow.create", {
    name: "可调整计划",
    document: {
      schema_version: 5,
      stages: [
        { key: "first", name: "材料处理" },
        { key: "optional", name: "可选复核" },
      ],
      steps: [
        {
          key: "collect",
          name: "整理材料",
          stage: "first",
          executor: { type: "control" },
        },
        {
          key: "review",
          name: "可选检查",
          stage: "optional",
          executor: { type: "control" },
          dependencies: ["collect"],
        },
      ],
    },
  });
  const published = await rpc(page, "workflow.publish", {
    command_id: crypto.randomUUID(),
    target: draft.id,
    expected_revision: draft.revision,
    payload: {},
  });
  const project = await command("project.create", { title: "运行计划验收" });
  const workspace = await rpc(page, "query.workspace", {
    project_id: project.id,
  });
  const started = await rpc(page, "workflow.start", {
    project_id: project.id,
    command_id: crypto.randomUUID(),
    target: project.id,
    expected_revision: workspace.active_project.revision,
    payload: { workflow_ref: published.id, role_agent_bindings: {} },
  });
  await page.goto(`/?tab=execution&project_id=${project.id}`);
  const runtime = page.getByRole("region", { name: "当前工作流", exact: true });
  await expect(
    runtime.getByRole("region", { name: "项目经理协作" }),
  ).toBeVisible();
  const collect = runtime.getByRole("article", {
    name: "步骤 · 整理材料",
    exact: true,
  });
  await collect.getByRole("button", { name: "执行步骤", exact: true }).click();
  await collect.getByRole("button", { name: "接纳结果", exact: true }).click();
  await expect
    .poll(
      async () =>
        (
          await rpc(page, "query.workflow_instance", {
            project_id: project.id,
            id: started.id,
          })
        ).runtime.steps[0].status,
    )
    .toBe("completed");
  const before = await rpc(page, "query.workflow_instance", {
    project_id: project.id,
    id: started.id,
  });
  const editor = runtime
    .locator("details")
    .filter({ has: page.getByText("调整当前运行计划", { exact: true }) });
  await editor.locator("summary").click();
  await editor.getByLabel("计划调整方式").selectOption("bypass");
  await editor.getByLabel("计划调整对象").selectOption("optional");
  await editor
    .getByLabel("计划调整理由")
    .fill("此验收允许跳过无材料输出的可选复核");
  await editor.getByRole("button", { name: "预览影响", exact: true }).click();
  await expect(editor.getByText(/受影响步骤：review/)).toBeVisible();
  await editor
    .getByRole("button", { name: "应用此计划调整", exact: true })
    .click();
  await expect(
    runtime.getByRole("button", { name: /可选检查 · 已跳过/ }),
  ).toBeVisible();
  await editor.getByLabel("计划调整方式").selectOption("add");
  await editor.getByLabel("计划调整对象").selectOption("collect");
  await editor.getByLabel("新增步骤名称").fill("补充交接");
  await editor.getByLabel("计划调整理由").fill("在已完成成果之后增加交接");
  await editor.getByRole("button", { name: "预览影响", exact: true }).click();
  await editor
    .getByRole("button", { name: "应用此计划调整", exact: true })
    .click();
  const added = runtime.getByRole("article", {
    name: "步骤 · 补充交接",
    exact: true,
  });
  await added.getByRole("button", { name: "执行步骤", exact: true }).click();
  await added.getByRole("button", { name: "接纳结果", exact: true }).click();
  await runtime.getByRole("button", { name: "完成流程", exact: true }).click();
  await expect
    .poll(
      async () =>
        (
          await rpc(page, "query.workflow_instance", {
            project_id: project.id,
            id: started.id,
          })
        ).runtime.status,
    )
    .toBe("completed");
  const after = await rpc(page, "query.workflow_instance", {
    project_id: project.id,
    id: started.id,
  });
  expect(after.runtime.status).toBe("completed");
  const {
    budget_wait_started: _started,
    budget_waited_ms: _waited,
    ...frozen
  } = before.runtime.steps[0];
  expect(
    after.runtime.steps.find((s: any) => s.step === "collect"),
  ).toMatchObject(frozen);
  expect(after.runtime.active_plan_revision_id).not.toBe(
    before.runtime.active_plan_revision_id,
  );
  await page.setViewportSize({ width: 390, height: 844 });
  await page.reload();
  await runtime.getByText("计划与执行历史", { exact: true }).click();
  await expect(runtime.getByText(/原合同/).first()).toBeVisible();
  await page.screenshot({
    path: info.outputPath("harness-plan-mobile.png"),
    fullPage: true,
  });
});

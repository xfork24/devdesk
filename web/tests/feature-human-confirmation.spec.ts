import { test, expect, type Page } from "@playwright/test";
import { mkdtempSync } from "node:fs";
import { tmpdir } from "node:os";
import { join } from "node:path";

async function setup(page: Page) {
  await page.goto("/");
  if (
    (await page.request.get("/capabilities").then((r) => r.json()))
      .setup_required
  ) {
    await page
      .getByLabel("数据目录")
      .fill(mkdtempSync(join(tmpdir(), "devdesk-human-gates-")));
    await page.getByRole("button", { name: "初始化 / 打开数据集" }).click();
    await expect(page.getByLabel("数据目录")).toHaveCount(0);
  }
}

async function creationFixture(page: Page, inputs: Record<string, unknown>) {
  await setup(page);
  const project = {
    id: "feature-project",
    title: "需求确认项目",
    revision: 1,
    execution_mode: "live",
  };
  let submitted: any;
  await page.route("**/capabilities", (route) =>
    route.fulfill({
      json: { setup_required: false, active_project_id: project.id },
    }),
  );
  await page.route("**/rpc", async (route) => {
    const call = route.request().postDataJSON();
    let result: any;
    if (call.method === "query.workspace") {
      result = {
        active_project: project,
        projects: [project],
        features: [],
        total_features: 0,
        total_projects: 1,
        repository_ready: true,
        configuration_ready: true,
        configurations: [
          {
            type: "project_configuration",
            id: "selected-config",
            version: 1,
            data: { workflow_ref: "selected-flow:1" },
          },
        ],
        summary: {},
        definitions: [],
        harnesses: [],
        sandboxes: [],
      };
    } else if (call.method === "query.workflow") {
      expect(call.params.id).toBe("selected-flow:1");
      result = { id: "selected-flow:1", type: "workflow", data: { inputs } };
    } else if (call.method === "feature.create") {
      submitted = call.params;
      // Stop at the command boundary; persistence and frozen Agent input are tested in Rust.
      return route.fulfill({
        json: {
          jsonrpc: "2.0",
          id: call.id,
          error: {
            code: -32000,
            message: "已捕获创建请求",
            data: { code: "fixture_boundary" },
          },
        },
      });
    } else return route.fallback();
    return route.fulfill({ json: { jsonrpc: "2.0", id: call.id, result } });
  });
  await page.goto("/?tab=overview");
  await page.getByRole("button", { name: "新 Feature", exact: true }).click();
  return () => submitted;
}

test("创建 Feature 接收自然语言需求，未声明输入时隐藏 JSON 和高级选项", async ({
  page,
}) => {
  const submitted = await creationFixture(page, {});
  const dialog = page.getByRole("dialog", { name: "创建 Feature" });
  await expect(dialog.getByLabel("初始需求", { exact: true })).toBeVisible();
  await expect(dialog.getByText("高级选项", { exact: true })).toHaveCount(0);
  await expect(
    dialog.getByLabel("流程输入（JSON）", { exact: true }),
  ).toHaveCount(0);
  await dialog.getByLabel("Feature 名称", { exact: true }).fill("CSV 导出");
  await dialog.getByRole("button", { name: "确认", exact: true }).click();
  expect(submitted()).toBeUndefined();
  const brief = "用户可以把筛选后的结果导出为 CSV；保留已有 JSON 导出。";
  await dialog.getByLabel("初始需求", { exact: true }).fill(brief);
  await dialog.getByRole("button", { name: "确认", exact: true }).click();
  await expect(dialog.getByRole("alert")).toContainText("已捕获创建请求");
  expect(submitted().payload.initial_requirement).toBe(brief);
  expect(submitted().payload.workflow_inputs).toBeUndefined();
  expect(submitted().payload.configuration_revision_id).toBe("selected-config");
  expect(submitted().payload.start_workflow).toBe(false);
});

test("声明的流程输入放在折叠高级选项中，并与自然语言需求分别提交", async ({
  page,
}) => {
  const submitted = await creationFixture(page, {
    reference: { delivery_ref: "brief-document:1", required: true },
  });
  const dialog = page.getByRole("dialog", { name: "创建 Feature" });
  await expect(
    dialog.getByLabel("流程输入（JSON）", { exact: true }),
  ).toBeHidden();
  await dialog.getByText("高级选项", { exact: true }).click();
  await expect(
    dialog.getByText(/reference（必填，brief-document:1）/),
  ).toBeVisible();
  await dialog
    .getByLabel("流程输入（JSON）", { exact: true })
    .fill('{"reference":"用户提供的业务背景"}');
  await dialog.getByLabel("Feature 名称", { exact: true }).fill("自定义流程");
  await dialog
    .getByLabel("初始需求", { exact: true })
    .fill("基于提供的业务背景梳理导出需求。");
  await dialog.getByRole("button", { name: "确认", exact: true }).click();
  await expect(dialog.getByRole("alert")).toContainText("已捕获创建请求");
  expect(submitted().payload.workflow_inputs).toEqual({
    reference: "用户提供的业务背景",
  });
  expect(submitted().payload.initial_requirement).toBe(
    "基于提供的业务背景梳理导出需求。",
  );
});

test("已有 Feature 只在执行与交付显式启动，创建入口仍在工作总览", async ({
  page,
}) => {
  await setup(page);
  const project = {
    id: "start-project",
    title: "显式启动项目",
    revision: 1,
    workspace_definition: { repository: "fixture/start", mainline: "main" },
  };
  const feature = {
    id: "start-feature",
    project_id: project.id,
    title: "待启动的 Feature",
    status: "draft",
    revision: 3,
  };
  const configuration = {
    id: "saved-work:1",
    type: "work_configuration",
    definition_id: feature.id,
    version: 1,
    data: {
      workflow_ref: "saved-flow:1",
      standard_ref: "saved-standard:1",
      role_agent_bindings: {},
    },
  };
  let submitted: any;
  await page.route("**/capabilities", (route) =>
    route.fulfill({
      json: { setup_required: false, active_project_id: project.id },
    }),
  );
  await page.route("**/rpc", async (route) => {
    const call = route.request().postDataJSON();
    let result: any;
    switch (call.method) {
      case "query.workspace":
        result = {
          active_project: project,
          projects: [project],
          features: [feature],
          total_features: 1,
          total_projects: 1,
          repository_ready: true,
          configuration_ready: true,
          configurations: [configuration],
          summary: {},
          definitions: [],
          harnesses: [],
          sandboxes: [],
          detail: {
            feature,
            tasks: [],
            documents: [],
            snapshots: [],
            contexts: [],
            totals: {},
          },
        };
        break;
      case "query.work_configuration":
        result = { configuration, history: [] };
        break;
      case "query.project_configuration":
        result = { configuration: null };
        break;
      case "query.configuration_index":
      case "query.configurations":
        result = { records: [], total: 0 };
        break;
      case "query.standard_catalog":
        result = { records: [] };
        break;
      case "query.workflow_requirements":
        result = { roles: [] };
        break;
      case "workflow.start":
        submitted = call.params;
        return route.fulfill({
          json: {
            jsonrpc: "2.0",
            id: call.id,
            error: {
              code: -32000,
              message: "已捕获启动请求",
              data: { code: "fixture_boundary" },
            },
          },
        });
      default:
        return route.fallback();
    }
    await route.fulfill({ json: { jsonrpc: "2.0", id: call.id, result } });
  });
  await page.goto(
    `/?tab=overview&project_id=${project.id}&feature_id=${feature.id}`,
  );
  await expect(
    page.getByRole("button", { name: "新 Feature", exact: true }),
  ).toBeVisible();
  await expect(
    page.getByRole("button", { name: "启动 Feature", exact: true }),
  ).toHaveCount(0);
  expect(submitted).toBeUndefined();
  await page.getByRole("button", { name: "执行与交付", exact: true }).click();
  await expect(
    page.getByRole("button", { name: "新 Feature", exact: true }),
  ).toHaveCount(0);
  const start = page.getByRole("region", { name: "启动 Feature", exact: true });
  await expect(
    start.getByText("Feature 尚未启动", { exact: true }),
  ).toBeVisible();
  await expect(
    page.getByRole("tab", { name: "项目经理", exact: true }),
  ).toHaveAttribute("aria-selected", "true");
  await expect(
    page.getByRole("tablist", { name: "执行详情" }).getByRole("tab"),
  ).toHaveText(["项目经理", "工作流程", "材料", "上下文", "执行记录"]);
  await expect(page.locator(".work-standard")).toBeVisible();
  expect(submitted).toBeUndefined();
  await page.getByRole("tab", { name: "工作流程", exact: true }).click();
  await expect(start).toHaveCount(0);
  await expect(page.locator(".work-standard")).toHaveCount(0);
  await page.getByRole("button", { name: "工作总览", exact: true }).click();
  await page.getByRole("button", { name: "执行与交付", exact: true }).click();
  await expect(
    page.getByRole("tab", { name: "项目经理", exact: true }),
  ).toHaveAttribute("aria-selected", "true");
  await expect(start).toBeVisible();
  const executionUrl = `/?tab=execution&project_id=${project.id}&feature_id=${feature.id}`;
  await page.goto(executionUrl);
  await expect(
    page.getByRole("tab", { name: "项目经理", exact: true }),
  ).toHaveAttribute("aria-selected", "true");
  await expect(start).toBeVisible();
  await page.reload();
  await expect(start).toBeVisible();
  await page.goto(`${executionUrl}&detail=overview`);
  await expect(
    page.getByRole("tab", { name: "工作流程", exact: true }),
  ).toHaveAttribute("aria-selected", "true");
  await expect(start).toHaveCount(0);
  await page.getByRole("tab", { name: "项目经理", exact: true }).click();
  await expect(start).toBeVisible();
  await page.goBack();
  await expect(
    page.getByRole("tab", { name: "工作流程", exact: true }),
  ).toHaveAttribute("aria-selected", "true");
  await expect(start).toHaveCount(0);
  await page.goForward();
  await expect(start).toBeVisible();
  await start
    .getByRole("button", { name: "启动 Feature", exact: true })
    .click();
  await expect
    .poll(() => submitted)
    .toMatchObject({
      project_id: project.id,
      target: feature.id,
      expected_revision: feature.revision,
      payload: {},
    });
  await expect(start.getByText(/已捕获启动请求/)).toBeVisible();
});

test("Spec 验收后展示待确认产物，人工确认绑定当前执行和输入输出", async ({
  page,
}, info) => {
  await setup(page);
  const project = {
    id: "confirmation-project",
    title: "人工确认项目",
    revision: 1,
    execution_mode: "live",
    workspace_definition: {
      repository: "fixture/repository",
      mainline: "main",
    },
  };
  const feature = {
    id: "confirmation-feature",
    project_id: project.id,
    title: "CSV 导出",
    status: "running",
    revision: 1,
  };
  const plan = {
    stages: [{ key: "requirements", name: "需求" }],
    steps: [
      {
        key: "requirements",
        name: "澄清需求与 PRD",
        stage: "requirements",
        executor: { type: "agent" },
        outputs: { result: {} },
      },
      {
        key: "requirements_review",
        name: "PRD 验收与人工确认",
        stage: "requirements",
        executor: { type: "agent" },
        confirmation: "human",
        inputs: {
          work: {
            type: "source",
            source: { step: "requirements", port: "result" },
          },
        },
        outputs: { result: {} },
      },
    ],
  };
  const execution = {
    id: "review-execution",
    step: "requirements_review",
    status: "waiting_confirmation",
    round: 1,
    input_digest: "frozen-input-digest",
    outputs: { result: "spec-review-report" },
    reason: "Spec 各项检查通过。",
  };
  const instance = {
    id: "confirmation-root",
    type: "workflow_instance",
    name: "Feature 流程",
    revision: 7,
    data: {
      owner_id: feature.id,
      project_id: project.id,
      active_plan_revision_id: "current-plan",
      control_version: 1,
      status: "running",
      paused: false,
      steps: [
        {
          id: "prd-execution",
          step: "requirements",
          status: "completed",
          round: 1,
          outputs: { result: "original-prd" },
        },
        execution,
      ],
      plan: { document: plan },
      role_agent_bindings: {},
      parent_execution_id: null,
    },
  };
  let confirmation: any = null;
  let submitted: any;
  await page.route("**/capabilities", (route) =>
    route.fulfill({
      json: { setup_required: false, active_project_id: project.id },
    }),
  );
  await page.route("**/rpc", async (route) => {
    const call = route.request().postDataJSON();
    let result: any;
    switch (call.method) {
      case "query.workspace":
        result = {
          active_project: project,
          active_project_id: project.id,
          projects: [project],
          features: [feature],
          total_features: 1,
          total_projects: 1,
          summary: {},
          configurations: [instance],
          repository_ready: true,
          configuration_ready: true,
          definitions: [],
          harnesses: [],
          sandboxes: [],
          detail: {
            feature,
            tasks: [],
            documents: [],
            snapshots: [],
            contexts: [],
            sessions: [],
            totals: {},
          },
        };
        break;
      case "query.workflow_instance":
      case "query.workflow_management_summary":
        result = {
          record: instance,
          runtime: instance.data,
          plan,
          history: [],
          outputs: {},
          children: [],
          completion_status: {
            [execution.id]: {
              satisfied: true,
              confirmation,
              human_satisfied: !!confirmation,
            },
          },
        };
        break;
      case "query.work_configuration":
        result = { configuration: null, history: [] };
        break;
      case "query.standard_catalog":
      case "query.configuration_index":
      case "query.configurations":
        result = { records: [] };
        break;
      case "query.orchestration_summary":
        result = { record: null, decisions: [], calls: [] };
        break;
      case "query.workflow_driver_summary":
        result = { driver: null };
        break;
      case "workflow.step.confirm":
        submitted = call.params;
        confirmation = {
          actor: { kind: "human" },
          input_digest: execution.input_digest,
          outputs: execution.outputs,
        };
        result = { id: instance.id };
        break;
      default:
        return route.fallback();
    }
    return route.fulfill({ json: { jsonrpc: "2.0", id: call.id, result } });
  });
  await page.goto(
    `/?tab=execution&detail=overview&project_id=${project.id}&feature_id=${feature.id}`,
  );
  await page.getByRole("button", { name: /^需求 1 \/ 2/ }).click();
  const review = page.getByRole("article", {
    name: "步骤 · PRD 验收与人工确认",
  });
  await review.getByRole("button", { name: /PRD 验收与人工确认/ }).click();
  await expect(review.getByText(/后续步骤将等待你的确认/)).toBeVisible();
  await expect(
    review.getByRole("button", { name: "接纳结果", exact: true }),
  ).toBeDisabled();
  await expect(
    review.getByRole("link", { name: "核对产物 · 澄清需求与 PRD" }),
  ).toHaveAttribute("href", /output_id=original-prd/);
  await page.screenshot({
    path: info.outputPath("prd-human-confirmation.png"),
    fullPage: true,
  });
  await review
    .getByRole("button", { name: "确认此版本结果", exact: true })
    .click();
  await expect(review.getByText("已人工确认", { exact: true })).toBeVisible();
  await expect(
    review.getByRole("button", { name: "接纳结果", exact: true }),
  ).toBeEnabled();
  expect(submitted.expected_revision).toBe(7);
  expect(submitted.payload).toMatchObject({
    plan_revision_id: "current-plan",
    step: execution.step,
    execution_id: execution.id,
    input_digest: execution.input_digest,
    outputs: execution.outputs,
  });
});

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
    if (
      ["query.workspace", "query.execution_workspace"].includes(call.method)
    ) {
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

test("在工作总览展开分支并选择 Feature，在执行工作台显式启动", async ({
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
  const delivered = {
    ...feature,
    id: "delivered-feature",
    title: "已交付的 Feature",
    status: "completed",
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
  const branchReads: any[] = [];
  await page.route("**/capabilities", (route) =>
    route.fulfill({
      json: { setup_required: false, active_project_id: project.id },
    }),
  );
  await page.route("**/rpc", async (route) => {
    const call = route.request().postDataJSON();
    let result: any;
    switch (call.method) {
      case "query.execution_workspace":
      case "query.workspace":
        result = {
          active_project: project,
          projects: [project],
          features: [feature, delivered],
          total_features: 2,
          total_projects: 1,
          repository_ready: true,
          configuration_ready: true,
          configurations: [configuration],
          summary: {},
          definitions: [],
          harnesses: [],
          sandboxes: [],
          detail: {
            feature:
              call.params.feature_id === delivered.id ? delivered : feature,
            tasks: [],
            documents: [],
            snapshots: [],
            contexts: [],
            totals: {},
          },
        };
        break;
      case "query.feature_branches":
        branchReads.push(call.params);
        result = {
          feature_branch: `devdesk/features/${call.params.feature_id}`,
          candidate:
            call.params.feature_id === delivered.id
              ? {
                  branch: "devdesk/candidates/published",
                  status: "completed",
                  pull_request_number: 31,
                }
              : null,
          tasks:
            call.params.feature_id === delivered.id
              ? [
                  {
                    id: "published-task",
                    title: "实现交付功能",
                    branch: "devdesk/tasks/recovered-source",
                    status: "completed",
                  },
                ]
              : [],
          next_task_offset: null,
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
  await page.goto(`/?tab=overview&project_id=${project.id}`);
  await expect(
    page.getByRole("button", { name: "新 Feature", exact: true }),
  ).toBeVisible();
  await expect(
    page.getByRole("button", { name: "启动 Feature", exact: true }),
  ).toHaveCount(0);
  const queue = page.getByRole("region", { name: /Feature 工作队列/ });
  await expect(queue.getByText("main", { exact: true })).toBeVisible();
  const enter = queue.getByRole("button", { name: "进入执行工作台" });
  await expect(enter).toHaveCount(0);
  expect(branchReads).toHaveLength(0);
  await queue.getByRole("button", { name: `展开 ${delivered.title}` }).click();
  await expect(queue.getByText("devdesk/candidates/published")).toBeVisible();
  await expect(queue.getByRole("link", { name: "PR #31" })).toHaveAttribute(
    "href",
    "https://github.com/fixture/start/pull/31",
  );
  expect(branchReads[0]).toMatchObject({
    project_id: project.id,
    feature_id: delivered.id,
  });
  await expect(queue.getByText("实现交付功能")).toHaveCount(0);
  await queue
    .getByRole("button", { name: "展开 Feature 分支的 Tasks" })
    .click();
  await expect(queue.getByText("devdesk/tasks/recovered-source")).toBeVisible();
  await expect(queue.getByText("已完成", { exact: true })).toHaveCount(4);
  await queue
    .getByRole("button", { name: "折叠 Feature 分支的 Tasks" })
    .click();
  await expect(queue.getByText("实现交付功能")).toHaveCount(0);
  await expect(enter).toHaveCount(0);
  const draft = queue.getByRole("button", { name: feature.title, exact: true });
  await draft.click();
  await expect(draft).toHaveAttribute("aria-pressed", "true");
  await expect(queue.getByText("尚无分支记录")).toBeVisible();
  await expect(
    queue.getByText("工作中", { exact: true }).first(),
  ).toBeVisible();
  await expect(page.getByText("SELECTED FEATURE")).toHaveCount(0);
  expect(submitted).toBeUndefined();
  await enter.click();
  await expect(page).toHaveURL(
    new RegExp(
      `tab=execution&project_id=${project.id}&feature_id=${feature.id}`,
    ),
  );
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

test("总览统计重置筛选并直接打开匹配的 Feature", async ({ page }, info) => {
  await setup(page);
  const project = {
    id: "metrics-project",
    title: "状态统计项目",
    revision: 1,
    workspace_definition: { repository: "fixture/metrics", mainline: "main" },
  };
  const features = ["recent", "review", "blocked"].map((id) => ({
    id,
    project_id: project.id,
    title: `Feature ${id}`,
    revision: 1,
    status: "running",
  }));
  const matches: Record<string, typeof features> = {
    running: [features[0], features[1]],
    waiting_confirmation: [features[1]],
    blocked: [features[2]],
  };
  const shortcuts: {
    project_id: string;
    work_filter: string;
    limit: number;
  }[] = [];
  await page.route("**/capabilities", (route) =>
    route.fulfill({ json: { setup_required: false } }),
  );
  await page.route("**/rpc", async (route) => {
    const call = route.request().postDataJSON();
    if (!["query.workspace", "query.execution_workspace"].includes(call.method))
      return route.fallback();
    const { work_filter, feature_id, limit } = call.params;
    const rows = matches[work_filter] ?? features;
    const active = features.find((feature) => feature.id === feature_id);
    if (limit === 1) shortcuts.push(call.params);
    await route.fulfill({
      json: {
        jsonrpc: "2.0",
        id: call.id,
        result: {
          active_project: project,
          projects: [project],
          total_projects: 1,
          repository_ready: true,
          configuration_ready: true,
          configurations: [],
          summary: {
            features: 3,
            running: 2,
            waiting_confirmation: 1,
            blocked: 1,
          },
          total_features: rows.length,
          features: rows.slice(0, limit),
          detail: active ? { feature: active, tasks: [] } : null,
        },
      },
    });
  });
  const overview = `?tab=overview&project_id=${project.id}`;
  await page.goto(
    `${overview}&filter=running&feature_id=recent&task_id=old-task&output_id=old-output&config_tab=workflow`,
  );
  const metrics = page.getByRole("complementary", { name: "项目统计" });
  const queue = page.getByRole("region", { name: /Feature 工作队列/ });
  await expect(
    queue.getByRole("button", { name: "Feature blocked", exact: true }),
  ).toHaveCount(0);
  await expect(metrics.getByRole("button")).toHaveCount(4);
  await expect(
    page.getByRole("heading", { name: "当前工作流程", exact: true }),
  ).toHaveCount(0);
  await metrics.getByRole("button", { name: /项目 Features/ }).click();
  await expect(page).toHaveURL(`/${overview}`);
  await expect(
    queue.getByRole("button", { name: "Feature blocked", exact: true }),
  ).toBeVisible();
  await expect(
    queue.getByRole("button", { name: "进入执行工作台" }),
  ).toHaveCount(0);
  const queueBox = (await queue.boundingBox())!;
  for (const button of await metrics.getByRole("button").all()) {
    const box = (await button.boundingBox())!;
    expect(box.x).toBeGreaterThan(queueBox.x + queueBox.width);
    expect(Math.abs(box.width - box.height)).toBeLessThan(1);
  }
  await page.screenshot({ path: info.outputPath("overview-statistics.png") });

  for (const [label, filter, id] of [
    ["等待人确认", "waiting_confirmation", "review"],
    ["运行中", "running", "recent"],
    ["需要处理", "blocked", "blocked"],
  ]) {
    await metrics.getByRole("button", { name: new RegExp(label) }).click();
    await expect(page).toHaveURL(
      `/?tab=execution&project_id=${project.id}&feature_id=${id}&detail=manager`,
    );
    await expect(page.getByLabel("当前 Feature")).toHaveValue(id);
    await expect(
      page.getByRole("heading", { name: "Feature 工作队列", exact: true }),
    ).toHaveCount(0);
    expect(shortcuts.at(-1)).toEqual({
      project_id: project.id,
      work_filter: filter,
      limit: 1,
    });
    await page.goBack();
    await expect(metrics).toBeVisible();
  }

  matches.blocked = [];
  await metrics.getByRole("button", { name: /需要处理/ }).click();
  await expect(metrics.getByRole("status")).toHaveText(
    "当前没有符合此状态的 Feature。",
  );
  await expect(page).toHaveURL(`/${overview}`);
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
      case "query.execution_workspace":
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
          output_names: { "original-prd": "PRD" },
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
      case "query.workflow_driver":
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
  const review = page.getByRole("article", {
    name: "步骤 · PRD 验收与人工确认",
  });
  await review.getByRole("button", { name: /PRD 验收与人工确认/ }).click();
  await expect(review.getByText(/后续步骤将等待你的确认/)).toBeVisible();
  await expect(
    review.getByRole("button", { name: "接纳结果", exact: true }),
  ).toBeDisabled();
  await expect(
    review.getByRole("link", { name: "核对产物 · PRD", exact: true }),
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

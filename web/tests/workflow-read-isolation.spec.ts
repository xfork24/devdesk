import { test, expect, type Page } from "@playwright/test";
import { mkdtempSync } from "node:fs";
import { tmpdir } from "node:os";
import { join } from "node:path";

// Controlled UI responses; actual execution and publication are verified separately.
async function fixture(
  page: Page,
  failure?: "catalog" | "defaults",
  metadataOnly = false,
  contexts: any[] = [],
) {
  await page.goto("/");
  if (
    (await page.request.get("/capabilities").then((r) => r.json()))
      .setup_required
  ) {
    await page
      .getByLabel("数据目录")
      .fill(mkdtempSync(join(tmpdir(), "devdesk-read-ui-")));
    await page.getByRole("button", { name: "初始化 / 打开数据集" }).click();
    await expect(page.getByLabel("数据目录")).toHaveCount(0);
  }
  const original = await page.request
    .post("/rpc", {
      data: { jsonrpc: "2.0", id: 1, method: "query.workspace", params: {} },
    })
    .then((r) => r.json());
  const project = {
    id: "read-project",
    title: "读取隔离项目",
    revision: 1,
    workspace_definition: { repository: "fixture/reads", mainline: "main" },
  };
  const feature = {
    id: "read-feature",
    title: "读取隔离 Feature",
    status: "running",
    revision: 1,
  };
  const document = { stages: [], steps: [] };
  const instance = {
    id: "read-instance",
    type: "workflow_instance",
    name: "读取隔离流程",
    revision: 1,
    data: {
      owner_id: feature.id,
      parent_execution_id: null,
      status: "running",
      paused: false,
      steps: [],
      plan: { document, standard_ref: "standard-current:1" },
      active_plan_revision_id: "plan-current",
      role_agent_bindings: {},
    },
  };
  const history = ["standard-old:1", "standard-current:1"].map(
    (ref, index) => ({
      id: `plan-${index}`,
      data: { standard_ref: ref, reason: `采用记录 ${index + 1}` },
    }),
  );
  await page.route("**/capabilities", (route) =>
    route.fulfill({
      json: {
        setup_required: false,
        active_project_id: project.id,
      },
    }),
  );
  await page.route("**/rpc", async (route) => {
    const call = route.request().postDataJSON();
    let result: any;
    switch (call.method) {
      case "query.execution_workspace":
      case "query.workspace":
        result = {
          ...original.result,
          active_project: project,
          projects: [project],
          total_projects: 1,
          repository_ready: true,
          configuration_ready: true,
          features: [feature],
          total_features: 1,
          configurations: [
            metadataOnly
              ? {
                  ...instance,
                  work_root: true,
                  metadata_only: true,
                  data: {
                    owner_id: feature.id,
                    root_instance_id: instance.id,
                    parent_execution_id: null,
                    active_plan_revision_id: "plan-current",
                    workflow_ref: "workflow-current:1",
                    status: "running",
                    plan: { standard_ref: "standard-current:1" },
                  },
                }
              : instance,
          ],
          detail: {
            feature,
            tasks: [],
            documents: [],
            snapshots: [],
            contexts,
            totals: {},
          },
        };
        break;
      case "query.workflow_progress":
        result = {
          project_id: project.id,
          record: instance,
          counts: { total: 1, completed: 0, running: 1, waiting: 0, failed: 0 },
          stages: [
            {
              key: "work",
              name: "当前阶段摘要",
              counts: {
                total: 1,
                completed: 0,
                running: 1,
                waiting: 0,
                failed: 0,
              },
            },
          ],
          next_stage_offset: null,
          observed_only: true,
        };
        break;
      case "query.workflow_instance":
      case "query.workflow_management_summary":
        result = {
          record: instance,
          runtime: instance.data,
          plan: document,
          history: [],
          outputs: {},
        };
        break;
      case "query.work_configuration":
        result = { configuration: null, history };
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
      case "query.orchestration":
      case "query.orchestration_summary":
        result = { record: null };
        break;
      case "query.workflow_driver":
      case "query.workflow_driver_summary":
        result = { driver: null };
        break;
      default:
        return route.fallback();
    }
    if (
      (failure === "catalog" && call.method === "query.configuration_index") ||
      (failure === "defaults" && call.method === "query.project_configuration")
    ) {
      await new Promise((resolve) => setTimeout(resolve, 200));
      return route.fulfill({
        json: {
          jsonrpc: "2.0",
          id: call.id,
          error: {
            code: -32000,
            message: "controlled read failure",
            data: { code: "read_failed" },
          },
        },
      });
    }
    await route.fulfill({ json: { jsonrpc: "2.0", id: call.id, result } });
  });
  return "/?tab=execution&detail=manager&project_id=read-project&feature_id=read-feature";
}

test("a manager material link keeps its Feature scope before workspace loading finishes", async ({
  page,
}) => {
  const url = await fixture(page);
  const owners: string[] = [];
  let release!: () => void;
  const workspace = new Promise<void>((resolve) => {
    release = resolve;
  });
  await page.route("**/rpc", async (route) => {
    const call = route.request().postDataJSON();
    if (
      ["query.workspace", "query.execution_workspace"].includes(call.method)
    ) {
      await workspace;
      return route.fallback();
    }
    if (call.method !== "query.workflow_materials") return route.fallback();
    owners.push(call.params.owner_id);
    const result = call.params.output_id
      ? {
          record: { delivery_name: "冻结 PRD", round: 1 },
          output: { delivery_ref: "prd:1", value: "本轮需求正文" },
          definitions: {},
        }
      : { records: [], total: 0 };
    return route.fulfill({
      json: { jsonrpc: "2.0", id: call.id, result },
    });
  });
  try {
    await page.goto(
      url.replace("detail=manager", "detail=documents") +
        "&output_id=frozen-prd&execution_id=prd-original",
    );
    await expect(
      page.locator("#material-frozen-prd").getByRole("heading", {
        name: "冻结 PRD",
        exact: true,
      }),
    ).toBeVisible();
    expect(owners.length).toBeGreaterThan(0);
    expect(owners.every((owner) => owner === "read-feature")).toBe(true);
  } finally {
    release();
  }
});

test("continuous runtime events refresh the manager without starving or busy polling", async ({
  page,
}) => {
  const url = await fixture(page);
  // Controlled transport, real page/hook/cache: notifications keep arriving
  // faster than the refresh interval, just like an active multi-step root.
  await page.addInitScript(() => {
    class RuntimeEvents {
      onopen: (() => void) | null = null;
      onmessage: ((event: { data: string }) => void) | null = null;
      onerror = null;
      timer: ReturnType<typeof setInterval>;
      constructor() {
        setTimeout(() => this.onopen?.(), 0);
        this.timer = setInterval(
          () =>
            this.onmessage?.({
              data: JSON.stringify({
                kind: "workflow.runtime",
                detail: JSON.stringify({
                  instance_id: "read-instance",
                  root_instance_id: "read-instance",
                }),
              }),
            }),
          30,
        );
      }
      close() {
        clearInterval(this.timer);
      }
    }
    Object.defineProperty(window, "EventSource", { value: RuntimeEvents });
  });
  let reads = 0;
  await page.route("**/rpc", async (route) => {
    if (route.request().postDataJSON().method === "query.workflow_progress")
      reads++;
    await route.fallback();
  });
  await page.goto(url);
  await expect(
    page.getByRole("button", { name: "刷新管理状态", exact: true }),
  ).toBeVisible();
  const initial = reads;
  await expect
    .poll(() => reads, { timeout: 2000 })
    .toBeGreaterThan(initial + 2);
  expect(reads - initial).toBeLessThan(12);
});

test("another root's events do not reread the current Feature context workspace", async ({
  page,
}) => {
  const url = await fixture(page);
  await page.addInitScript(() => {
    class ForeignEvents {
      onopen: (() => void) | null = null;
      onmessage: ((event: { data: string }) => void) | null = null;
      onerror = null;
      timer: ReturnType<typeof setInterval>;
      constructor() {
        Reflect.set(window, "runtimeEventCount", 0);
        setTimeout(() => this.onopen?.(), 0);
        this.timer = setInterval(() => {
          Reflect.set(
            window,
            "runtimeEventCount",
            Reflect.get(window, "runtimeEventCount") + 1,
          );
          this.onmessage?.({
            data: JSON.stringify({
              kind: "workflow.runtime",
              detail: JSON.stringify({
                instance_id: "another-root",
                root_instance_id: "another-root",
              }),
            }),
          });
        }, 30);
      }
      close() {
        clearInterval(this.timer);
      }
    }
    Object.defineProperty(window, "EventSource", { value: ForeignEvents });
  });
  let reads = 0;
  await page.route("**/rpc", async (route) => {
    if (
      ["query.workspace", "query.execution_workspace"].includes(
        route.request().postDataJSON().method,
      )
    )
      reads++;
    await route.fallback();
  });
  await page.goto(url.replace("detail=manager", "detail=context"));
  await expect(page.getByLabel("当前 Feature", { exact: true })).toHaveValue(
    "read-feature",
  );
  await page.waitForFunction(
    () => Reflect.get(window, "runtimeEventCount") >= 10,
  );
  const initial = reads;
  await page.waitForFunction(
    () => Reflect.get(window, "runtimeEventCount") >= 30,
  );
  expect(reads).toBe(initial);
});

test("manager prepares exact materials and review evidence for a human decision", async ({
  page,
}) => {
  const url = await fixture(page);
  let decision: any;
  const reference = {
    label: "本轮 PRD",
    purpose: "请核对需求范围和验收标准",
    owner_id: "read-feature",
    instance_id: "read-instance",
    execution_id: "prd-original",
    output_id: "frozen-prd",
    session_id: "prd-session",
    turn_id: "prd-turn",
  };
  await page.route("**/rpc", async (route) => {
    const call = route.request().postDataJSON();
    if (call.method === "query.orchestration") {
      await route.fulfill({
        json: {
          jsonrpc: "2.0",
          id: call.id,
          result: {
            record: {
              id: "controller",
              data: { status: "active", owner_id: "read-feature" },
            },
            decisions: [
              {
                id: "human-request",
                revision: 7,
                data: {
                  status: decision ? "approved" : "pending",
                  request: {
                    operation: "workflow.step.confirm",
                    arguments: { execution_id: "review-original" },
                    interaction: {
                      title: "本轮 PRD",
                      background: "需求文档和独立初审已经完成。",
                      requested_action:
                        "请确认范围与验收标准；如需修改可退回并填写意见。",
                      completed_work:
                        "已安排需求 Agent 编写 PRD，并由独立审核 Agent 完成初审。",
                      review_summary: "初审通过，无阻塞问题，尚待你确认。",
                      materials: [
                        reference,
                        {
                          ...reference,
                          label: "本轮初审报告",
                          output_id: "frozen-review",
                          execution_id: "review-original",
                          session_id: "review-session",
                          turn_id: "review-turn",
                        },
                      ],
                      evidence: [
                        {
                          ...reference,
                          label: "独立初审",
                          output_id: "frozen-review",
                          execution_id: "review-original",
                          session_id: "review-session",
                          turn_id: "review-turn",
                        },
                      ],
                    },
                  },
                },
              },
            ],
            calls: [
              {
                id: "call",
                data: { method: "workflow.inspect", status: "succeeded" },
              },
            ],
          },
        },
      });
    } else if (call.method === "workflow.decision.resolve") {
      decision = call.params;
      await route.fulfill({
        json: { jsonrpc: "2.0", id: call.id, result: { id: "human-request" } },
      });
    } else return route.fallback();
  });
  await page.goto(url);
  const manager = page.getByRole("region", {
    name: "项目经理协作",
    exact: true,
  });
  const card = manager.getByRole("article", { name: "本轮 PRD", exact: true });
  await expect(card).toContainText("需要你做什么");
  await expect(card).toContainText("独立审核 Agent 完成初审");
  await expect(card).toContainText("初审通过");
  await expect(
    card.getByRole("link", { name: "本轮 PRD", exact: true }),
  ).toHaveAttribute("href", /output_id=frozen-prd/);
  await expect(
    card.getByRole("link", { name: "材料上下文", exact: true }).first(),
  ).toHaveAttribute("href", /session_id=prd-session/);
  await expect(
    card.getByRole("link", { name: "材料执行记录", exact: true }).first(),
  ).toHaveAttribute("href", /execution_id=prd-original/);
  await expect(
    card.getByRole("link", { name: "查看上下文", exact: true }),
  ).toHaveAttribute("href", /session_id=review-session/);
  await expect(
    card.getByRole("link", { name: "查看依据原件", exact: true }),
  ).toHaveAttribute("href", /output_id=frozen-review/);
  await expect(
    card.getByRole("link", { name: "查看执行记录", exact: true }),
  ).toHaveAttribute("href", /execution_id=review-original/);
  await expect(manager.getByText("查看确切请求", { exact: true })).toHaveCount(
    0,
  );
  await expect(
    manager.getByText("调度与经理管理", { exact: true }),
  ).toHaveCount(0);
  await expect(manager.getByText(/workflow\.inspect/)).toHaveCount(0);
  await page.setViewportSize({ width: 390, height: 844 });
  await expect(
    card.getByRole("link", { name: "本轮 PRD", exact: true }),
  ).toBeVisible();
  await card
    .getByLabel("意见或修改要求", { exact: true })
    .fill("确认当前范围，保留初审记录。");
  await card
    .getByRole("button", { name: "确认：本轮 PRD", exact: true })
    .click();
  await expect
    .poll(() => decision?.payload)
    .toEqual({ approved: true, reason: "确认当前范围，保留初审记录。" });
  expect(decision.target).toBe("human-request");
  expect(decision.expected_revision).toBe(7);
  await expect(card).toContainText("你已确认本轮事项");
});

test("manager scopes child confirmations and tool approvals, with errors beside the relevant card", async ({
  page,
}) => {
  const url = await fixture(page);
  let attempts = 0;
  let approved = false;
  let toolApproved = false;
  let managerStatus = "paused";
  const commands: any[] = [];
  const args = {
    plan_revision_id: "child-plan",
    control_version: 4,
    step: "review",
    execution_id: "child-execution",
    input_digest: "frozen-digest",
    outputs: { result: "frozen-output" },
    accept_result: true,
  };
  const plan = { stages: [], steps: [] };
  const rootRecord = {
    id: "read-instance",
    name: "读取隔离流程",
    revision: 1,
    data: {
      owner_id: "read-feature",
      root_instance_id: "read-instance",
      status: "running",
      steps: [],
      plan: { document: plan },
      active_plan_revision_id: "plan-current",
    },
  };
  const childRecord = {
    id: "child-instance",
    name: "当前Task",
    revision: 8,
    data: {
      owner_id: "read-feature",
      root_instance_id: "read-instance",
      parent_execution_id: "parent-execution",
      status: "running",
      steps: [],
      plan: { document: plan },
      active_plan_revision_id: "child-plan",
    },
  };
  await page.route("**/rpc", async (route) => {
    const call = route.request().postDataJSON();
    let result: any;
    if (
      ["query.workflow_instance", "query.workflow_management_summary"].includes(
        call.method,
      )
    ) {
      const record =
        call.params.id === "child-instance" ? childRecord : rootRecord;
      result = {
        record,
        runtime: record.data,
        plan,
        history: [],
        children: record.id === rootRecord.id ? [childRecord] : [],
      };
    } else if (call.method === "query.orchestration") {
      result = {
        record: {
          id: "shared-controller",
          revision: 3,
          data: {
            status: managerStatus,
            epoch: 7,
            owner_id: "read-feature",
            session_id: "shared-manager-session",
          },
        },
        decisions: [],
        messages: [
          {
            id: "manager-message",
            session_id: "shared-manager-session",
            turn_id: "manager-turn",
            sequence: 1,
            content_excerpt: "已同步当前流程进度",
            truncated: false,
          },
          {
            id: "manager-message-long",
            session_id: "shared-manager-session",
            turn_id: "manager-turn-long",
            sequence: 2,
            content_excerpt: "正在核对任务交付与后续执行计划",
            truncated: true,
          },
        ],
      };
    } else if (call.method === "orchestration.resume") {
      commands.push(call);
      managerStatus = "active";
      result = { resumed: true };
    } else if (call.method === "query.workflow_driver") {
      result = {
        driver: null,
        waiting: approved
          ? []
          : [
              {
                instance_id: "child-instance",
                owner_id: "read-feature",
                execution_id: "child-execution",
                ancestor_instance_ids: ["read-instance"],
                step: "review",
                step_name: "子流程待确认点",
                status: "waiting_confirmation",
              },
            ],
        approvals: approved
          ? []
          : [
              {
                instance_id: "child-instance",
                owner_id: "read-feature",
                ancestor_instance_ids: ["read-instance"],
                revision: 8,
                operation: "workflow.step.confirm",
                arguments: args,
                interaction: {
                  title: "确认子流程需求",
                  background: "子流程等待确认",
                  requested_action: "确认当前版本后继续",
                  completed_work: "需求已完成",
                  review_summary: "检查通过",
                  materials: [],
                  evidence: [],
                },
              },
            ],
        runtime_approvals: toolApproved
          ? []
          : [
              {
                id: "tool-interaction",
                instance_id: "child-instance",
                ancestor_instance_ids: ["read-instance"],
                revision: 3,
                session_id: "child-session",
                turn_id: "child-turn",
                reason: "执行测试需要工具权限",
                tool: "test.run",
              },
            ],
      };
    } else if (call.method === "workflow.step.confirm") {
      commands.push(call);
      if (++attempts === 1)
        return route.fulfill({
          json: {
            jsonrpc: "2.0",
            id: call.id,
            error: { code: -32000, message: "审批暂不可用，请重试" },
          },
        });
      approved = true;
      result = { confirmed: true };
    } else if (call.method === "interaction.answer") {
      commands.push(call);
      toolApproved = true;
      result = { answered: true };
    } else return route.fallback();
    return route.fulfill({ json: { jsonrpc: "2.0", id: call.id, result } });
  });
  await page.goto(url);
  const childPanel = page.locator(".manager-child-flow");
  await expect(childPanel).toBeVisible();
  await expect(childPanel.locator(":scope > summary")).toHaveCount(0);
  const mainCollaboration = page
    .locator(".manager-workflow-summary")
    .getByRole("region", { name: "项目经理协作", exact: true });
  const childCollaboration = childPanel.getByRole("region", {
    name: "项目经理协作",
    exact: true,
  });
  await expect(
    childCollaboration.getByRole("heading", {
      name: "项目经理协作",
      exact: true,
    }),
  ).toBeVisible();
  await expect(
    childCollaboration.getByRole("region", { name: "当前阻塞点", exact: true }),
  ).toContainText("子流程待确认点");
  await expect(
    mainCollaboration.getByRole("region", { name: "当前阻塞点", exact: true }),
  ).toContainText("子流程待确认点");
  await expect(
    childCollaboration.getByRole("link", {
      name: "与项目经理继续讨论",
      exact: true,
    }),
  ).toHaveAttribute("href", /session_id=shared-manager-session/);
  await expect(childCollaboration.locator(".manager-log")).not.toHaveAttribute(
    "open",
  );
  await childCollaboration.locator(".manager-log > summary").click();
  await expect(
    childCollaboration.getByText("已同步当前流程进度", { exact: true }),
  ).toBeVisible();
  const log = childCollaboration.locator(".manager-log");
  await expect(log.locator(".notice")).toHaveCount(2);
  await expect(log.getByText("正在核对任务交付与后续执行计划…")).toBeVisible();
  const fullLog = log.getByRole("link", { name: "查看完整日志", exact: true });
  await expect(fullLog).toHaveCount(1);
  await expect(fullLog).toHaveAttribute(
    "href",
    /session_id=shared-manager-session/,
  );
  await expect(fullLog).toHaveAttribute("href", /turn_id=manager-turn-long/);
  await childCollaboration
    .getByRole("button", { name: "恢复项目经理", exact: true })
    .click();
  expect(commands.at(-1).params).toMatchObject({
    target: "shared-controller",
    expected_revision: 3,
    payload: { epoch: 7 },
  });
  await expect(
    mainCollaboration.getByRole("button", { name: "人工接管", exact: true }),
  ).toBeVisible();
  await expect(
    childCollaboration.getByRole("button", { name: "人工接管", exact: true }),
  ).toBeVisible();
  await expect(
    mainCollaboration.getByRole("article", {
      name: "确认子流程需求",
      exact: true,
    }),
  ).toHaveCount(0);
  const card = childCollaboration.getByRole("article", {
    name: "确认子流程需求",
    exact: true,
  });
  await expect(
    card.getByRole("button", { name: "退回并反馈意见", exact: true }),
  ).toBeDisabled();
  await card
    .getByRole("button", { name: "确认：确认子流程需求", exact: true })
    .click();
  await expect(card.getByRole("alert")).toContainText("审批暂不可用，请重试");
  await card
    .getByRole("button", { name: "确认：确认子流程需求", exact: true })
    .click();
  await expect(card).toHaveCount(0);
  expect(commands.at(-1).params).toMatchObject({
    target: "child-instance",
    expected_revision: 8,
    payload: args,
  });
  await expect(
    mainCollaboration.getByRole("article", {
      name: "运行时工具审批",
      exact: true,
    }),
  ).toHaveCount(0);
  const tool = childCollaboration.getByRole("article", {
    name: "运行时工具审批",
    exact: true,
  });
  await tool.getByRole("button", { name: "仅允许本次", exact: true }).click();
  await expect(tool).toHaveCount(0);
  expect(commands.at(-1).params).toMatchObject({
    target: "tool-interaction",
    expected_revision: 3,
    payload: {
      session_id: "child-session",
      turn_id: "child-turn",
      answer: { kind: "tool_approval", outcome: "allowed_once" },
    },
  });
});

test("slow controller polling renders its result without overlapping requests and stops on unmount", async ({
  page,
}) => {
  const url = await fixture(page);
  let active = 0,
    maximum = 0,
    calls = 0;
  await page.route("**/rpc", async (route) => {
    const call = route.request().postDataJSON();
    if (call.method !== "query.orchestration") return route.fallback();
    calls++;
    active++;
    maximum = Math.max(maximum, active);
    try {
      await new Promise((resolve) => setTimeout(resolve, 4000));
      await route.fulfill({
        json: {
          jsonrpc: "2.0",
          id: call.id,
          result: {
            record: {
              id: "controller",
              revision: 1,
              data: { status: "active", reason: "slow controller observed" },
            },
            calls: [],
            messages: [],
            decisions: [],
          },
        },
      });
    } finally {
      active--;
    }
  });
  await page.goto(url);
  await expect(
    page
      .getByRole("region", { name: "项目经理协作" })
      .getByText("slow controller observed"),
  ).toBeVisible({ timeout: 6500 });
  expect(maximum).toBe(1);
  await page.getByRole("tab", { name: "工作流程", exact: true }).click();
  const before = calls;
  await page.waitForTimeout(3000);
  expect(calls).toBe(before);
});

test("manager context pins its session through delayed role discovery and accepts manager messages", async ({
  page,
}) => {
  page.on("pageerror", (error) => {
    throw error;
  });
  const url = await fixture(page);
  const manager = {
    id: "manager-agent:1",
    type: "agent",
    definition_id: "manager-agent",
    name: "项目经理",
    version: 1,
    role_ref: "manager-role:1",
    capability_ref: "manager-capability:1",
    data: {},
  };
  const instance = {
    id: "manager-instance:1",
    type: "agent_instance",
    definition_id: "manager-instance",
    name: "项目经理实例",
    scope: "global",
    data: { agent_revision_id: manager.id },
  };
  const binding = {
    installation: { key: { provider: "codex" } },
    agent_configuration: { agent_instance_id: instance.id },
    model: "fixture-model",
  };
  const session = {
    id: "manager-session",
    owner_id: "read-feature",
    agent: manager.id,
    runtime_binding: binding,
    lifecycle: "idle",
  };
  const controller = {
    id: "manager-controller",
    revision: 3,
    data: { epoch: 1, status: "active" },
  };
  let submitted: any;
  let chatApproved = false;
  const lookups: any[] = [];
  await page.route("**/rpc", async (route) => {
    const call = route.request().postDataJSON();
    let result: any;
    switch (call.method) {
      case "query.workflow_contexts":
        result = {
          agents: [
            {
              id: instance.id,
              actor: { name: manager.name, agent_revision_id: manager.id },
              entries: [
                {
                  kind: "controller",
                  owner_id: "read-feature",
                  session_id: session.id,
                  turn_id: "manager-turn",
                  status: "completed",
                  actor: { agent_revision_id: manager.id },
                  historical: false,
                },
              ],
            },
          ],
        };
        break;
      case "query.configuration_index":
      case "query.configurations":
        result = {
          records: [
            {
              ...manager,
              id: "worker:1",
              definition_id: "worker",
              name: "需求分析师",
            },
            manager,
            instance,
            {
              id: "manager-role:1",
              type: "role",
              definition_id: "manager-role",
            },
            { id: "manager-capability:1", type: "capability", data: {} },
          ],
          total: 5,
        };
        break;
      case "query.sessions":
        lookups.push(call.params);
        result = {
          sessions: [{ ...session, id: "newer-manager-session" }, session],
        };
        break;
      case "query.session":
        expect(call.params.id).toBe(session.id);
        await new Promise((resolve) => setTimeout(resolve, 350));
        result = {
          session,
          controller,
          approval_context: chatApproved
            ? []
            : [
                {
                  id: "observed-decision",
                  revision: 7,
                  data: { request: { interaction: { title: "确认本轮PRD" } } },
                },
              ],
          turns: [{ id: "manager-turn", status: "completed" }],
          events: [
            {
              id: "manager-response",
              turn_id: "manager-turn",
              kind: "assistant",
              content: "经理会话已正确加载",
              sequence: 1,
            },
          ],
          next_cursor: 1,
          interactions: [],
          proposals: [],
          executions: [],
          formal_execution: true,
        };
        break;
      case "query.agent_experience":
        result = { records: [], total: 0, unavailable: [] };
        break;
      case "orchestration.input":
        submitted = call.params;
        chatApproved =
          call.params.payload.input === "确认PRD，继续执行后续步骤";
        result = {
          accepted: true,
          ...(chatApproved ? { approval: { status: "approved" } } : {}),
        };
        break;
      default:
        return route.fallback();
    }
    await route.fulfill({ json: { jsonrpc: "2.0", id: call.id, result } });
  });
  await page.goto(url.replace("detail=manager", "detail=context"));
  await page.getByRole("link", { name: "查看此轮会话", exact: true }).click();
  await expect(page.getByRole("log", { name: "讨论记录" })).toContainText(
    "经理会话已正确加载",
  );
  await expect(page.getByLabel("讨论角色", { exact: true })).toHaveValue(
    manager.id,
  );
  await expect(page.getByLabel("Agent", { exact: true })).toHaveValue(
    instance.id,
  );
  await expect(page.getByLabel("讨论会话", { exact: true })).toHaveValue(
    session.id,
  );
  await expect(page.getByLabel("讨论轮次", { exact: true })).toHaveValue(
    "manager-turn",
  );
  expect(
    lookups.every((params) => !params.agent && !params.agent_instance_id),
  ).toBe(true);
  await page.getByRole("button", { name: "讨论输入", exact: true }).click();
  await page
    .getByRole("textbox", { name: "讨论输入", exact: true })
    .fill("只导出 ID 和名称。");
  await page.getByRole("button", { name: "发送", exact: true }).click();
  await expect
    .poll(() => submitted)
    .toMatchObject({
      target: controller.id,
      payload: {
        epoch: 1,
        input: "只导出 ID 和名称。",
        approval_context: [{ id: "observed-decision", revision: 7 }],
      },
    });
  await expect(
    page.getByText("当前待审批：确认本轮PRD", { exact: true }),
  ).toBeVisible();
  await page.getByRole("button", { name: "讨论输入", exact: true }).click();
  await page
    .getByRole("textbox", { name: "讨论输入", exact: true })
    .fill("确认PRD，继续执行后续步骤");
  await page.getByRole("button", { name: "发送", exact: true }).click();
  await expect
    .poll(() => submitted.payload.input)
    .toBe("确认PRD，继续执行后续步骤");
  await expect(
    page.getByText(
      "已登记人工批准并处理当前结果，后续步骤由流程驱动继续执行。",
      { exact: true },
    ),
  ).toBeVisible();
});

test("manager shows the current blocker without scheduling configuration at desktop and mobile widths", async ({
  page,
}) => {
  const url = await fixture(page);
  await page.route("**/rpc", async (route) => {
    const call = route.request().postDataJSON();
    let result: any;
    if (call.method === "query.orchestration")
      result = { record: { id: "controller", data: { status: "active" } } };
    else if (call.method === "query.workflow_driver")
      result = {
        driver: { data: { mode: "agent", epoch: 1 } },
        waiting: [
          {
            instance_id: "read-instance",
            execution_id: "requirements-execution",
            step: "requirements",
            step_name: "澄清需求与PRD",
            status: "waiting_input",
            reason: "需要导出哪些字段？",
            actions: {},
            waiting_for: [],
          },
        ],
      };
    else return route.fallback();
    await route.fulfill({ json: { jsonrpc: "2.0", id: call.id, result } });
  });
  await page.goto(url);
  const manager = page.getByRole("region", {
    name: "项目经理协作",
    exact: true,
  });
  await expect(
    manager.getByRole("region", { name: "当前阻塞点" }),
  ).toContainText("需要导出哪些字段？");
  await expect(
    manager.getByText("调度与经理管理", { exact: true }),
  ).toHaveCount(0);
  await expect(manager.getByLabel("切换调度模式", { exact: true })).toHaveCount(
    0,
  );
  await expect(manager.locator(".manager-configuration-row")).toHaveCount(0);
  await page.setViewportSize({ width: 390, height: 844 });
  await expect(manager.getByLabel("回复项目经理", { exact: true })).toHaveCount(
    0,
  );
  expect(
    await page.evaluate(
      () => document.documentElement.scrollWidth <= innerWidth,
    ),
  ).toBe(true);
});

test("流程经理只保留审批入口和接管操作，状态刷新恢复后清除超时提示", async ({
  page,
}) => {
  const url = await fixture(page);
  let failRead = false;
  let status = "active";
  const commands: { method: string; params: any }[] = [];
  await page.route("**/rpc", async (route) => {
    const call = route.request().postDataJSON();
    if (call.method === "query.orchestration") {
      expect(call.params.root_instance_id).toBe("read-instance");
      if (failRead)
        return route.fulfill({
          json: {
            jsonrpc: "2.0",
            id: call.id,
            error: { code: -32000, message: "TimeoutError: signal timed out" },
          },
        });
      return route.fulfill({
        json: {
          jsonrpc: "2.0",
          id: call.id,
          result: {
            record: {
              id: "flow-controller",
              revision: 2,
              data: { status, epoch: 1, owner_id: "read-feature" },
            },
          },
        },
      });
    }
    if (
      [
        "orchestration.input",
        "orchestration.takeover",
        "orchestration.resume",
      ].includes(call.method)
    ) {
      commands.push({ method: call.method, params: call.params });
      if (call.method === "orchestration.takeover") status = "paused";
      if (call.method === "orchestration.resume") status = "active";
      return route.fulfill({
        json: { jsonrpc: "2.0", id: call.id, result: { accepted: true } },
      });
    }
    return route.fallback();
  });
  await page.goto(url);
  const manager = page.getByRole("region", {
    name: "项目经理协作",
    exact: true,
  });
  await expect(manager).toContainText("协作中");
  await expect(manager).toContainText("在此处理当前流程的待确认事项");
  failRead = true;
  await page.getByRole("button", { name: "刷新管理状态", exact: true }).click();
  await expect(manager).toContainText("暂时无法刷新经理状态");
  failRead = false;
  await page.getByRole("button", { name: "刷新管理状态", exact: true }).click();
  await expect(manager).not.toContainText("暂时无法刷新经理状态");
  await expect(manager.getByLabel("回复项目经理", { exact: true })).toHaveCount(
    0,
  );
  await expect(
    manager.getByRole("button", { name: "发送回复", exact: true }),
  ).toHaveCount(0);
  await manager.getByRole("button", { name: "人工接管", exact: true }).click();
  await expect(
    manager.getByRole("button", { name: "恢复项目经理", exact: true }),
  ).toBeVisible();
  expect(
    commands.find((command) => command.method === "orchestration.takeover")
      ?.params.target,
  ).toBe("flow-controller");
  await manager
    .getByRole("button", { name: "恢复项目经理", exact: true })
    .click();
  await expect(manager).toContainText("协作中");
});

test("paused root manager resumes its frozen sources without configuration controls", async ({
  page,
}) => {
  const url = await fixture(page);
  let adopted: Record<string, unknown> | undefined;
  await page.route("**/rpc", async (route) => {
    const call = route.request().postDataJSON();
    let result: any;
    if (call.method === "query.orchestration") {
      result = {
        record: {
          id: "controller",
          revision: adopted ? 2 : 1,
          data: {
            status: adopted ? "active" : "paused",
            epoch: adopted ? 4 : 3,
            role_ref: "manager-role:1",
            agent_revision_id: "manager-old:1",
            harness_permission_ref: "context-old:1",
          },
        },
        calls: [],
        messages: [],
        decisions: [],
      };
    } else if (call.method === "query.configuration_index") {
      result = {
        records: [
          {
            id: "manager-new:1",
            type: "agent",
            definition_id: "manager-new",
            name: "Selected manager",
            version: 1,
            scope: "global",
            role_ref: "manager-role:1",
            data: {},
          },
          {
            id: "developer-agent:1",
            type: "agent",
            definition_id: "developer-agent",
            name: "Developer cannot manage projects",
            version: 1,
            scope: "global",
            role_ref: "developer-role:1",
            data: {},
          },
          {
            id: "context-new:1",
            type: "permission_profile",
            definition_id: "context-new",
            name: "Selected budget",
            version: 1,
            scope: "global",
            data: { target_kind: "harness" },
          },
        ],
        total: 3,
      };
    } else if (call.method === "orchestration.resume") {
      adopted = call.params.payload;
      result = { id: "controller" };
    } else return route.fallback();
    await route.fulfill({ json: { jsonrpc: "2.0", id: call.id, result } });
  });
  await page.goto(url);
  const panel = page.getByRole("region", { name: "项目经理协作" });
  await expect(panel.getByText("调度与经理管理", { exact: true })).toHaveCount(
    0,
  );
  await expect(
    panel.getByLabel("经理 Agent 精确修订", { exact: true }),
  ).toHaveCount(0);
  await panel
    .getByRole("button", { name: "恢复项目经理", exact: true })
    .click();
  await expect.poll(() => adopted).toEqual({ epoch: 3 });
  await expect(
    panel.getByRole("button", { name: "人工接管", exact: true }),
  ).toBeVisible();
});

for (const failure of ["catalog", "defaults"] as const) {
  test(`Standard details keep errors visible when ${failure} loading fails`, async ({
    page,
  }) => {
    const url = await fixture(page, failure);
    await page.goto(url);
    const standard = page
      .getByRole("region", { name: "流程规范", exact: true })
      .locator(".work-standard");
    await standard
      .getByRole("button", { name: "采用的规范", exact: true })
      .click();
    await expect(standard.getByRole("alert")).toContainText(
      "controlled read failure",
    );
    await expect(standard.getByText(/规范采用历史/)).toHaveCount(0);
    await expect(
      standard.getByText("与其他工作比较规范", { exact: true }),
    ).toHaveCount(0);
  });
}

test("Standard and authorization open independently on one row without comparison or history controls", async ({
  page,
}, info) => {
  const url = await fixture(page);
  const reads: string[] = [];
  page.on("request", (request) => {
    if (request.url().endsWith("/rpc"))
      reads.push(request.postDataJSON().method);
  });
  await page.goto(url);
  const standard = page
    .getByRole("region", { name: "流程规范", exact: true })
    .locator(".work-standard");
  const adopted = standard.getByRole("button", {
    name: "采用的规范",
    exact: true,
  });
  const authorization = standard.getByRole("button", {
    name: "执行者与授权",
    exact: true,
  });
  await expect(adopted).toHaveAttribute("aria-expanded", "false");
  await expect(authorization).toHaveAttribute("aria-expanded", "false");
  const [left, right] = await Promise.all([
    adopted.boundingBox(),
    authorization.boundingBox(),
  ]);
  expect(Math.abs(left!.y - right!.y)).toBeLessThan(2);
  await authorization.click();
  await expect(
    standard.getByRole("region", { name: "执行者与授权内容", exact: true }),
  ).toBeVisible();
  await expect(
    standard.getByRole("region", { name: "采用的规范内容", exact: true }),
  ).toBeHidden();
  await expect(adopted).toHaveAttribute("aria-expanded", "false");
  await adopted.click();
  await expect(
    standard.getByRole("region", { name: "采用的规范内容", exact: true }),
  ).toBeVisible();
  await expect(authorization).toHaveAttribute("aria-expanded", "true");
  await expect(
    standard.getByRole("button", { name: "保存采用的规范", exact: true }),
  ).toHaveCount(0);
  await expect(
    standard.getByText("与其他工作比较规范", { exact: true }),
  ).toHaveCount(0);
  await expect(standard.getByText(/规范采用历史/)).toHaveCount(0);
  await expect(standard.getByText(/展开查看/)).toHaveCount(0);
  expect(reads).not.toContain("query.work_standard_choices");
  expect(reads).not.toContain("query.work_standard_diff");
  await page.screenshot({
    path: info.outputPath("standard-and-authorization.png"),
    fullPage: true,
  });
  await page.setViewportSize({ width: 390, height: 844 });
  const [narrowLeft, narrowRight] = await Promise.all([
    adopted.boundingBox(),
    authorization.boundingBox(),
  ]);
  expect(Math.abs(narrowLeft!.y - narrowRight!.y)).toBeLessThan(2);
  expect(
    await page.evaluate(
      () => document.documentElement.scrollWidth <= innerWidth,
    ),
  ).toBe(true);
});

for (const catalogState of ["pending", "failed"] as const) {
  test(`manager runtime remains visible when configuration catalog is ${catalogState}`, async ({
    page,
  }) => {
    const url = await fixture(page);
    let release!: () => void;
    const blocked = new Promise<void>((resolve) => {
      release = resolve;
    });
    await page.route("**/rpc", async (route) => {
      const call = route.request().postDataJSON();
      let result: any;
      if (call.method === "query.configuration_index") {
        if (catalogState === "pending") await blocked;
        return route.fulfill({
          json: {
            jsonrpc: "2.0",
            id: call.id,
            error: {
              code: -32000,
              message: "catalog unavailable",
              data: { code: "read_failed" },
            },
          },
        });
      } else if (call.method === "query.orchestration") {
        result = {
          record: {
            id: "controller",
            revision: 1,
            data: {
              status: "active",
              epoch: 5,
              reason: "actual manager is active",
            },
          },
          calls: [],
          messages: [],
          decisions: [],
        };
      } else if (call.method === "query.workflow_driver") {
        result = { driver: { data: { mode: "agent", epoch: 7 } } };
      } else return route.fallback();
      await route.fulfill({ json: { jsonrpc: "2.0", id: call.id, result } });
    });
    try {
      await page.goto(url);
      const panel = page.getByRole("region", { name: "项目经理协作" });
      await expect(panel.getByText("协作中", { exact: true })).toBeVisible({
        timeout: 6500,
      });
      await expect(
        panel.getByText("actual manager is active", { exact: true }),
      ).toBeVisible();
      await expect(
        panel.getByRole("button", { name: "人工接管", exact: true }),
      ).toBeEnabled();
      await expect(panel).not.toContainText("当前调度：manual");
      await expect(panel).not.toContainText("发送指令后启动项目经理");
      await expect(
        panel.getByText("调度与经理管理", { exact: true }),
      ).toHaveCount(0);
      await expect(
        panel.getByText("配置选项暂不可用", { exact: false }),
      ).toHaveCount(0);
    } finally {
      release();
    }
  });
}

test("流程、材料、步骤上下文、Agent 讨论和审计定位同一执行与轮次", async ({
  page,
}, info) => {
  const url = await fixture(page, undefined, true);
  const pageErrors: string[] = [];
  page.on("pageerror", (error) => pageErrors.push(error.message));
  const execution = {
    id: "11111111-2222-4333-8444-555555555555",
    step: "requirements",
    node_id: "77777777-8888-4999-8aaa-bbbbbbbbbbbb",
    round: 1,
    status: "completed",
    reason: "需求已整理",
    session_id: "22222222-3333-4444-8555-666666666666",
    turn_id: "33333333-4444-4555-8666-777777777777",
    plan_revision_id: "55555555-6666-4777-8888-999999999999",
    inputs: {},
    outputs: { report: "44444444-5555-4666-8777-888888888888" },
  };
  const record = {
    ...execution,
    execution_id: execution.id,
    instance_id: "read-instance",
    instance_name: "需求流程",
    owner_id: "read-feature",
    owner_kind: "feature",
    feature_id: "read-feature",
    owner_title: "读取隔离 Feature",
    step_name: "澄清需求",
    actor: {
      name: "需求分析师",
      agent_instance_id: "66666666-7777-4888-8999-aaaaaaaaaaaa",
    },
    historical: false,
    outputs: [
      {
        id: "44444444-5555-4666-8777-888888888888",
        name: "需求报告",
        port: "report",
        turn_id: "33333333-4444-4555-8666-777777777777",
      },
    ],
  };
  const pendingExecution = {
    ...execution,
    id: "future-execution",
    step: "future",
    node_id: "future-node",
    status: "queued",
    reason: "",
    session_id: null,
    turn_id: null,
    outputs: {},
  };
  const pendingRecord = {
    ...record,
    ...pendingExecution,
    execution_id: pendingExecution.id,
    step_name: "尚未产生的步骤",
    outputs: [],
    context_available: false,
    execution_available: false,
  };
  const plan = {
    stages: [{ key: "requirements", name: "需求阶段" }],
    steps: [
      {
        key: "requirements",
        node_id: "77777777-8888-4999-8aaa-bbbbbbbbbbbb",
        name: "澄清需求",
        stage: "requirements",
        executor: { type: "agent" },
        outputs: {},
      },
      {
        key: "future",
        node_id: "future-node",
        name: "尚未产生的步骤",
        stage: "requirements",
        executor: { type: "control" },
      },
    ],
  };
  const worker = {
    id: "worker:1",
    type: "agent",
    definition_id: "worker",
    name: "需求分析师",
    version: 1,
    capability_ref: "capability:1",
    data: {},
  };
  const binding = {
    installation: { key: { provider: "codex" } },
    model: "fixture",
    agent_configuration: {
      agent_instance_id: "66666666-7777-4888-8999-aaaaaaaaaaaa",
    },
  };
  const session = {
    id: "22222222-3333-4444-8555-666666666666",
    project_id: "read-project",
    feature_id: "read-feature",
    owner_id: "read-feature",
    agent: worker.id,
    runtime_binding: binding,
    lifecycle: "idle",
  };
  await page.route("**/rpc", async (route) => {
    const call = route.request().postDataJSON();
    let result: any;
    switch (call.method) {
      case "query.workflow_executions":
        result = call.params.execution_id
          ? {
              record,
              execution,
              facts: [
                {
                  id: "decision",
                  type: "workflow_confirmation",
                  name: "人类确认回执",
                  created_at: 1,
                  data: { actor: "human", reason: "核对完成" },
                },
              ],
            }
          : {
              records: [
                record,
                pendingRecord,
                {
                  ...record,
                  id: "previous-execution",
                  execution_id: "previous-execution",
                  step_name: "首轮需求",
                  historical: true,
                  outputs: [],
                },
              ],
            };
        break;
      case "query.workflow_progress":
        result = {
          record: {
            id: "read-instance",
            data: { owner_id: "read-feature", status: "running" },
          },
          counts: { total: 2, completed: 1, running: 0, waiting: 1, failed: 0 },
          stages: [],
          next_stage_offset: null,
          observed_only: true,
        };
        break;
      case "query.workflow_instance": {
        const runtime = {
          owner_id: "read-feature",
          status: "running",
          steps: [execution, pendingExecution],
          plan: { document: plan },
        };
        result = {
          record: { id: "read-instance", name: "需求流程", data: runtime },
          runtime,
          plan,
          children: [],
          history: [],
          actors: { [execution.id]: record.actor },
        };
        break;
      }
      case "query.workflow_materials":
        result = call.params.output_id
          ? {
              record: { ...record, delivery_name: "需求报告", port: "report" },
              output: {
                delivery_ref: "report:1",
                value: "这份材料来自本轮需求分析",
              },
              definitions: {},
            }
          : {
              records: [
                {
                  ...record,
                  id: "44444444-5555-4666-8777-888888888888",
                  delivery_name: "需求报告",
                },
              ],
              total: 1,
            };
        break;
      case "query.workflow_contexts":
        result = {
          executions: [record, pendingRecord],
          agents: [
            {
              id: "66666666-7777-4888-8999-aaaaaaaaaaaa",
              actor: record.actor,
              entries: [{ ...record, task_available: true, kind: "step" }],
            },
          ],
        };
        break;
      case "query.execution_task":
        expect(call.params.turn_id).toBe(
          "33333333-4444-4555-8666-777777777777",
        );
        result = {
          configured: true,
          turn_id: "33333333-4444-4555-8666-777777777777",
          task: { task_instructions: "这是此轮实际冻结的需求输入" },
        };
        break;
      case "query.configuration_index":
        result = {
          records: [
            worker,
            {
              id: "66666666-7777-4888-8999-aaaaaaaaaaaa",
              type: "agent_instance",
              name: "需求分析师实例",
              data: { agent_revision_id: worker.id },
            },
          ],
          total: 2,
        };
        break;
      case "query.sessions":
        result = { sessions: [session] };
        break;
      case "query.session":
        expect(call.params.id).toBe(session.id);
        result = {
          session,
          workflow_context: {
            instance_id: "read-instance",
            step_instance_id: execution.id,
          },
          formal_execution: true,
          turns: [
            { id: "33333333-4444-4555-8666-777777777777", status: "completed" },
            { id: "other-turn", status: "completed" },
          ],
          events: [
            {
              id: "response",
              turn_id: "33333333-4444-4555-8666-777777777777",
              kind: "assistant",
              content: "需求分析师的本轮回答",
              sequence: 1,
            },
            {
              id: "other",
              turn_id: "other-turn",
              kind: "assistant",
              content: "另一个轮次的回答",
              sequence: 2,
            },
          ],
          next_cursor: 2,
          interactions: [],
          proposals: [],
          executions: [],
        };
        break;
      case "query.agent_experience":
        result = { records: [], total: 0, unavailable: [] };
        break;
      default:
        return route.fallback();
    }
    await route.fulfill({ json: { jsonrpc: "2.0", id: call.id, result } });
  });
  await page.goto(
    url.replace("detail=manager", "detail=documents") +
      "&output_id=44444444-5555-4666-8777-888888888888",
  );
  const material = page.getByRole("article", { name: "选中材料正文" });
  await expect(material).toContainText("这份材料来自本轮需求分析");
  await expect(
    material.getByRole("link", { name: "查看流程步骤", exact: true }),
  ).toHaveAttribute(
    "href",
    /execution_id=11111111-2222-4333-8444-555555555555/,
  );
  await material
    .getByRole("link", { name: "查看执行者上下文", exact: true })
    .click();
  const context = page.getByRole("article", { name: "选中步骤上下文" });
  await expect(context).toContainText("这是此轮实际冻结的需求输入");
  await expect(
    page.getByLabel("上下文任务与轮次", { exact: true }),
  ).toHaveValue("33333333-4444-4555-8666-777777777777");
  await expect(
    page.getByRole("navigation", { name: "流程步骤上下文目录" }),
  ).not.toContainText("尚未产生的步骤");
  await expect(
    page.getByRole("navigation", { name: "Agent 实例目录" }),
  ).toHaveCount(0);
  await page.setViewportSize({ width: 390, height: 844 });
  expect(
    await page.evaluate(
      () => document.documentElement.scrollWidth <= innerWidth,
    ),
  ).toBe(true);
  await page.setViewportSize({ width: 1440, height: 1000 });
  await context
    .getByRole("link", { name: "查看此轮会话", exact: true })
    .click();
  await expect(page.getByLabel("讨论会话", { exact: true })).toHaveValue(
    "22222222-3333-4444-8555-666666666666",
  );
  await expect(page.getByLabel("讨论轮次", { exact: true })).toHaveValue(
    "33333333-4444-4555-8666-777777777777",
  );
  const log = page.getByRole("log", { name: "讨论记录" });
  await expect(log).toContainText("需求分析师的本轮回答");
  await expect(log).not.toContainText("另一个轮次的回答");
  expect(pageErrors).toEqual([]);
  await page
    .getByRole("region", { name: "会话与流程关联" })
    .getByRole("link", { name: "查看流程步骤", exact: true })
    .click();
  const step = page.getByRole("article", {
    name: "步骤 · 澄清需求",
    exact: true,
  });
  await expect(step).toHaveAttribute("aria-current", "step");
  const future = page.getByRole("article", {
    name: "步骤 · 尚未产生的步骤",
    exact: true,
  });
  await expect(future).toBeVisible();
  await expect(
    future.getByRole("link", { name: "查看执行者上下文", exact: true }),
  ).toHaveCount(0);
  await expect(
    step.getByRole("link", { name: "材料 · report", exact: true }),
  ).toBeVisible();
  for (const entrance of [
    step.getByRole("link", { name: "查看此轮会话", exact: true }),
    step.getByRole("button", { name: "打开 Agent 讨论", exact: true }),
  ]) {
    await entrance.click();
    await expect(page.getByLabel("讨论会话", { exact: true })).toHaveValue(
      execution.session_id,
    );
    await expect(page.getByLabel("讨论轮次", { exact: true })).toHaveValue(
      execution.turn_id,
    );
    await expect(log).toContainText("需求分析师的本轮回答");
    expect(pageErrors).toEqual([]);
    await page
      .getByRole("region", { name: "会话与流程关联" })
      .getByRole("link", { name: "查看流程步骤", exact: true })
      .click();
    await expect(step).toBeVisible();
  }
  await step.getByRole("link", { name: "查看执行记录", exact: true }).click();
  await expect(
    page.getByRole("checkbox", { name: "显示历史执行" }),
  ).toHaveCount(0);
  await expect(
    page
      .getByRole("navigation", { name: "执行轮次目录" })
      .getByRole("button", { name: /首轮需求/ }),
  ).toBeVisible();
  await expect(
    page.getByRole("article", { name: "选中执行记录" }),
  ).toContainText("人类确认回执");
  await expect(
    page.getByRole("navigation", { name: "执行轮次目录" }),
  ).not.toContainText("尚未产生的步骤");
  await expect(
    page.getByRole("article", { name: "选中步骤上下文" }),
  ).toHaveCount(0);
  await page.getByRole("tab", { name: "项目经理", exact: true }).click();
  await expect(
    page.getByRole("region", { name: "当前工作进度" }),
  ).toContainText("已完成 1 / 2 个步骤");
  await expect(page.getByRole("region", { name: "步骤上下文" })).toHaveCount(0);
  await page.setViewportSize({ width: 390, height: 844 });
  expect(
    await page.evaluate(
      () => document.documentElement.scrollWidth <= innerWidth,
    ),
  ).toBe(true);
  await page.screenshot({
    path: info.outputPath("execution-perspectives.png"),
    fullPage: true,
  });
});

test("上下文切换保留完整目录，经理编排轮次不读取步骤执行", async ({
  page,
}, info) => {
  const url = (await fixture(page)).replace("detail=manager", "detail=context");
  const common = {
    instance_id: "read-instance",
    instance_name: "当前开发流程",
    owner_id: "read-feature",
    owner_title: "当前 Feature",
    status: "completed",
    historical: false,
    round: 1,
  };
  const controller = {
    ...common,
    kind: "controller",
    instance_name: "编排控制",
    execution_id: "controller-record",
    step_name: "编排控制",
    session_id: "manager-session",
    turn_id: "manager-turn",
    task_available: false,
  };
  const requirements = {
    ...common,
    kind: "step",
    execution_id: "requirements-execution",
    step_name: "澄清需求",
    session_id: "requirements-session",
    turn_id: "requirements-turn",
    task_available: true,
  };
  const entries = [
    controller,
    { ...controller, turn_id: "manager-previous-turn", historical: true },
    requirements,
    {
      ...requirements,
      turn_id: "requirements-previous-turn",
      historical: true,
    },
    {
      ...requirements,
      execution_id: "design-execution",
      step_name: "确认设计",
      session_id: "design-session",
      turn_id: "design-turn",
    },
    {
      ...requirements,
      execution_id: "earlier-requirements-execution",
      step_name: "首轮需求",
      session_id: "earlier-requirements-session",
      turn_id: "earlier-requirements-turn",
      historical: true,
    },
  ];
  const executionReads: string[] = [];
  await page.route("**/rpc", async (route) => {
    const call = route.request().postDataJSON();
    let result;
    if (call.method === "query.workflow_contexts") {
      const selected = entries.filter(
        (entry) =>
          (!call.params.execution_id ||
            entry.execution_id === call.params.execution_id) &&
          (!call.params.turn_id || entry.turn_id === call.params.turn_id),
      );
      result = {
        agents: [
          { id: "contexts", actor: { name: "执行者" }, entries: selected },
        ],
      };
    } else if (call.method === "query.execution_task") {
      result = {
        task: { task_instructions: `冻结输入 · ${call.params.turn_id}` },
      };
    } else if (
      call.method === "query.workflow_executions" &&
      call.params.execution_id
    ) {
      executionReads.push(call.params.execution_id);
      return route.fulfill({
        json: {
          jsonrpc: "2.0",
          id: call.id,
          error: {
            code: -32000,
            message: "执行不属于当前工作范围",
            data: { code: "not_found" },
          },
        },
      });
    } else return route.fallback();
    await route.fulfill({ json: { jsonrpc: "2.0", id: call.id, result } });
  });
  await page.goto(url);
  const directory = page.getByRole("navigation", {
    name: "流程步骤上下文目录",
  });
  const detail = page.getByRole("article", { name: "选中步骤上下文" });
  await expect(
    page.getByRole("checkbox", { name: "显示历史执行" }),
  ).toHaveCount(0);
  await expect(
    directory.getByRole("button", { name: /首轮需求/ }),
  ).toBeVisible();
  await expect(directory.getByRole("button")).toHaveCount(4);
  await expect(detail).toContainText("项目经理编排轮次的请求与响应见关联会话");
  await expect(detail.getByRole("alert")).toHaveCount(0);
  await expect(
    detail.getByRole("link", { name: "查看流程步骤", exact: true }),
  ).toHaveCount(0);
  await expect(
    detail.getByRole("link", { name: "查看执行记录", exact: true }),
  ).toHaveCount(0);
  await expect(
    detail.getByRole("link", { name: "查看此轮会话", exact: true }),
  ).toBeVisible();
  for (const name of ["澄清需求", "确认设计", "编排控制"]) {
    const selectedRead = page.waitForResponse((response) => {
      if (!response.url().endsWith("/rpc")) return false;
      const call = response.request().postDataJSON();
      return call.method === "query.workflow_contexts" && !!call.params.turn_id;
    });
    await directory.getByRole("button", { name: new RegExp(name) }).click();
    await selectedRead;
    await page.evaluate(
      () =>
        new Promise((resolve) =>
          requestAnimationFrame(() => requestAnimationFrame(resolve)),
        ),
    );
    await expect(
      detail.getByRole("heading", { name, exact: true }),
    ).toBeVisible();
    await expect(directory.getByRole("button")).toHaveCount(4);
    await expect(detail.getByRole("alert")).toHaveCount(0);
  }
  await expect(
    page.getByLabel("上下文任务与轮次").locator("option"),
  ).toHaveCount(2);
  await page
    .getByLabel("上下文任务与轮次")
    .selectOption("manager-previous-turn");
  await expect(directory.getByRole("button")).toHaveCount(4);
  await expect(detail).toContainText("项目经理编排轮次的请求与响应见关联会话");
  expect(executionReads).toEqual([]);
  await page.screenshot({
    path: info.outputPath("context-directory.png"),
    fullPage: true,
  });
});

test("workflow approval keeps its original draft versions and command identity after progress changes", async ({
  page,
}) => {
  const url = (await fixture(page)).replace(
    "detail=manager",
    "detail=overview",
  );
  let revision = 5;
  const submitted: any[] = [];
  await page.route("**/rpc", async (route) => {
    const call = route.request().postDataJSON();
    if (call.method === "workflow.step.approve") {
      submitted.push(call.params);
      return route.fulfill({
        json: {
          jsonrpc: "2.0",
          id: call.id,
          error: {
            code: -32000,
            message: "controlled original-version conflict",
            data: { code: "revision_conflict" },
          },
        },
      });
    }
    if (call.method !== "query.workflow_instance") return route.fallback();
    const plan = {
      stages: [{ key: "work", name: "审阅阶段" }],
      steps: [
        {
          key: "review",
          stage: "work",
          name: "核对版本",
          executor: { type: "control" },
          confirmation: "human",
        },
      ],
    };
    const runtime = {
      owner_id: "read-feature",
      status: "running",
      paused: false,
      control_version: revision === 5 ? 2 : 8,
      active_plan_revision_id: revision === 5 ? "plan-original" : "plan-new",
      steps: [
        {
          id: "review-execution",
          step: "review",
          status: "waiting_confirmation",
          round: 1,
          input_digest: revision === 5 ? "input-original" : "input-new",
          outputs: {
            result: revision === 5 ? "output-original" : "output-new",
          },
        },
      ],
    };
    return route.fulfill({
      json: {
        jsonrpc: "2.0",
        id: call.id,
        result: {
          record: {
            id: "read-instance",
            name: `流程状态 ${revision}`,
            revision,
            data: runtime,
          },
          runtime,
          plan,
          completion_status: {
            "review-execution": { satisfied: false, human_satisfied: false },
          },
          children: [],
        },
      },
    });
  });
  await page.goto(url);
  const runtime = page.getByRole("region", { name: "当前工作流", exact: true });
  await expect(
    runtime.getByRole("heading", { name: "流程状态 5" }),
  ).toBeVisible();
  await runtime.locator(".workflow-step-toggle").click();
  const reason = runtime.getByLabel("手动批准理由（必填）");
  await reason.fill("基于最初产物的决定");
  revision = 9;
  await runtime.getByRole("button", { name: "刷新进度", exact: true }).click();
  await expect(
    runtime.getByRole("heading", { name: "流程状态 9" }),
  ).toBeVisible();
  const approve = runtime.getByRole("button", { name: "手动 Approve 此版本" });
  await approve.click();
  await expect(runtime.getByRole("alert")).toContainText("revision_conflict");
  await approve.click();
  await expect.poll(() => submitted.length).toBe(2);
  expect(submitted[0]).toMatchObject({
    expected_revision: 5,
    payload: {
      plan_revision_id: "plan-original",
      control_version: 2,
      execution_id: "review-execution",
      input_digest: "input-original",
      outputs: { result: "output-original" },
      reason: "基于最初产物的决定",
    },
  });
  expect(submitted[1]).toEqual(submitted[0]);
  await expect(reason).toHaveValue("基于最初产物的决定");
  await reason.fill("修改后的理由仍基于原版本");
  await approve.click();
  await expect.poll(() => submitted.length).toBe(3);
  expect(submitted[2].expected_revision).toBe(5);
  expect(submitted[2].command_id).not.toBe(submitted[0].command_id);
});

test("parallel supplemental inputs retain separate drafts and original control versions", async ({
  page,
}) => {
  const url = (await fixture(page)).replace(
    "detail=manager",
    "detail=overview",
  );
  let revision = 5;
  const submitted: any[] = [];
  await page.route("**/rpc", async (route) => {
    const call = route.request().postDataJSON();
    if (call.method === "workflow.step.input") {
      submitted.push(call.params);
      return route.fulfill({
        json: {
          jsonrpc: "2.0",
          id: call.id,
          error: {
            code: -32000,
            message: "controlled original-version conflict",
            data: { code: "revision_conflict" },
          },
        },
      });
    }
    if (call.method !== "query.workflow_instance") return route.fallback();
    const runtime = {
      owner_id: "read-feature",
      status: "running",
      paused: false,
      active_plan_revision_id: "plan-original",
      control_version: revision,
      steps: ["first", "second"].map((step) => ({
        id: `${step}-execution`,
        step,
        status: "waiting_input",
        round: 1,
        outputs: {},
      })),
    };
    return route.fulfill({
      json: {
        jsonrpc: "2.0",
        id: call.id,
        result: {
          record: {
            id: "read-instance",
            name: `输入状态 ${revision}`,
            revision,
          },
          runtime,
          plan: {
            stages: [{ key: "work", name: "并行输入" }],
            steps: ["first", "second"].map((key) => ({
              key,
              name: key,
              stage: "work",
              executor: { type: "control" },
            })),
          },
          children: [],
        },
      },
    });
  });
  await page.goto(url);
  const runtime = page.getByRole("region", { name: "当前工作流", exact: true });
  await expect(
    runtime.getByRole("heading", { name: "输入状态 5" }),
  ).toBeVisible();
  const first = runtime.getByRole("article", { name: "步骤 · first" });
  const second = runtime.getByRole("article", { name: "步骤 · second" });
  await first.locator(".workflow-step-toggle").click();
  await second.locator(".workflow-step-toggle").click();
  await first.getByLabel("补充本次任务").fill("第一步输入");
  await expect(second.getByLabel("补充本次任务")).toHaveValue("");
  await second.getByLabel("补充本次任务").fill("第二步输入");
  revision = 9;
  await runtime.getByRole("button", { name: "刷新进度", exact: true }).click();
  await expect(
    runtime.getByRole("heading", { name: "输入状态 9" }),
  ).toBeVisible();
  await first.getByRole("button", { name: "提交补充输入" }).click();
  await expect(runtime.getByRole("alert")).toContainText("revision_conflict");
  await first.getByRole("button", { name: "提交补充输入" }).click();
  await second.getByRole("button", { name: "提交补充输入" }).click();
  await expect.poll(() => submitted.length).toBe(3);
  expect(submitted[1]).toEqual(submitted[0]);
  expect(submitted[0]).toMatchObject({
    expected_revision: 5,
    payload: { step: "first", input: "第一步输入", control_version: 5 },
  });
  expect(submitted[2]).toMatchObject({
    expected_revision: 5,
    payload: { step: "second", input: "第二步输入", control_version: 5 },
  });
  await expect(first.getByLabel("补充本次任务")).toHaveValue("第一步输入");
  await expect(second.getByLabel("补充本次任务")).toHaveValue("第二步输入");
});

test("manager loads an exact plan only on expansion and retains its draft through summary refreshes", async ({
  page,
}) => {
  const url = await fixture(page);
  let revision = 1,
    fullReads = 0;
  const submissions: any[] = [];
  let release!: () => void;
  const held = new Promise<void>((resolve) => {
    release = resolve;
  });
  const record = () => ({
    id: "read-instance",
    name: "Summary-only manager",
    revision,
    data: {
      owner_id: "read-feature",
      status: "running",
      root_instance_id: "read-instance",
      active_plan_revision_id: "plan-current",
      control_version: 1,
      role_agent_bindings: {},
      plan: { standard_ref: "standard-current:1" },
    },
  });
  await page.route("**/rpc", async (route) => {
    const call = route.request().postDataJSON();
    if (call.method === "query.workflow_change_preview")
      return route.fulfill({
        json: {
          jsonrpc: "2.0",
          id: call.id,
          result: {
            changeset: call.params.changeset,
            preview_hash: "original-preview",
            affected: [],
            must_stop: [],
            bypassed: [],
          },
        },
      });
    if (call.method === "workflow.change.apply") {
      submissions.push(call.params);
      return route.fulfill({
        json: {
          jsonrpc: "2.0",
          id: call.id,
          error: {
            code: -32000,
            message: "controlled original plan conflict",
            data: { code: "revision_conflict" },
          },
        },
      });
    }
    if (call.method === "query.workflow_management_summary")
      return route.fulfill({
        json: {
          jsonrpc: "2.0",
          id: call.id,
          result: { record: record(), children: [], observed_only: true },
        },
      });
    if (call.method !== "query.workflow_instance") return route.fallback();
    fullReads++;
    expect(call.params).toEqual({
      project_id: "read-project",
      id: "read-instance",
    });
    await held;
    const exact = record();
    await route.fulfill({
      json: {
        jsonrpc: "2.0",
        id: call.id,
        result: {
          record: {
            ...exact,
            data: {
              ...exact.data,
              plan: {
                ...exact.data.plan,
                document: {
                  stages: [{ key: "work", name: "审阅" }],
                  steps: [
                    {
                      key: "review",
                      node_id: "review-node",
                      name: "审阅步骤",
                      stage: "work",
                      executor: { type: "control" },
                    },
                  ],
                },
              },
            },
          },
        },
      },
    });
  });
  try {
    await page.goto(url);
    const manager = page.getByRole("region", { name: "项目经理", exact: true });
    await expect(
      manager.getByRole("heading", {
        name: "主工作流 · Summary-only manager",
        exact: true,
      }),
    ).toBeVisible();
    expect(fullReads).toBe(0);
    await manager.getByText("调整当前运行计划", { exact: true }).click();
    await expect(
      manager.getByRole("status").filter({ hasText: "正在读取确切执行计划" }),
    ).toBeVisible();
    expect(fullReads).toBe(1);
    release();
    const reason = manager.getByLabel("计划调整理由", { exact: true });
    await reason.fill("保留原计划基线和未保存理由");
    revision = 2;
    await manager
      .getByRole("button", { name: "刷新管理状态", exact: true })
      .click();
    await expect(reason).toHaveValue("保留原计划基线和未保存理由");
    expect(fullReads).toBe(1);
    await manager
      .getByLabel("计划调整对象", { exact: true })
      .selectOption("review");
    await manager
      .getByRole("button", { name: "预览影响", exact: true })
      .click();
    const apply = manager.getByRole("button", {
      name: "应用此计划调整",
      exact: true,
    });
    await apply.click();
    await expect(manager.getByRole("alert")).toContainText("revision_conflict");
    await apply.click();
    await expect.poll(() => submissions.length).toBe(2);
    expect(submissions[0].expected_revision).toBe(1);
    expect(submissions[1]).toEqual(submissions[0]);
    expect(fullReads).toBe(1);
    await expect(reason).toHaveValue("保留原计划基线和未保存理由");
    await manager.getByText("调整当前运行计划", { exact: true }).click();
    await manager.getByText("调整当前运行计划", { exact: true }).click();
    await expect(reason).toHaveValue("保留原计划基线和未保存理由");
    expect(fullReads).toBe(1);
  } finally {
    release();
  }
});

test("folded work configuration is lazy and Standard apply keeps its original revision", async ({
  page,
}) => {
  const url = await fixture(page);
  let workReads = 0;
  let fullReads = 0;
  let revision = 1;
  const submissions: any[] = [];
  await page.route("**/rpc", async (route) => {
    const call = route.request().postDataJSON();
    let result: any;
    if (call.method === "query.work_configuration") {
      workReads++;
      return route.fallback();
    } else if (call.method === "query.workflow_instance") {
      fullReads++;
      return route.fallback();
    } else if (call.method === "query.workflow_management_summary") {
      result = {
        record: {
          id: "read-instance",
          name: "读取隔离流程",
          revision,
          data: {
            owner_id: "read-feature",
            status: "running",
            active_plan_revision_id: "plan-current",
            workflow_ref: "workflow-current:1",
            role_agent_bindings: {},
            plan: { standard_ref: "standard-current:1" },
          },
        },
      };
    } else if (call.method === "query.configuration_index") {
      result = {
        records: [
          {
            id: "standard-current:1",
            type: "standard",
            name: "Current Standard",
            data: { default_workflow_ref: "workflow-current:1" },
            workflow_refs: ["workflow-current:1"],
          },
        ],
        total: 1,
      };
    } else if (call.method === "query.workflow_change_preview") {
      result = {
        changeset: call.params.changeset,
        preview_hash: "original-preview",
        affected: [],
        standard_diff: { equal: true, changes: [] },
      };
    } else if (call.method === "workflow.change.apply") {
      submissions.push(call.params);
      return route.fulfill({
        json: {
          jsonrpc: "2.0",
          id: call.id,
          error: {
            code: -32000,
            message: "original revision has changed",
            data: { code: "revision_conflict" },
          },
        },
      });
    } else return route.fallback();
    await route.fulfill({ json: { jsonrpc: "2.0", id: call.id, result } });
  });
  await page.goto(url);
  const manager = page.getByRole("region", { name: "项目经理", exact: true });
  await expect(
    manager.getByRole("button", { name: "刷新管理状态" }),
  ).toBeVisible();
  expect(workReads).toBe(0);
  expect(fullReads).toBe(0);
  const standard = manager.locator(".work-standard").first();
  await standard
    .getByRole("button", { name: "采用的规范", exact: true })
    .click();
  await expect.poll(() => workReads).toBe(1);
  await expect(
    standard.getByLabel("采用的入口 Workflow", { exact: true }),
  ).toHaveValue("workflow-current:1");
  await standard
    .getByLabel("规范切换原因", { exact: true })
    .fill("keep original baseline");
  revision = 2;
  await manager
    .getByRole("button", { name: "刷新管理状态", exact: true })
    .click();
  await standard
    .getByRole("button", { name: "采用的规范", exact: true })
    .click();
  await standard
    .getByRole("button", { name: "采用的规范", exact: true })
    .click();
  await expect(
    standard.getByLabel("规范切换原因", { exact: true }),
  ).toHaveValue("keep original baseline");
  await standard
    .getByRole("button", { name: "预演规范切换", exact: true })
    .click();
  await standard
    .getByRole("button", { name: "应用规范切换", exact: true })
    .click();
  await expect(standard.getByRole("alert")).toContainText("revision_conflict");
  await standard
    .getByRole("button", { name: "应用规范切换", exact: true })
    .click();
  await expect.poll(() => submissions.length).toBe(2);
  expect(submissions[0].expected_revision).toBe(1);
  expect(submissions[1]).toEqual(submissions[0]);
  expect(fullReads).toBe(0);
  expect(workReads).toBe(1);
  await expect(
    standard.getByLabel("规范切换原因", { exact: true }),
  ).toHaveValue("keep original baseline");
});

test("阶段卡片跨页加载并展开对应步骤后滚动定位", async ({ page }) => {
  const url = (await fixture(page)).replace(
    "detail=manager",
    "detail=overview",
  );
  const stages = [
    { key: "first", name: "前置阶段" },
    { key: "last", name: "交付阶段" },
  ];
  const definitions = Array.from({ length: 21 }, (_, index) => ({
    key: `step-${index}`,
    name: index === 20 ? "交付验收步骤" : `准备步骤 ${index + 1}`,
    stage: index === 20 ? "last" : "first",
    executor: { type: "control" },
  }));
  const statusReads: any[] = [];
  await page.route("**/rpc", async (route) => {
    const call = route.request().postDataJSON();
    let result;
    if (call.method === "query.workflow_executions") {
      result = { records: [] };
    } else if (call.method === "query.workflow_progress") {
      const counts = (total: number) => ({
        total,
        completed: 0,
        running: 0,
        waiting: 0,
        failed: 0,
      });
      result = {
        project_id: "read-project",
        record: { id: "read-instance" },
        counts: counts(21),
        next_stage_offset: null,
        stages: stages.map((stage, index) => ({
          ...stage,
          counts: counts(index ? 1 : 20),
        })),
      };
    } else if (call.method === "query.workflow_instance") {
      statusReads.push(call.params);
      const offset = call.params.step_offset || 0;
      const steps = definitions.slice(offset, offset + 20);
      const plan = { stages, steps };
      const runtime = {
        owner_id: "read-feature",
        status: "running",
        plan: { document: plan },
        steps: steps.map((step) => ({
          id: `execution-${step.key}`,
          step: step.key,
          round: 1,
          status: "queued",
          outputs: {},
        })),
      };
      result = {
        record: {
          id: "read-instance",
          name: "分页流程",
          revision: 1,
          data: runtime,
        },
        runtime,
        plan,
        children: [],
        step_total: 21,
        step_offset: offset,
        next_step_offset: offset === 0 ? 20 : null,
      };
    } else return route.fallback();
    await route.fulfill({ json: { jsonrpc: "2.0", id: call.id, result } });
  });
  await page.goto(url);
  const runtime = page.getByRole("region", { name: "当前工作流", exact: true });
  const progress = runtime.getByRole("list", { name: "执行阶段进度" });
  const lastStage = runtime
    .locator(".workflow-stage-toggle")
    .filter({ hasText: "交付阶段" });
  await lastStage.click();
  await expect(lastStage).toHaveAttribute("aria-expanded", "false");
  await progress
    .getByRole("button", { name: "跳转到阶段：交付阶段", exact: true })
    .click();
  const delivery = runtime.getByRole("article", {
    name: "步骤 · 交付验收步骤",
    exact: true,
  });
  await expect(delivery).toBeInViewport();
  await expect(lastStage).toHaveAttribute("aria-expanded", "true");
  await expect(lastStage).toBeFocused();
  expect(statusReads.some((read) => read.step_offset === 20)).toBe(true);
  expect(statusReads.every((read) => read.view === "status")).toBe(true);
  await progress
    .getByRole("button", { name: "跳转到阶段：前置阶段", exact: true })
    .click();
  const firstStage = runtime
    .locator(".workflow-stage-toggle")
    .filter({ hasText: "前置阶段" });
  await expect(firstStage).toBeFocused();
  await expect(firstStage).toBeInViewport();
  await expect(
    runtime.getByRole("article", { name: "步骤 · 准备步骤 1", exact: true }),
  ).toBeInViewport();
  await expect(delivery).toHaveCount(0);
});

test("workflow overview opens stage progress and bounded step actions together", async ({
  page,
}, info) => {
  const url = (await fixture(page)).replace(
    "detail=manager",
    "detail=overview",
  );
  await page.goto("about:blank");
  const calls: { method: string; params: any }[] = [];
  await page.route("**/rpc", async (route) => {
    const call = route.request().postDataJSON();
    calls.push(call);
    if (call.method === "query.workflow_executions") {
      return route.fulfill({
        json: { jsonrpc: "2.0", id: call.id, result: { records: [] } },
      });
    }
    if (call.method === "query.workflow_progress") {
      const counts = (total: number, completed: number) => ({
        total,
        completed,
        running: 0,
        waiting: 0,
        failed: 0,
      });
      return route.fulfill({
        json: {
          jsonrpc: "2.0",
          id: call.id,
          result: {
            project_id: "read-project",
            record: { id: "read-instance" },
            counts: counts(22, 8),
            next_stage_offset: null,
            stages: [
              ["需求", 2, 2],
              ["设计", 4, 4],
              ["任务规划", 3, 2],
              ["实施", 1, 0],
              ["整体验证", 4, 0],
              ["交付整理", 2, 0],
              ["接纳发布", 6, 0],
            ].map(([name, total, completed], index) => ({
              key: `stage-${index}`,
              name,
              counts: counts(Number(total), Number(completed)),
            })),
          },
        },
      });
    }
    await route.fallback();
  });
  await page.goto(url);
  await expect(
    page.getByRole("region", { name: "当前工作流", exact: true }),
  ).toBeVisible();
  await expect(
    page.getByRole("region", { name: "工作流进度摘要" }),
  ).toHaveCount(0);
  await expect(
    page.getByRole("region", { name: "阶段进度", exact: true }),
  ).toContainText("2 / 2 已完成");
  await expect(
    page.getByRole("button", { name: /(?:展开|收起)步骤与操作/ }),
  ).toHaveCount(0);
  const methods = calls.map((call) => call.method);
  expect(methods).toContain("query.execution_workspace");
  expect(methods).not.toContain("query.workspace");
  expect(methods).toContain("query.workflow_progress");
  const statusReads = calls.filter(
    (call) => call.method === "query.workflow_instance",
  );
  expect(statusReads.length).toBeGreaterThan(0);
  for (const call of statusReads) {
    expect(call.params).toMatchObject({
      view: "status",
      child_offset: 0,
      step_offset: 0,
    });
  }
  const progress = page.getByRole("list", { name: "执行阶段进度" });
  await expect(progress.getByRole("listitem")).toHaveCount(7);
  await expect(progress.locator(".workflow-stage-arrow")).toHaveCount(6);
  await expect(page.getByRole("alert")).toHaveCount(0);
  const tops = await progress
    .getByRole("listitem")
    .evaluateAll((items) =>
      items.map((item) => item.getBoundingClientRect().top),
    );
  expect(new Set(tops).size).toBe(1);
  await page.screenshot({
    path: info.outputPath("stage-progress-desktop.png"),
    fullPage: true,
  });
  await page.setViewportSize({ width: 390, height: 844 });
  expect(
    await page.evaluate(
      () => document.documentElement.scrollWidth <= innerWidth,
    ),
  ).toBe(true);
  expect(
    await progress.evaluate(
      (element) => element.scrollWidth > element.clientWidth,
    ),
  ).toBe(true);
  await page.screenshot({
    path: info.outputPath("stage-progress-mobile.png"),
    fullPage: true,
  });
});

test("manual background bodies load only for the selected disclosure", async ({
  page,
}) => {
  const contexts = Array.from({ length: 25 }, (_, index) => ({
    id: `background-${index}`,
    owner_id: "read-feature",
    revision: 1,
    metadata_only: true,
  }));
  const url = await fixture(page, undefined, false, contexts);
  const bodies: string[] = [];
  await page.route("**/rpc", async (route) => {
    const call = route.request().postDataJSON();
    if (call.method !== "query.workflow_contexts") return route.fallback();
    if (call.params.context_id) bodies.push(call.params.context_id);
    await route.fulfill({
      json: {
        jsonrpc: "2.0",
        id: call.id,
        result: call.params.context_id
          ? {
              context: {
                ...contexts[3],
                content: "Only the selected original background",
              },
            }
          : { agents: [], executions: [] },
      },
    });
  });
  await page.goto(url.replace("detail=manager", "detail=context"));
  await page
    .getByText("共享工作背景 · 手工维护（不代表当轮已发送）", { exact: true })
    .click();
  await expect(page.locator("[data-context-id]")).toHaveCount(25);
  expect(bodies).toEqual([]);
  const selected = page.locator('[data-context-id="background-3"]');
  await selected.locator("summary").click();
  await expect(selected).toContainText("Only the selected original background");
  expect(bodies).toEqual(["background-3"]);
  await expect(
    selected.getByRole("button", { name: "编辑上下文", exact: true }),
  ).toBeVisible();
  await expect(selected.locator("a.raw-json-link")).toHaveAttribute(
    "href",
    /context_id=background-3/,
  );
});

test("经理待接纳产物与真正的人工确认分别展示", async ({ page }) => {
  const url = await fixture(page);
  let pendingHuman = false;
  await page.route("**/rpc", async (route) => {
    const call = route.request().postDataJSON();
    let result: any;
    if (call.method === "query.orchestration") {
      result = {
        record: {
          id: "manager",
          revision: 1,
          data: { status: "active", owner_id: "read-feature", epoch: 1 },
        },
        decisions: pendingHuman
          ? [
              {
                id: "exact-decision",
                revision: 1,
                current: true,
                data: {
                  status: "pending",
                  request: {
                    instance_id: "read-instance",
                    operation: "workflow.step.confirm",
                    arguments: { execution_id: "implementation-2" },
                    interaction: {
                      title: "请确认本轮实施产物",
                      requested_action: "确认或拒绝本轮产物",
                      materials: [],
                      evidence: [],
                    },
                  },
                },
              },
            ]
          : [],
        interactions: [],
        messages: [],
        calls: [],
      };
    } else if (call.method === "query.workflow_driver") {
      result = {
        driver: { id: "driver", data: { mode: "agent" } },
        waiting: [
          {
            instance_id: "read-instance",
            execution_id: "implementation-2",
            path: "/implementation/0/implementation",
            status: "awaiting_acceptance",
            actions: {},
          },
        ],
      };
    } else return route.fallback();
    await route.fulfill({ json: { jsonrpc: "2.0", id: call.id, result } });
  });
  await page.goto(url);
  await expect(
    page.getByText(
      "产物已完成，等待项目经理研判并接纳；当前没有需要你确认的事项。",
      { exact: true },
    ),
  ).toBeVisible();
  await expect(page.getByText(/待经理研判/)).toBeVisible();
  await expect(
    page.getByText("请确认本轮实施产物", { exact: true }),
  ).toHaveCount(0);
  pendingHuman = true;
  await page.reload();
  await expect(
    page.getByText("请确认本轮实施产物", { exact: true }),
  ).toBeVisible();
  await expect(
    page.getByText(
      "产物已完成，等待项目经理研判并接纳；当前没有需要你确认的事项。",
      { exact: true },
    ),
  ).toHaveCount(0);
});

async function managerReviewLinks(page: Page, review: string) {
  const url = await fixture(page);
  await page.route("**/rpc", async (route) => {
    const call = route.request().postDataJSON();
    if (call.method !== "query.orchestration") return route.fallback();
    await route.fulfill({
      json: {
        jsonrpc: "2.0",
        id: call.id,
        result: {
          record: {
            id: "link-manager",
            revision: 1,
            data: { status: "active", owner_id: "read-feature", epoch: 1 },
          },
          decisions: [
            {
              id: "link-decision",
              revision: 1,
              current: true,
              data: {
                status: "pending",
                request: {
                  instance_id: "read-instance",
                  operation: "workflow.step.confirm",
                  arguments: { execution_id: "publication" },
                  interaction: {
                    title: "确认 PR 与确切版本",
                    requested_action: "请查看 PR 和版本后确认",
                    review_summary: review,
                    materials: [],
                    evidence: [],
                  },
                },
              },
            },
          ],
          interactions: [],
          messages: [],
          calls: [],
        },
      },
    });
  });
  await page.goto(url);
  const card = page.getByRole("article", {
    name: "确认 PR 与确切版本",
    exact: true,
  });
  await expect(card).toBeVisible();
  return card;
}

test("经理 PR 摘要仅链接 PR 和三个提交，中文说明保持正文且点击到达确切地址", async ({
  page,
}) => {
  const repository = "https://github.com/xfork24/devdesk-project-2";
  const pr = `${repository}/pull/2`;
  const revisions = [
    "8d2d450bd71b75180697bef09d5a8d6a15fb2e07",
    "f1eaa0d11add3772c9c31ba25beb2a98b1677a12",
    "5b9989797e96c1280335d9516a5b038169cebd39",
  ];
  const review = `PR ${pr}；源版本${revisions[0]}，目标main基线${revisions[1]}，待合入候选${revisions[2]}。改动为欢迎页、独立测试及验证材料，评审未发现阻塞问题。`;
  const card = await managerReviewLinks(page, review);
  const summary = card
    .locator(".markdown-preview")
    .filter({ hasText: "改动为欢迎页" });
  await expect(summary).toHaveText(review);
  await expect(summary.getByRole("link")).toHaveCount(4);
  const destinations = [
    pr,
    ...revisions.map((sha) => `${repository}/commit/${sha}`),
  ];
  for (const [index, href] of destinations.entries())
    await expect(summary.getByRole("link").nth(index)).toHaveAttribute(
      "href",
      href,
    );
  await expect(
    summary.getByRole("link").filter({ hasText: "改动为" }),
  ).toHaveCount(0);
  await page.context().route("https://github.com/**", (route) =>
    route.fulfill({
      contentType: "text/html",
      body: "<p>GitHub link destination</p>",
    }),
  );
  for (const [index, href] of destinations.entries()) {
    await summary.getByRole("link").nth(index).click();
    await expect(page).toHaveURL(href);
    await page.goBack();
    await expect(summary).toHaveText(review);
  }
});

test("经理版本链接保留显式中文地址、行内代码和代码块", async ({ page }) => {
  const repository = "https://github.com/owner/repository";
  const source = "a".repeat(40);
  const target = "b".repeat(40);
  const codeCommit = "c".repeat(40);
  const chineseUrl = "https://example.com/中文/路径?说明=版本，基线";
  const codeUrl = `${repository}/pull/2；这是代码`;
  const review =
    `PR [查看 PR](${repository}/pull/2)；源版本 \`${source}\`，目标基线${target}。\n\n[中文地址](${chineseUrl})\n\n\`${codeUrl}\`\n\n\`\`\`text\n${codeCommit}\n${codeUrl}\n\`\`\`\n\n完整摘要 ${"d".repeat(64)}`.replaceAll(
      "\\n",
      "\n",
    );
  const card = await managerReviewLinks(page, review);
  await expect(
    card.getByRole("link", { name: "查看 PR", exact: true }),
  ).toHaveAttribute("href", `${repository}/pull/2`);
  await expect(
    card.getByRole("link", { name: source, exact: true }),
  ).toHaveAttribute("href", `${repository}/commit/${source}`);
  await expect(
    card.getByRole("link", { name: target, exact: true }),
  ).toHaveAttribute("href", `${repository}/commit/${target}`);
  expect(
    decodeURI(
      (await card
        .getByRole("link", { name: "中文地址", exact: true })
        .getAttribute("href"))!,
    ),
  ).toBe(chineseUrl);
  await expect(card.locator("pre code")).toHaveText(
    `${codeCommit}\n${codeUrl}\n`,
  );
  await expect(card.locator("pre a")).toHaveCount(0);
  await expect(
    card.getByRole("link", { name: codeUrl, exact: true }),
  ).toHaveCount(0);
  await expect(
    card.getByRole("link", { name: "d".repeat(64), exact: true }),
  ).toHaveCount(0);
});

test("经理摘要包含多个仓库时保留各 PR 地址并避免猜测提交所属仓库", async ({
  page,
}) => {
  const sha = "a".repeat(40);
  const card = await managerReviewLinks(
    page,
    `PR https://github.com/owner/first/pull/1；参照 https://github.com/owner/second/pull/2；源版本${sha}。`,
  );
  await expect(card.getByRole("link")).toHaveCount(2);
  await expect(card.getByRole("link").nth(0)).toHaveAttribute(
    "href",
    "https://github.com/owner/first/pull/1",
  );
  await expect(card.getByRole("link").nth(1)).toHaveAttribute(
    "href",
    "https://github.com/owner/second/pull/2",
  );
  await expect(card.getByRole("link", { name: sha, exact: true })).toHaveCount(
    0,
  );
});

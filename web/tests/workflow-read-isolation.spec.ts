import { test, expect, type Page } from "@playwright/test";
import { mkdtempSync } from "node:fs";
import { tmpdir } from "node:os";
import { join } from "node:path";

// Controlled UI responses; actual execution and publication are verified separately.
async function fixture(page: Page, failure?: "catalog" | "defaults") {
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
          configurations: [instance],
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
        result = { record: null };
        break;
      case "query.workflow_driver":
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
    if (call.method === "query.workspace") {
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

test("manager collects child confirmations and tool approvals, with errors beside the relevant card", async ({
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
    if (call.method === "query.workflow_instance") {
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
        messages: [{ id: "manager-message", content: "已同步当前流程进度" }],
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
  ).toHaveCount(0);
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
  const card = childPanel.getByRole("article", {
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
  const tool = childPanel.getByRole("article", {
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

test("manager loads an exact plan only on expansion and retains its draft through summary refreshes", async ({
  page,
}) => {
  const url = await fixture(page);
  let revision = 1,
    fullReads = 0;
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
              plan: { ...exact.data.plan, document: { stages: [], steps: [] } },
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
        name: "Summary-only manager",
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
    await manager.getByText("调整当前运行计划", { exact: true }).click();
    await manager.getByText("调整当前运行计划", { exact: true }).click();
    await expect(reason).toHaveValue("保留原计划基线和未保存理由");
    expect(fullReads).toBe(1);
  } finally {
    release();
  }
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
  await expect(manager).toContainText("暂时无法刷新经理状态");
  failRead = false;
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
  test(`Standard history remains visible when ${failure} loading fails`, async ({
    page,
  }) => {
    const url = await fixture(page, failure);
    await page.goto(url);
    const standard = page
      .getByRole("region", { name: "流程规范", exact: true })
      .locator(".work-standard");
    await standard.locator(":scope > summary").click();
    await standard.getByText("规范采用历史（2）", { exact: true }).click();
    await expect(
      standard.getByText("standard-old:1 · 采用记录 1", { exact: false }),
    ).toBeVisible();
    await expect(standard.getByRole("alert")).toContainText(
      "controlled read failure",
    );
    await expect(
      standard.getByText("standard-old:1 · 采用记录 1", { exact: false }),
    ).toBeVisible();
  });
}

test("a late defaults read preserves an already displayed Standard comparison", async ({
  page,
}) => {
  const url = await fixture(page);
  let releaseDefaults!: () => void;
  const defaults = new Promise<void>((resolve) => {
    releaseDefaults = resolve;
  });
  const selection = {
    standard_ref: "standard-current:1",
    children: [],
    exceptions: [],
  };
  const diff = {
    equal: false,
    changes: [{ kind: "definition_changed", source_kind: "workflow" }],
  };
  await page.route("**/rpc", async (route) => {
    const call = route.request().postDataJSON();
    let result: any;
    if (call.method === "query.project_configuration") {
      await defaults;
      result = { configuration: null };
    } else if (call.method === "query.work_standard_choices") {
      result = {
        records: [
          {
            id: "other-work",
            project_title: "Fixture",
            title: "Other work",
            work: { kind: "task" },
          },
        ],
        total: 1,
      };
    } else if (call.method === "query.work_standard_diff") {
      result = { entry_diff: diff, diff, before: selection, after: selection };
    } else return route.fallback();
    await route.fulfill({ json: { jsonrpc: "2.0", id: call.id, result } });
  });
  await page.goto(url);
  const standard = page
    .getByRole("region", { name: "流程规范", exact: true })
    .locator(".work-standard");
  await standard.locator(":scope > summary").click();
  await standard.getByText("与其他工作比较规范", { exact: true }).click();
  await standard
    .getByLabel("比较工作", { exact: true })
    .selectOption("other-work");
  await standard
    .getByRole("button", { name: "比较工作规范", exact: true })
    .click();
  await expect(standard.locator(".standard-diff").first()).toContainText(
    "1 项规范变化",
  );
  releaseDefaults();
  // This selection is set by the late read's callback, so the assertion runs after it.
  await expect(
    standard.getByLabel("采用 Standard", { exact: true }),
  ).toHaveValue("standard-current:1");
  await expect(standard.locator(".standard-diff").first()).toContainText(
    "1 项规范变化",
  );
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
  const url = await fixture(page);
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
          : { records: [record, pendingRecord] };
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
  await step.getByRole("link", { name: "查看执行记录", exact: true }).click();
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

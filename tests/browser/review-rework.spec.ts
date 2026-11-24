import { expect, type Page } from "@playwright/test";
import { test } from "./support/kernel";

async function rpc(page: Page, method: string, params: object = {}) {
  const response = await page.request.post("/rpc", {
    data: { jsonrpc: "2.0", id: 1, method, params },
  });
  const result = await response.json();
  expect(result.error, JSON.stringify(result)).toBeUndefined();
  return result.result;
}

test("经理阻塞链接定位分页外的真实步骤，刷新及前进后退仍可聚焦", async ({
  page,
}) => {
  await page.goto("/");
  const project = await rpc(page, "project.create", {
    command_id: crypto.randomUUID(),
    payload: { title: "返工阻塞定位" },
  });
  const draft = await rpc(page, "workflow.create", {
    command_id: crypto.randomUUID(),
    payload: {
      name: "分页阻塞流程",
      document: {
        schema_version: 6,
        stages: [{ key: "work", name: "确认材料" }],
        steps: Array.from({ length: 35 }, (_, index) => ({
          key: `gate${String(index).padStart(2, "0")}`,
          name: `材料 ${index}`,
          stage: "work",
          executor: { type: "control" },
          confirmation: "human",
        })),
      },
    },
  });
  const workflow = await rpc(page, "workflow.publish", {
    command_id: crypto.randomUUID(),
    target: draft.id,
    expected_revision: draft.revision,
    payload: {},
  });
  const root = await rpc(page, "workflow.start", {
    project_id: project.id,
    command_id: crypto.randomUUID(),
    target: project.id,
    expected_revision: (
      await rpc(page, "query.workspace", { project_id: project.id })
    ).active_project.revision,
    payload: {
      workflow_ref: workflow.id,
      role_agent_bindings: {},
      orchestration: { mode: "rust" },
    },
  });
  const status = () =>
    rpc(page, "query.workflow_instance", {
      project_id: project.id,
      id: root.id,
      view: "status",
      step_limit: 100,
    });
  await expect
    .poll(
      async () =>
        (await status()).runtime.steps.filter(
          (step: { status: string }) => step.status === "waiting_confirmation",
        ).length,
    )
    .toBe(35);
  const current = await status();
  const sorted = current.runtime.steps
    .map((step: { id: string }) => step.id)
    .sort();
  const driver = await rpc(page, "query.workflow_driver", {
    project_id: project.id,
    id: root.id,
    view: "summary",
  });
  const target = driver.waiting.find(
    (s: { execution_id: string }) => sorted.indexOf(s.execution_id) >= 20,
  );
  expect(target).toBeTruthy();
  const originalReason = await rpc(page, "query.workflow_instance", {
    project_id: project.id,
    id: root.id,
    execution_id: target.execution_id,
    view: "reason",
  });
  expect(originalReason.revision).toBe(target.step_revision);
  expect(originalReason.reason ?? "").toBe(target.reason);
  await page.goto(
    `/?tab=execution&detail=manager&project_id=${project.id}&instance_id=${root.id}`,
  );
  // The manager summary exposes actual blocked executions independent of the
  // workflow's current twenty-step page.
  const link = page
    .locator(`a[href*="execution_id=${target.execution_id}"]`)
    .filter({ hasText: "查看流程阻塞点" })
    .first();
  await link
    .locator("xpath=ancestor::details[1]")
    .locator(":scope > summary")
    .click();
  await expect(link).toBeVisible();
  const href = await link.getAttribute("href");
  expect(href).toContain("detail=overview");
  expect(href).not.toContain("feature_id=");
  await link.click();
  const step = page.locator(`#workflow-execution-${target.execution_id}`);
  await expect(step).toBeVisible();
  await expect(step).toHaveAttribute("aria-current", "step");
  await expect(step.locator(".workflow-step-toggle")).toBeFocused();
  await page.reload();
  await expect(step).toBeVisible();
  await page.goBack();
  await expect(page).toHaveURL(/detail=manager/);
  await page.goForward();
  await expect(step).toBeVisible();
  await page.screenshot({
    path: test.info().outputPath("workflow-block-focus.png"),
    fullPage: true,
  });
});

test("根经理页处理子流程的真实例外申请，理由必填且只批准确切结果", async ({
  page,
}) => {
  await page.goto("/");
  const project = await rpc(page, "project.create", {
    command_id: crypto.randomUUID(),
    payload: { title: "子流程例外审批" },
  });
  const spec = await rpc(page, "spec.create", {
    command_id: crypto.randomUUID(),
    payload: {
      name: "报告字段",
      data: {
        schema_version: 3,
        body: "读取当前报告",
        checks: [{ key: "evidence", name: "证据", instructions: "核对证据" }],
      },
    },
  });
  const delivery = await rpc(page, "delivery_type.create", {
    command_id: crypto.randomUUID(),
    payload: {
      name: "例外报告",
      spec_refs: [spec.id],
      data: { format: "json", content_schema: { type: "object" } },
    },
  });
  async function publish(name: string, document: object) {
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
  }
  const child = await publish("Task 证据评审", {
    schema_version: 6,
    stages: [{ key: "work", name: "评审" }],
    rework_policies: {
      repair: {
        entry_step: "author",
        allowed_targets: [
          "producer_step",
          "after_confirmed_checkpoint",
          "child_workflow",
        ],
        max_reworks: 0,
      },
    },
    review_policies: {
      report: {
        review_step: "review",
        rejected_when: {
          op: "equals",
          source: {
            step: "review",
            port: "report",
            pointer: "/inputs/approve",
          },
          value: false,
        },
        rework_policy: "repair",
        allow_human_approval: true,
      },
    },
    steps: [
      {
        key: "author",
        name: "责任步骤",
        stage: "work",
        executor: { type: "control" },
      },
      {
        key: "review",
        name: "独立报告",
        stage: "work",
        dependencies: ["author"],
        executor: {
          type: "tool",
          tool_ref: "builtin-tool-materials-capture:2",
          arguments: {
            approve: { type: "constant", value: false },
            issues: {
              type: "constant",
              value: "M-001 至 M-004 浏览器观察缺失",
            },
          },
        },
        outputs: { report: { delivery_ref: delivery.id } },
        completion: {
          op: "equals",
          source: {
            step: "review",
            port: "report",
            pointer: "/inputs/approve",
          },
          value: true,
        },
      },
    ],
  });
  const parent = await publish("根经理审批聚合", {
    schema_version: 6,
    stages: [{ key: "work", name: "Task" }],
    steps: [
      {
        key: "task",
        name: "对应 Task",
        stage: "work",
        executor: { type: "workflow", workflow_ref: child.id },
      },
    ],
  });
  const root = await rpc(page, "workflow.start", {
    project_id: project.id,
    command_id: crypto.randomUUID(),
    target: project.id,
    expected_revision: (
      await rpc(page, "query.workspace", { project_id: project.id })
    ).active_project.revision,
    payload: {
      workflow_ref: parent.id,
      role_agent_bindings: {},
      orchestration: { mode: "rust" },
    },
  });
  const readRoot = () =>
    rpc(page, "query.workflow_instance", {
      project_id: project.id,
      id: root.id,
    });
  await expect.poll(async () => (await readRoot()).children.length).toBe(1);
  const childId = (await readRoot()).children[0].id;
  const readChild = () =>
    rpc(page, "query.workflow_instance", {
      project_id: project.id,
      id: childId,
    });
  await expect
    .poll(async () => (await readChild()).runtime.steps[1].status)
    .toBe("awaiting_acceptance");
  const before = await readChild();
  const review = before.runtime.steps[1];
  await rpc(page, "workflow.decision.request", {
    project_id: project.id,
    command_id: crypto.randomUUID(),
    target: childId,
    expected_revision: before.record.revision,
    payload: {
      plan_revision_id: before.runtime.active_plan_revision_id,
      control_version: before.runtime.control_version,
      operation: "workflow.step.approve",
      reason: "经理建议接受此确切报告的已知缺口",
      arguments: {
        instance_id: childId,
        plan_revision_id: before.runtime.active_plan_revision_id,
        step: "review",
        execution_id: review.id,
        input_digest: review.input_digest,
        outputs: review.outputs,
      },
      interaction: {
        title: "例外批准当前 Task",
        background: "独立评审未通过",
        requested_action: "决定是否接受当前候选的具体浏览器证据缺口",
        completed_work: "已核对当前报告及输入",
        review_summary: "M-001 至 M-004 观察缺失",
        issues: "接受未执行浏览器验收的风险",
        recommendation: "建议当前版本例外批准；拒绝后停止，额度已耗尽",
        materials: [
          {
            label: "原拒绝报告",
            purpose: "核对未通过项目",
            instance_id: childId,
            execution_id: review.id,
            output_id: review.outputs.report,
          },
        ],
        evidence: [],
      },
    },
  });
  await page.goto(
    `/?tab=execution&detail=manager&project_id=${project.id}&instance_id=${root.id}`,
  );
  const card = page
    .locator(".manager-child-flow")
    .getByRole("article", { name: "例外批准当前 Task", exact: true });
  await expect(card).toBeVisible();
  const blocker = page
    .getByRole("region", { name: "当前阻塞点", exact: true })
    .locator(`a[href*="execution_id=${review.id}"]`)
    .filter({ hasText: "查看流程阻塞点" })
    .first();
  await expect(blocker).toBeVisible();
  const reviewHref = await blocker.getAttribute("href");
  expect(reviewHref).toBeTruthy();
  await blocker.click();
  await expect(page.locator(`#workflow-execution-${review.id}`)).toBeVisible();
  await expect(
    page.locator(`#workflow-execution-${review.id} .workflow-step-toggle`),
  ).toBeFocused();
  await page.reload();
  await expect(page.locator(`#workflow-execution-${review.id}`)).toBeVisible();
  await page.goto(
    `/?tab=execution&detail=manager&project_id=${project.id}&instance_id=${root.id}`,
  );
  await expect(card).toBeVisible();
  await expect(
    page.getByRole("article", { name: "例外批准当前 Task", exact: true }),
  ).toHaveCount(1);
  await expect(card.getByText("待用户 Approve", { exact: true })).toBeVisible();
  const approve = card.getByRole("button", {
    name: "确认：例外批准当前 Task",
    exact: true,
  });
  await expect(approve).toBeDisabled();
  await card
    .getByLabel("意见或修改要求")
    .fill("我已核对当前报告，接受本次浏览器观察缺口及风险");
  await approve.click();
  await expect
    .poll(
      async () =>
        (await readChild()).completion_status[review.id]?.confirmation
          ?.manual_approval,
    )
    .toBe(true);
  const after = await readChild();
  expect(after.outputs[review.outputs.report].value.inputs.approve).toBe(false);
  await page.goto(reviewHref!);
  await expect(
    page
      .locator(`#workflow-execution-${review.id}`)
      .getByRole("status")
      .filter({ hasText: "评审未通过，用户已例外批准" }),
  ).toBeVisible();
  await page.screenshot({
    path: test.info().outputPath("manager-child-exception.png"),
    fullPage: true,
  });
});

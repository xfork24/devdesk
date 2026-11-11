import { test, expect, type Page } from "@playwright/test";
import { mkdtempSync } from "node:fs";
import { tmpdir } from "node:os";
import { join } from "node:path";
import { useMockCodexCatalog } from "./support/model-catalog";
useMockCodexCatalog();

async function rpc(page: Page, method: string, params: any = {}) {
  const response = await page.request.post("/rpc", {
    data: { jsonrpc: "2.0", id: 1, method, params },
  });
  const value = await response.json();
  expect(value.error, JSON.stringify(value)).toBeUndefined();
  return value.result;
}
async function setup(page: Page) {
  await page.goto("/");
  if (
    (await page.request.get("/capabilities").then((r) => r.json()))
      .setup_required
  ) {
    await page
      .getByLabel("数据目录")
      .fill(mkdtempSync(join(tmpdir(), "devdesk-standard-web-")));
    await page.getByRole("button", { name: "初始化 / 打开数据集" }).click();
  }
  await expect(
    page.getByRole("button", { name: "流程与规范", exact: true }),
  ).toBeVisible();
}
async function command(
  page: Page,
  method: string,
  project: string | undefined,
  target: string | undefined,
  revision: number | undefined,
  payload: any = {},
) {
  return rpc(page, method, {
    project_id: project,
    command_id: crypto.randomUUID(),
    target,
    expected_revision: revision,
    payload,
  });
}
async function publish(page: Page, document: any) {
  const draft = await command(
    page,
    "workflow.create",
    undefined,
    undefined,
    undefined,
    { name: "快照工作流程 " + crypto.randomUUID().slice(0, 6), document },
  );
  return command(page, "workflow.publish", undefined, draft.id, draft.revision);
}
async function fixtures(page: Page) {
  const spec = await command(
    page,
    "spec.create",
    undefined,
    undefined,
    undefined,
    {
      name: "浏览器材料规范",
      data: {
        schema_version: 3,
        body: "核对测试场景明确声明的字段、预期与真实来源。",
        checks: [
          {
            key: "fixture_record",
            name: "场景材料",
            instructions: "按当前输入核对字段和值，保留来源。",
          },
        ],
      },
    },
  );
  const create = async (type: string) =>
    (
      await command(
        page,
        "delivery_type.create",
        undefined,
        undefined,
        undefined,
        {
          name: `Standard browser ${type}`,
          data: {
            format: "json",
            spec_refs: [spec.id],
            content_schema: { type },
          },
        },
      )
    ).id;
  return {
    material: await create("object"),
    collection: await create("array"),
  };
}
async function child(page: Page, marker: string, material: string) {
  const spec = await command(
    page,
    "spec.create",
    undefined,
    undefined,
    undefined,
    {
      name: marker + " 规范",
      data: {
        schema_version: 3,
        body: `检查 ${marker} 的实际材料来源。`,
        checks: [],
      },
    },
  );
  return publish(page, {
    schema_version: 6,
    stages: [{ key: "work", name: "材料处理" }],
    inputs: { item: { delivery_ref: material } },
    steps: [
      {
        key: "capture",
        name: "封存材料",
        stage: "work",
        spec_refs: [spec.id],
        executor: {
          type: "tool",
          tool_ref: "builtin-tool-materials-capture:1",
          arguments: {
            marker: { type: "constant", value: marker },
            item: { type: "source", source: { step: "$input", port: "item" } },
          },
        },
        outputs: { result: { delivery_ref: material } },
      },
      {
        key: "finish",
        name: "结束核对",
        stage: "work",
        dependencies: ["capture"],
        executor: { type: "control" },
      },
    ],
    outputs: { result: { step: "capture", port: "result" } },
  });
}
async function control(
  page: Page,
  method: string,
  project: string,
  instance: string,
  payload: any = {},
) {
  const view = await rpc(page, "query.workflow_instance", {
    project_id: project,
    id: instance,
  });
  return command(page, method, project, instance, view.record.revision, {
    plan_revision_id: view.runtime.active_plan_revision_id,
    ...payload,
  });
}
async function captureStep(
  page: Page,
  project: string,
  instance: string,
  step: string,
) {
  await control(page, "workflow.step.dispatch", project, instance, { step });
  await expect
    .poll(
      async () =>
        (
          await rpc(page, "query.workflow_instance", {
            project_id: project,
            id: instance,
          })
        ).runtime.steps.find((s: any) => s.step === step).status,
    )
    .toBe("awaiting_acceptance");
  const view = await rpc(page, "query.workflow_instance", {
    project_id: project,
    id: instance,
  });
  const execution = view.runtime.steps.find((s: any) => s.step === step);
  await control(page, "workflow.step.accept", project, instance, {
    step,
    execution_id: execution.id,
    input_digest: execution.input_digest,
    outputs: execution.outputs,
    reason: "核对实际材料工具回执",
  });
  return { view, execution };
}

test("Standard 只读生成比较与归档保留规范源", async ({ page }, info) => {
  await setup(page);
  const { material } = await fixtures(page);
  const a = await child(page, "规范 A", material),
    b = await child(page, "规范 B", material);
  await page.getByRole("button", { name: "流程与规范", exact: true }).click();
  await page.getByRole("tab", { name: "Standard", exact: true }).click();
  const panel = page.getByRole("region", { name: "Standard 规范快照" });
  await panel.getByText("从 Workflow 生成快照", { exact: true }).click();
  const ids: string[] = [];
  for (const [workflow, suffix] of [
    [a, "A"],
    [b, "B"],
  ] as const) {
    const name =
      "Web 不可变规范 " + suffix + " " + crypto.randomUUID().slice(0, 6);
    await panel.getByLabel("快照名称", { exact: true }).fill(name);
    await panel
      .getByLabel("入口 Workflow", { exact: true })
      .selectOption(workflow.id);
    await panel
      .getByRole("button", { name: "预览规范快照", exact: true })
      .click();
    await expect(
      panel.getByRole("button", { name: "生成快照", exact: true }),
    ).toBeVisible();
    await panel.getByRole("button", { name: "生成快照", exact: true }).click();
    await expect(
      panel.getByRole("heading", { name: new RegExp(name) }),
    ).toBeVisible();
    ids.push(
      await panel.getByLabel("查看 Standard", { exact: true }).inputValue(),
    );
  }
  await expect(panel.getByRole("button", { name: "添加覆盖规则" })).toHaveCount(
    0,
  );
  await expect(
    panel.getByRole("textbox", { name: "正文", exact: true }),
  ).toHaveCount(0);
  await panel
    .getByLabel("与另一个 Standard 比较", { exact: true })
    .selectOption(ids[0]);
  await panel.getByRole("button", { name: "比较规范", exact: true }).click();
  await expect(panel.locator(".standard-diff")).toContainText("规范变化");
  await panel.getByLabel("查看 Standard", { exact: true }).selectOption(ids[1]);
  await panel.getByRole("button", { name: "归档快照", exact: true }).click();
  await expect(
    panel.getByRole("button", { name: "恢复快照选择", exact: true }),
  ).toBeVisible();
  await page.reload();
  await expect(
    page.getByRole("button", { name: "恢复快照选择", exact: true }),
  ).toBeVisible();
  await page.screenshot({path:info.outputPath("standard-readonly-archived.png"),fullPage:true});
  const catalog = await rpc(page, "query.standard", { id: ids[1] });
  expect(Object.keys(catalog.record.data).sort()).toEqual([
    "root_workflow_refs",
    "schema_version",
  ]);
  await page.setViewportSize({width:390,height:844});
  await expect.poll(() => page.evaluate(() => document.documentElement.scrollWidth <= document.documentElement.clientWidth + 1)).toBe(true);
  await page.screenshot({path:info.outputPath("standard-readonly-mobile.png"),fullPage:true});
});

test("同一 Feature 的 Task 实际采用不同 Standard 且切换保留执行依据", async ({
  page,
}) => {
  await setup(page);
  const { material, collection } = await fixtures(page);
  const a = await child(page, "A", material),
    b = await child(page, "B", material);
  const parent = await publish(page, {
    schema_version: 6,
    stages: [{ key: "work", name: "工作" }],
    steps: [
      {
        key: "create",
        name: "创建两项工作",
        stage: "work",
        executor: {
          type: "tool",
          tool_ref: "builtin-tool-tasks-create:1",
          arguments: {
            tasks: {
              type: "constant",
              value: [
                {
                  key: "a",
                  title: "Task A",
                  instructions: "按当前规范处理材料",
                  dependencies: [],
                },
                {
                  key: "b",
                  title: "Task B",
                  instructions: "按当前规范处理材料",
                  dependencies: [],
                },
              ],
            },
          },
        },
        outputs: { result: { delivery_ref: material } },
      },
      {
        key: "call",
        name: "逐工作采用规范",
        stage: "work",
        dependencies: ["create"],
        executor: {
          type: "workflow",
          workflow_ref: a.id,
          work_binding: { input: "item", pointer: "/work_ref" },
          foreach: {
            source: { step: "create", port: "result", pointer: "/tasks" },
            item_input: "item",
            max_concurrency: 2,
            min_items: 2,
          },
        },
        outputs: { result: { delivery_ref: collection } },
      },
    ],
  });
  const sa = await command(
    page,
    "standard.capture",
    undefined,
    undefined,
    undefined,
    { name: "共同规范 A", root_workflow_refs: [parent.id] },
  );
  const sb = await command(
    page,
    "standard.capture",
    undefined,
    undefined,
    undefined,
    { name: "材料规范 B", root_workflow_refs: [b.id] },
  );
  const project = await command(
    page,
    "project.create",
    undefined,
    undefined,
    undefined,
    { title: "Standard 浏览器验收" },
  );
  await command(page, "project.open", project.id, project.id, undefined);
  let workspace = await rpc(page, "query.workspace", {
    project_id: project.id,
  });
  await command(
    page,
    "project.configuration.update",
    project.id,
    project.id,
    workspace.active_project.revision,
    { workflow_ref: parent.id, standard_ref: sa.id, role_agent_bindings: {} },
  );
  const feature = await command(
    page,
    "feature.create",
    project.id,
    undefined,
    undefined,
    { project_id: project.id, title: "两项工作分别采用规范" },
  );
  let root = await rpc(page, "query.workflow_instance", {
    project_id: project.id,
    owner_id: feature.id,
  });
  await captureStep(page, project.id, root.record.id, "create");
  workspace = await rpc(page, "query.workspace", {
    project_id: project.id,
    feature_id: feature.id,
  });
  const tb = workspace.detail.tasks.find((t: any) => t.title === "Task B");
  await page.goto(
    `/?tab=execution&project_id=${project.id}&feature_id=${feature.id}`,
  );
  const card = page.locator(".task-grid article").filter({ has: page.getByText("Task B", {exact:true}) });
  await card.locator(".work-standard > summary").click();
  await card.getByLabel("采用 Standard", { exact: true }).selectOption(sb.id);
  await card
    .getByLabel("采用的入口 Workflow", { exact: true })
    .selectOption(b.id);
  await card
    .getByRole("button", { name: "保存采用的规范", exact: true })
    .click();
  await expect
    .poll(
      async () =>
        (
          await rpc(page, "query.work_configuration", {
            project_id: project.id,
            owner_id: tb.id,
          })
        ).configuration.data.standard_ref,
    )
    .toBe(sb.id);
  await control(page, "workflow.step.dispatch", project.id, root.record.id, {
    step: "call",
  });
  await expect
    .poll(
      async () =>
        (
          await rpc(page, "query.workspace", {
            project_id: project.id,
            feature_id: feature.id,
          })
        ).configurations.filter(
          (r: any) =>
            r.kind === "workflow_instance" && r.data.parent_execution_id,
        ).length,
    )
    .toBe(2);
  workspace = await rpc(page, "query.workspace", {
    project_id: project.id,
    feature_id: feature.id,
  });
  const children = workspace.configurations.filter(
    (r: any) => r.kind === "workflow_instance" && r.data.parent_execution_id,
  );
  const childB = children.find((r: any) => r.data.owner_id === tb.id);
  expect(childB.data.plan.standard_ref).toBe(sb.id);
  const old = await captureStep(page, project.id, childB.id, "capture");
  const oldPlan = old.view.runtime.active_plan_revision_id;
  const oldOutput = old.view.outputs[old.execution.outputs.result];
  expect(oldOutput.value.inputs.marker).toBe("B");
  await page.reload();
  const updatedCard = page
    .locator(".task-grid article")
    .filter({ has: page.getByText("Task B", {exact:true}) });
  await updatedCard.locator(".work-standard > summary").click();
  await updatedCard.getByText("与其他工作比较规范", {exact:true}).click();
  const ta = workspace.detail.tasks.find((t:any) => t.title === "Task A");
  await updatedCard.getByLabel("比较工作", {exact:true}).selectOption(ta.id);
  await updatedCard.getByRole("button",{name:"比较工作规范",exact:true}).click();
  await expect(updatedCard.locator(".standard-diff").first()).toContainText("规范变化");
  await updatedCard
    .getByLabel("采用 Standard", { exact: true })
    .selectOption(sa.id);
  await updatedCard
    .getByLabel("采用的入口 Workflow", { exact: true })
    .selectOption(a.id);
  await updatedCard
    .getByLabel("规范切换原因", { exact: true })
    .fill("本次修改显式采用 A 规范，保留 B 的原始依据");
  await updatedCard
    .getByRole("button", { name: "预演规范切换", exact: true })
    .click();
  await expect(
    updatedCard.getByRole("button", { name: "应用规范切换", exact: true }),
  ).toBeVisible();
  await updatedCard
    .getByRole("button", { name: "应用规范切换", exact: true })
    .click();
  await expect
    .poll(
      async () =>
        (
          await rpc(page, "query.workflow_instance", {
            project_id: project.id,
            id: childB.id,
          })
        ).runtime.plan.standard_ref,
    )
    .toBe(sa.id);
  const history = await rpc(page, "query.work_configuration", {
    project_id: project.id,
    owner_id: tb.id,
  });
  expect(
    history.history.find((r: any) => r.id === oldPlan).data.standard_ref,
  ).toBe(sb.id);
  const output = await rpc(page, "workflow.outputs.read", {
    project_id: project.id,
    id: childB.id,
    ids: [old.execution.outputs.result],
  });
  expect(JSON.stringify(output)).toContain('"marker":"B"');
  root = await rpc(page, "query.workflow_instance", {
    project_id: project.id,
    id: root.record.id,
  });
  expect(root.runtime.plan.standard_ref).toBe(sa.id);
});

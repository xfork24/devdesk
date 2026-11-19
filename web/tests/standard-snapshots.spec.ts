import { expect, type Page } from "@playwright/test";
import { test } from "./support/kernel";
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
    await expect(page.getByLabel("数据目录")).toHaveCount(0);
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
          spec_refs: [spec.id],
          name: `Standard browser ${type}`,
          data: { format: "json", content_schema: { type } },
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

test("Standard 内容相同时，默认启动入口未声明或不同均显示规范定义相同", async ({
  page,
}, info) => {
  await setup(page);
  const defaults = await rpc(page, "query.configuration_defaults");
  const base = await rpc(page, "query.standard", { id: defaults.standard_ref });
  const copies = [];
  let previous = base.record;
  for (const entry of [null, defaults.task_workflow_ref]) {
    const copy = await command(
      page,
      "standard.capture",
      undefined,
      previous.id,
      previous.revision,
      {
        name: "相同规范的入口元数据 " + crypto.randomUUID().slice(0, 6),
        root_workflow_refs: base.root_workflow_refs,
        default_workflow_ref: entry,
      },
    );
    expect(copy.default_workflow_ref ?? null).toBe(entry);
    copies.push(copy);
    previous = copy;
  }
  await page.getByRole("button", { name: "流程与规范", exact: true }).click();
  await page.getByRole("tab", { name: "Standard", exact: true }).click();
  const panel = page.getByRole("region", { name: "Standard 规范快照" });
  await panel.getByLabel("查看 Standard", { exact: true }).focus();
  await panel
    .getByLabel("查看 Standard", { exact: true })
    .selectOption(defaults.standard_ref);
  for (const copy of copies) {
    await panel.getByLabel("与另一个 Standard 比较", { exact: true }).focus();
    await panel
      .getByLabel("与另一个 Standard 比较", { exact: true })
      .selectOption(copy.id);
    await panel.getByRole("button", { name: "比较规范", exact: true }).click();
    await expect(panel.locator(".standard-diff")).toHaveText("规范定义相同");
  }
  const comparison = panel.getByRole("region", {
    name: "Standard 比较",
    exact: true,
  });
  const select = comparison.getByLabel("与另一个 Standard 比较", {
    exact: true,
  });
  const selectorBox = (await select.boundingBox())!;
  const buttonBox = (await comparison
    .getByRole("button", { name: "比较规范", exact: true })
    .boundingBox())!;
  expect(Math.abs(selectorBox.y - buttonBox.y)).toBeLessThan(2);
  expect(Math.abs(selectorBox.height - buttonBox.height)).toBeLessThan(2);
  const titleCenter = await select.locator("..").evaluate((label) => {
    const range = document.createRange();
    range.selectNodeContents(label.firstChild!);
    const rect = range.getBoundingClientRect();
    return rect.y + rect.height / 2;
  });
  expect(
    Math.abs(titleCenter - selectorBox.y - selectorBox.height / 2),
  ).toBeLessThan(2);
  const editor = (await panel
    .getByRole("region", { name: "生成 Standard 新版本", exact: true })
    .boundingBox())!;
  expect(selectorBox.y).toBeGreaterThan(editor.y + editor.height);
  const usage = panel
    .locator("summary")
    .filter({ hasText: "使用位置 · 当前 Standard：" });
  await expect(usage).toContainText(
    `${base.record.name} · v${base.record.version}`,
  );
  await comparison.screenshot({
    path: info.outputPath("standard-comparison-row.png"),
  });
  await panel.getByLabel("查看 Standard", { exact: true }).focus();
  await panel
    .getByLabel("查看 Standard", { exact: true })
    .selectOption(copies[0].id);
  await expect(usage).toContainText(
    `${copies[0].name} · v${copies[0].version}`,
  );
});

test("Standard 在配置项上替换版本并生成新修订，历史快照仍可查看", async ({
  page,
}, info) => {
  await setup(page);
  const { material } = await fixtures(page);
  const a = await child(page, "规范 A", material);
  const b = await child(page, "规范 B", material);
  const original = await command(
    page,
    "standard.capture",
    undefined,
    undefined,
    undefined,
    {
      name: "可替换的规范",
      root_workflow_refs: [a.id],
    },
  );
  await page.goto(
    `/?tab=workflows&config_tab=standard&standard_id=${encodeURIComponent(original.id)}`,
  );
  const panel = page.getByRole("region", { name: "Standard 规范快照" });
  await expect(
    panel.getByText("从 Workflow 生成快照", { exact: true }),
  ).toHaveCount(0);
  await expect(
    panel.getByRole("button", { name: "归档快照", exact: true }),
  ).toHaveCount(0);
  const baseline = await rpc(page, "query.standard", {
    id: original.id,
    limit: 1000,
  });
  const root = baseline.nodes.find((node: any) => node.reference === a.id);
  const source = panel.getByRole("region", {
    name: `${root.name} · ${a.id}`,
    exact: true,
  });
  await source.locator(":scope > details > summary").click();
  const rowAlignment = await source
    .locator(".standard-source-override > label")
    .evaluate((label) => {
      const title = label
        .querySelector(".configuration-field-title")!
        .getBoundingClientRect();
      const control = label.querySelector("select")!.getBoundingClientRect();
      return Math.abs(
        title.y + title.height / 2 - control.y - control.height / 2,
      );
    });
  expect(rowAlignment).toBeLessThan(2);
  await source.getByRole("combobox").selectOption(b.id);
  await panel.getByLabel("快照名称", { exact: true }).fill("替换后的规范");
  const save = panel.getByRole("button", { name: "生成新版本", exact: true });
  await expect(save).toBeDisabled();
  await panel.getByRole("button", { name: "预览替换", exact: true }).click();
  await expect(save).toBeEnabled();
  await expect(
    panel.locator(".standard-revision-save .standard-diff"),
  ).toContainText("规范变化");
  await page.screenshot({
    path: info.outputPath("standard-override-preview.png"),
    fullPage: true,
  });
  await save.click();
  const selected = panel.getByLabel("查看 Standard", { exact: true });
  await expect(selected).not.toHaveValue(original.id);
  const next = await rpc(page, "query.standard", {
    id: await selected.inputValue(),
    limit: 1000,
  });
  expect(next.record.version).toBe(2);
  expect(next.record.supersedes).toBe(original.id);
  expect(next.record.name).toBe("替换后的规范");
  expect(next.root_workflow_refs).toEqual([b.id]);
  expect(
    (await rpc(page, "query.standard", { id: original.id })).root_workflow_refs,
  ).toEqual([a.id]);
  await panel.getByLabel("与另一个 Standard 比较", { exact: true }).focus();
  await panel
    .getByLabel("与另一个 Standard 比较", { exact: true })
    .selectOption(original.id);
  await panel.getByRole("button", { name: "比较规范", exact: true }).click();
  await expect(panel.locator(".standard-compare .standard-diff")).toContainText(
    "规范变化",
  );
  await command(
    page,
    "standard.archive",
    undefined,
    next.record.id,
    next.record.revision,
    { archived: true },
  );
  await page.goto("/?tab=workflows&config_tab=standard");
  await expect(
    selected.locator(`option[value="${next.record.id}"]`),
  ).toHaveCount(0);
  await panel.getByLabel("显示历史与归档快照", { exact: true }).check();
  await expect(
    selected.locator(`option[value="${next.record.id}"]`),
  ).toHaveCount(1);
  await selected.selectOption(next.record.id);
  await expect(
    panel.getByText("此快照已归档，可查看历史内容或基于它生成新快照。"),
  ).toBeVisible();
  await page.setViewportSize({ width: 390, height: 844 });
  await expect
    .poll(() =>
      page.evaluate(
        () =>
          document.documentElement.scrollWidth <=
          document.documentElement.clientWidth + 1,
      ),
    )
    .toBe(true);
  await page.screenshot({
    path: info.outputPath("standard-override-mobile.png"),
    fullPage: true,
  });
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
  let workspace = await rpc(page, "query.workspace", {
    project_id: project.id,
  });
  const projectSelection = await command(
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
    `/?tab=execution&detail=manager&project_id=${project.id}&feature_id=${feature.id}`,
  );
  await expect(
    page.getByRole("region", { name: "项目经理协作", exact: true }),
  ).toBeVisible();
  const card = page
    .locator(".task-grid article")
    .filter({ has: page.getByText("Task B", { exact: true }) });
  await page.getByText("待启动任务的规范配置", { exact: true }).click();
  await card.getByRole("button", { name: "采用的规范", exact: true }).click();
  await expect(
    card.getByRole("button", { name: "保存采用的规范", exact: true }),
  ).toHaveCount(0);
  await expect(
    card.getByLabel("采用 Standard", { exact: true }),
  ).toBeDisabled();
  await command(
    page,
    "work.configuration.update",
    project.id,
    tb.id,
    tb.revision,
    {
      standard_ref: sb.id,
      workflow_ref: b.id,
      role_agent_bindings: {},
    },
  );
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
            r.type === "workflow_instance" && r.data.parent_execution_id,
        ).length,
    )
    .toBe(2);
  workspace = await rpc(page, "query.workspace", {
    project_id: project.id,
    feature_id: feature.id,
  });
  const children = workspace.configurations.filter(
    (r: any) => r.type === "workflow_instance" && r.data.parent_execution_id,
  );
  const childB = children.find((r: any) => r.data.owner_id === tb.id);
  expect(childB.data.plan.standard_ref).toBe(sb.id);
  const old = await captureStep(page, project.id, childB.id, "capture");
  const oldPlan = old.view.runtime.active_plan_revision_id;
  const oldOutput = old.view.outputs[old.execution.outputs.result];
  expect(oldOutput.value.inputs.marker).toBe("B");
  await page.reload();
  await expect(page.locator(".task-grid article")).toHaveCount(0);
  const updatedCard = page.locator(
    `.manager-child-flow[data-instance-id="${childB.id}"]`,
  );
  await updatedCard
    .getByRole("button", { name: "采用的规范", exact: true })
    .click();
  await expect(
    updatedCard.getByText("与其他工作比较规范", { exact: true }),
  ).toHaveCount(0);
  await expect(updatedCard.getByText(/规范采用历史/)).toHaveCount(0);
  await updatedCard
    .getByLabel("采用 Standard", { exact: true })
    .selectOption(sa.id);
  await updatedCard.getByLabel("采用的入口 Workflow", { exact: true }).focus();
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
  expect(root.runtime.plan.standard_ref).toBe(
    projectSelection.data.standard_ref,
  );
});

import { test, expect, Page } from "@playwright/test";
import { mkdtempSync } from "node:fs";
import { tmpdir } from "node:os";
import { join } from "node:path";
import { useMockCodexCatalog } from "./support/model-catalog";
useMockCodexCatalog();
async function setup(page: Page) {
  await page.goto("/");
  if ((await page.request.get("/capabilities").then((r) => r.json())).setup_required) {
    await page
      .getByLabel("数据目录")
      .fill(mkdtempSync(join(tmpdir(), "devdesk-config-web-")));
    await page.getByRole("button", { name: "初始化 / 打开数据集" }).click();
  }
  await expect(
    page.getByRole("button", { name: "流程与规范", exact: true }),
  ).toBeVisible();
}
async function rpc(page: Page, method: string, params: any = {}) {
  const response = await page.request.post("/rpc", {
    data: { jsonrpc: "2.0", id: 1, method, params },
  });
  const value = await response.json();
  expect(value.error, JSON.stringify(value)).toBeUndefined();
  return value.result;
}
test("五组导航与默认配置完整，Role 独立编辑且不派生 Agent", async ({
  page,
}) => {
  await setup(page);
  for (const name of ["项目管理", "流程与规范", "角色与 Agent", "基础组件"])
    await expect(page.getByRole("button", { name, exact: true })).toBeVisible();
  await page.getByRole("button", { name: "项目管理", exact: true }).click();
  await expect(
    page.getByText("默认配置已准备好", { exact: true }),
  ).toBeVisible();
  const defaults = await rpc(page, "query.configuration_defaults");
  expect(Object.keys(defaults.role_agent_bindings)).toHaveLength(6);
  expect(defaults.capability_ref).toBe("builtin-capability-sol-high-codex-v2");
  const tools = await rpc(page, "query.configurations", {
    kind: "tool",
    include_history: true,
  });
  expect(
    tools.records.filter(
      (row: any) => row.data.implementation === "harness.native",
    ),
  ).toHaveLength(2);
  for (const row of defaults.records.filter((row: any) =>
    ["capability", "agent"].includes(row.kind),
  ))
    expect(row.data.tools).toEqual(["builtin-tool-codex-native-v1"]);
  await page.getByRole("button", { name: "基础组件", exact: true }).click();
  await page.getByRole("tab", { name: "Harness原生工具", exact: true }).click();
  await expect(page.getByText("AgentID 经验检索", { exact: true })).toHaveCount(
    0,
  );
  await expect(
    page.getByRole("region", { name: "原生工具目录" }),
  ).toBeVisible();
  const before = await rpc(page, "query.configurations", {
    kind: "agent",
    include_history: true,
    limit: 100,
  });
  await page.getByRole("button", { name: "角色与 Agent", exact: true }).click();
  const role = page.getByRole("region", { name: "角色管理" });
  const name = "安全评审 " + crypto.randomUUID().slice(0, 6);
  await role.getByLabel("Role 名称", { exact: true }).fill(name);
  await role
    .getByRole("button", { name: "职责说明", exact: true })
    .click();
  await role
    .getByRole("textbox", { name: "职责说明", exact: true })
    .fill("# Duties\n\nReview evidence and security boundaries.");
  await role.getByRole("button", { name: "创建 Role", exact: true }).click();
  await expect(role.getByRole("status")).toHaveText(/Role 已保存/);
  await expect(
    role.getByRole("heading", { name: "Duties", level: 1 }),
  ).toBeVisible();
  await expect(
    role.getByRole("textbox", { name: "职责说明", exact: true }),
  ).toHaveCount(0);
  await role.getByLabel("Role 名称", { exact: true }).fill(name + " v2");
  await role
    .getByRole("button", { name: "保存 Role 新版本", exact: true })
    .click();
  await expect(
    role.getByRole("button", { name: new RegExp(name + " v2") }),
  ).toBeVisible();
  const after = await rpc(page, "query.configurations", {
    kind: "agent",
    include_history: true,
    limit: 100,
  });
  expect(after.total).toBe(before.total);
  await role.getByRole("button", { name: "删除 Role", exact: true }).click();
  await expect(
    role.getByRole("button", { name: new RegExp(name + " v2") }),
  ).toHaveCount(0);
});
test("原生工具目录提供独立当前入口且不编辑 Schema", async ({
  page,
}) => {
  await setup(page);
  await page.goto(
    "/?tab=components&config_tab=native&config_object=builtin-tool-codex-native-v1",
  );
  await expect(
    page.getByRole("region", { name: "原生工具目录" }),
  ).toBeVisible();
  await expect(
    page.getByRole("note", { name: "原生工具组说明" }),
  ).toBeVisible();
  await expect(
    page.getByLabel("输入 JSON Schema", { exact: true }),
  ).toHaveCount(0);
  await expect(
    page.getByLabel("输出 JSON Schema", { exact: true }),
  ).toHaveCount(0);
  await expect(
    page.getByRole("button", { name: "新建 Tool", exact: true }),
  ).toHaveCount(0);
});

test("基础组件、Capability 与 Agent 通过精确引用组合，跨 tab 保留草稿", async ({
  page,
}) => {
  await setup(page);
  await page.getByRole("button", { name: "基础组件", exact: true }).click();
  const name = "Browser Model " + crypto.randomUUID().slice(0, 5);
  await page.getByLabel("名称", { exact: true }).fill(name);
  await page.getByLabel("模型标识", { exact: true }).selectOption("fixture-model");
  await page.getByRole("button", { name: "创建 Model", exact: true }).click();
  await expect(
    page.getByRole("button", { name: new RegExp(name) }),
  ).toBeVisible();
  await page.getByRole("button", { name: "角色与 Agent", exact: true }).click();
  await page.getByRole("tab", { name: "Capability", exact: true }).click();
  await page.getByLabel("名称", { exact: true }).fill("Unfinished capability");
  await page.getByRole("tab", { name: "Agent", exact: true }).click();
  await page.getByRole("tab", { name: "Capability", exact: true }).click();
  await expect(page.getByLabel("名称", { exact: true })).toHaveValue(
    "Unfinished capability",
  );
  await page
    .getByLabel("Model 精确版本", { exact: true })
    .selectOption("builtin-model-gpt-6-1-sol-high-v1");
  await page
    .getByLabel("Harness 精确版本", { exact: true })
    .selectOption("builtin-harness-codex-managed-v1");
  await page
    .getByLabel("Tools", { exact: true })
    .selectOption("builtin-tool-codex-native-v1");
  const capName = "Browser capability " + crypto.randomUUID().slice(0, 5);
  await page.getByLabel("名称", { exact: true }).fill(capName);
  await page
    .getByRole("button", { name: "创建 Capability", exact: true })
    .click();
  await expect(
    page.getByRole("button", { name: new RegExp(capName) }),
  ).toBeVisible();
  const catalog = await rpc(page, "query.configurations", {
    kind: "capability",
    limit: 100,
  });
  const cap = catalog.records.find((r: any) => r.name === capName);
  expect(cap).toBeTruthy();
  await page.getByRole("tab", { name: "Agent", exact: true }).click();
  const agentName = "Browser analyst " + crypto.randomUUID().slice(0, 5);
  await page.getByLabel("名称", { exact: true }).fill(agentName);
  await page
    .getByLabel("Role 精确版本", { exact: true })
    .selectOption("builtin-role-analyst-v1");
  await page
    .getByLabel("Capability 精确版本", { exact: true })
    .selectOption(cap.id);
  await page.getByRole("button", { name: "创建 Agent", exact: true }).click();
  await expect(
    page.getByRole("button", { name: new RegExp(agentName) }),
  ).toBeVisible();
  await page.getByLabel("名称", { exact: true }).fill(agentName + " · 未保存");
  await page.reload();
  await expect(
    page.getByRole("tab", { name: "Agent", exact: true }),
  ).toHaveAttribute("aria-selected", "true");
  await expect(page.getByLabel("名称", { exact: true })).toHaveValue(
    agentName + " · 未保存",
  );
});
test("Workflow 两列编辑器保存发布并恢复对象及草稿", async ({ page }, info) => {
  await setup(page);
  await page.getByRole("button", { name: "流程与规范", exact: true }).click();
  await page
    .getByRole("button", { name: "新建 Workflow", exact: true })
    .click();
  const editor = page.getByRole("region", { name: "Workflow 编辑器" });
  const name = "浏览器子流程 " + crypto.randomUUID().slice(0, 6);
  await editor.getByLabel("Workflow 名称", { exact: true }).fill(name);
  await editor
    .getByLabel("流程用途", { exact: true })
    .selectOption("subworkflow");
  await editor.getByRole("button", { name: "添加步骤", exact: true }).click();
  await editor.getByLabel("步骤名称", { exact: true }).fill("确定性汇合");
  await editor
    .getByLabel("标准动作", { exact: true })
    .selectOption("attempt.execute");
  await editor
    .getByLabel("执行 Role", { exact: true })
    .selectOption("builtin-role-developer");
  await editor
    .getByLabel("步骤 Spec", { exact: true })
    .selectOption("builtin-spec-stage-implementation-v1");
  await editor.getByLabel("标准动作", { exact: true }).selectOption("join");

  await editor.getByRole("button", { name: "保存草稿", exact: true }).click();
  await expect(page.getByRole("status")).toHaveText(/草稿已保存/);
  await editor
    .getByLabel("步骤名称", { exact: true })
    .fill("确定性汇合 · 未保存");
  await page.reload();
  await expect(editor.getByLabel("步骤名称", { exact: true })).toHaveValue(
    "确定性汇合 · 未保存",
  );
  await editor
    .getByRole("button", { name: "校验与规范预览", exact: true })
    .click();
  await expect(page.getByText(/校验通过/)).toBeVisible();
  await editor
    .getByRole("button", { name: "保存可用版本", exact: true })
    .click();
  await expect(page.getByRole("status")).toHaveText(/可用版本已保存/);
  await page.reload();
  await expect(
    page.getByRole("region", { name: "Workflow 编辑器" }),
  ).toBeVisible();
  await expect(page.getByLabel("Workflow 名称", { exact: true })).toHaveValue(
    name,
  );
  await expect(
    page.getByRole("button", { name: "编辑流程", exact: true }),
  ).toBeVisible();
  await expect(
    page.getByText("正在连接本地内核...", { exact: false }),
  ).toHaveCount(0);
  await expect(page.locator(".workflow-editor-v2 > *")).toHaveCount(2);
  await page.screenshot({
    path: info.outputPath("configuration-workflow.png"),
    fullPage: true,
  });
});
test("规范与基础组件 Markdown 默认预览，源码修改后保存与重载不丢失", async ({
  page,
}) => {
  await setup(page);
  const body =
    '## Markdown 内容\n\n检查 **真实来源** 与 `revision`。\n\n- 保留换行\n- 保留列表\n\n| 字段 | 含义 |\n| --- | --- |\n| SHA | 代码修订 |\n\n```json\n{"source": "actual"}\n```';
  for (const [nav, kind] of [
    ["流程与规范", "Spec"],
    ["流程与规范", "Standard"],
    ["基础组件", "Skills"],
  ]) {
    await page.getByRole("button", { name: nav, exact: true }).click();
    await page.getByRole("tab", { name: kind, exact: true }).click();
    const resourceKind = kind === "Skills" ? "Skill" : kind;
    await page
      .getByRole("button", { name: `新建 ${resourceKind}`, exact: true })
      .click();
    const field = page.getByRole("group", {
      name: "正文",
      exact: true,
    });
    await expect(field.getByRole("textbox")).toHaveCount(0);
    await page
      .getByLabel("名称", { exact: true })
      .fill(`${kind} Markdown ` + crypto.randomUUID().slice(0, 6));
    await field
      .getByRole("button", { name: "正文", exact: true })
      .click();
    await field
      .getByRole("textbox", { name: "正文", exact: true })
      .fill(body);
    await field
      .getByRole("textbox", { name: "正文", exact: true }).press("Tab");
    await expect(field.getByRole("textbox")).toHaveCount(0);
    await expect(page.getByRole("button", { name: /编辑.*源码|预览正文/ })).toHaveCount(0);
    await expect(
      field.getByRole("heading", { name: "Markdown 内容", level: 2 }),
    ).toBeVisible();
    await expect(field.locator("strong")).toHaveText("真实来源");
    await expect(field.getByRole("table")).toBeVisible();
    await expect(field.locator("pre code")).toHaveText('{"source": "actual"}');
    if (kind === "Standard") {
      await page
        .getByRole("button", { name: "添加覆盖规则", exact: true })
        .click();
      await page
        .getByLabel("规则 1 必需 Spec", { exact: true })
        .selectOption("builtin-spec-stage-implementation-v1");
    }
    await page
      .getByRole("button", { name: `创建 ${resourceKind}`, exact: true })
      .click();
    await expect(page.getByRole("status")).toHaveText(/已保存/);
    await page.reload();
    await expect(
      field.getByRole("heading", { name: "Markdown 内容", level: 2 }),
    ).toBeVisible();
    await expect(field.getByRole("textbox")).toHaveCount(0);
    await field
      .getByRole("button", { name: "正文", exact: true })
      .click();
    await expect(
      field.getByRole("textbox", { name: "正文", exact: true }),
    ).toHaveValue(body);
    await page.getByLabel("名称", { exact: true }).click();
    await expect(field.getByRole("textbox")).toHaveCount(0);
    await expect(field.getByRole("table")).toBeVisible();
  }
});

test("阶段整体折叠且保留状态，讨论 Markdown 与已发布子流程可建立新修订", async ({
  page,
}, info) => {
  await setup(page);
  async function publish(name: string, document: any) {
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
  const child = await publish(
    "可编辑子流程 " + crypto.randomUUID().slice(0, 6),
    {
      schema_version: 3,
      process_kind: "subworkflow",
      stages: [{ key: "work", name: "子流程阶段" }],
      steps: [
        { key: "finish", name: "子流程汇合", kind: "join", stage: "work" },
      ],
    },
  );
  const goal = "## 讨论目标\n\n完成 **PRD**。";
  const topics = [
    "## 场景\n\n覆盖正常流程、失败路径, 并保留来源。",
    "- 需求\n- 验收",
  ];
  const parent = await publish(
    "阶段与讨论 " + crypto.randomUUID().slice(0, 6),
    {
      schema_version: 3,
      process_kind: "subworkflow",
      stages: [
        {
          key: "plan",
          name: "讨论与子流程",
          description: "## 规划\n\n讨论后执行子流程。",
        },
        { key: "end", name: "完成阶段" },
      ],
      steps: [
        {
          key: "discuss",
          name: "需求讨论",
          kind: "agent.interact",
          stage: "plan",
          role_ref: "builtin-role-analyst",
          contract_version: 2,
          task_instructions: goal,
          spec_refs: ["builtin-spec-stage-prd-v1"],
          interaction: {
            mode: "discussion",
            required_topics: topics,
            output_kind: "requirement",
            max_turns: 8,
          },
        },
        {
          key: "child",
          name: "调用独立子流程",
          kind: "workflow.call",
          stage: "plan",
          subworkflow_ref: child.id,
          dependencies: ["discuss"],
        },
        {
          key: "end",
          name: "流程结束",
          kind: "join",
          stage: "end",
          dependencies: ["child"],
        },
      ],
    },
  );
  await page.goto(`/?tab=workflows&workflow_id=${parent.id}`);
  const editor = page.getByRole("region", { name: "Workflow 编辑器" });
  const collapse = editor.getByRole("button", {
    name: "折叠阶段：讨论与子流程",
    exact: true,
  });
  await expect(collapse).toHaveAttribute("aria-expanded", "true");
  await collapse.click();
  await expect(
    editor.getByRole("button", { name: "需求讨论 角色讨论", exact: true }),
  ).toBeHidden();
  await expect(
    editor.getByRole("button", { name: /调用独立子流程/ }),
  ).toBeHidden();
  await expect(
    editor.getByRole("button", { name: "流程结束 汇合", exact: true }),
  ).toBeVisible();
  await page.reload();
  await expect(
    editor.getByRole("button", { name: "展开阶段：讨论与子流程", exact: true }),
  ).toHaveAttribute("aria-expanded", "false");
  await editor
    .getByRole("button", { name: "展开阶段：讨论与子流程", exact: true })
    .click();
  await editor
    .getByRole("button", { name: "需求讨论 角色讨论", exact: true })
    .click();
  const goalField = editor.getByRole("group", {
    name: "任务指令",
    exact: true,
  });
  const topicsField = editor.getByRole("group", {
    name: "必须讨论的主题",
    exact: true,
  });
  await expect(
    goalField.getByRole("heading", { name: "讨论目标", level: 2 }),
  ).toBeVisible();
  await expect(goalField.getByRole("textbox")).toHaveCount(0);
  await expect(
    topicsField.getByRole("heading", { name: "场景", level: 2 }),
  ).toBeVisible();
  const creation = page.waitForRequest(
    (request) =>
      request.url().endsWith("/rpc") &&
      request.postDataJSON()?.method === "workflow.draft.create",
  );
  await editor.getByRole("button", { name: "编辑流程", exact: true }).click();
  expect((await creation).postDataJSON().params.payload.base_revision_id).toBe(
    parent.id,
  );
  await expect(
    editor.getByRole("button", { name: "保存草稿", exact: true }),
  ).toBeVisible();
  await expect(editor.getByLabel("步骤名称", { exact: true })).toHaveValue(
    "需求讨论",
  );
  const changedGoal = "## 新讨论目标\n\n产出 **PRD** 与验收说明。";
  await goalField
    .getByRole("button", { name: "任务指令", exact: true })
    .click();
  await goalField
    .getByRole("textbox", { name: "任务指令", exact: true })
    .fill(changedGoal);
  await goalField
    .getByRole("textbox", { name: "任务指令", exact: true }).press("Tab");
  await expect(
    goalField.getByRole("heading", { name: "新讨论目标", level: 2 }),
  ).toBeVisible();
  await topicsField
    .getByRole("button", { name: "讨论主题 1", exact: true })
    .click();
  const changedTopic =
    "## 场景与边界\n\n保留多行 Markdown, 包括逗号。\n\n- 正常\n- 失败";
  await topicsField
    .getByRole("textbox", { name: "讨论主题 1", exact: true })
    .fill(changedTopic);
  await topicsField
    .getByRole("button", { name: "添加讨论主题", exact: true })
    .click();
  await topicsField
    .getByRole("button", { name: "删除讨论主题 3", exact: true })
    .click();
  await editor.getByRole("button", { name: "保存草稿", exact: true }).click();
  await expect(page.getByRole("status")).toHaveText(/草稿已保存/);
  const draftId = new URL(page.url()).searchParams.get("workflow_id");
  const saved = await rpc(page, "query.workflow_draft", { id: draftId });
  expect(saved.data.document.steps[0].task_instructions).toBe(changedGoal);
  expect(saved.data.document.steps[0].interaction.required_topics).toEqual([
    changedTopic,
    topics[1],
  ]);
  expect(
    (await rpc(page, "query.workflow", { id: parent.id })).data.steps[0]
      .task_instructions,
  ).toBe(goal);
  await page.reload();
  await expect(
    goalField.getByRole("heading", { name: "新讨论目标", level: 2 }),
  ).toBeVisible();
  await expect(
    topicsField.getByRole("heading", { name: "场景与边界", level: 2 }),
  ).toBeVisible();
  await expect(
    page.getByText("正在连接本地内核...", { exact: false }),
  ).toHaveCount(0);
  await expect(
    editor.getByLabel("执行 Role", { exact: true }).locator("option:checked"),
  ).toContainText("需求分析师");
  await page.screenshot({
    path: info.outputPath("workflow-markdown-preview.png"),
    fullPage: true,
  });
  await editor.getByRole("button", { name: /调用独立子流程/ }).click();
  await editor
    .getByRole("link", { name: new RegExp(`编辑子流程：${child.name}`) })
    .click();
  await editor
    .getByRole("button", { name: "子流程汇合 汇合", exact: true })
    .click();
  await editor.getByRole("button", { name: "编辑流程", exact: true }).click();
  await expect(editor.getByLabel("步骤名称", { exact: true })).toBeEnabled();
  await editor
    .getByLabel("步骤名称", { exact: true })
    .fill("子流程汇合 · 已修正");
  await editor
    .getByRole("button", { name: "保存可用版本", exact: true })
    .click();
  await expect(page.getByRole("status")).toHaveText(/可用版本已保存/);
  await editor
    .getByRole("button", { name: "子流程汇合 · 已修正 汇合", exact: true })
    .click();
  await expect(editor.getByLabel("步骤名称", { exact: true })).toHaveValue(
    "子流程汇合 · 已修正",
  );
  expect(
    (await rpc(page, "query.workflow", { id: child.id })).data.steps[0].name,
  ).toBe("子流程汇合");
});

test("项目默认 Role Agent 绑定显式保存且 Standard 可留空", async ({ page }) => {
  await setup(page);
  const p = await rpc(page, "project.create", {
    command_id: crypto.randomUUID(),
    payload: { title: "Configuration RPC fixture" },
  });
  let workspace = await rpc(page, "query.workspace");
  expect(workspace.configuration_ready).toBe(false);
  const defaults = await rpc(page, "query.configuration_defaults");
  const config = await rpc(page, "project.configuration.update", {
    project_id: p.id,
    command_id: crypto.randomUUID(),
    target: p.id,
    expected_revision: workspace.active_project.revision,
    payload: {
      workflow_ref: defaults.workflow_ref,
      role_agent_bindings: defaults.role_agent_bindings,
    },
  });
  expect(config.data.schema_version).toBe(2);
  expect(config.data.standard_ref).toBeNull();
  workspace = await rpc(page, "query.workspace");
  expect(workspace.configuration_ready).toBe(true);
  const f = await rpc(page, "feature.create", {
    project_id: p.id,
    command_id: crypto.randomUUID(),
    payload: { project_id: p.id, title: "Frozen configuration" },
  });
  const state = await rpc(page, "query.workspace", {
    project_id: p.id,
    feature_id: f.id,
  });
  const lock = state.configurations.find(
    (r: any) => r.kind === "feature_lock" && r.definition_id === f.id,
  );
  expect(lock.data.compiler_version).toBe(3);
  expect(
    lock.data.specification_snapshot
      .filter((b: any) =>
        [
          "agent.interact",
          "attempt.execute",
          "delivery.review",
          "merge.review",
        ].includes(b.action),
      )
      .every((b: any) => b.task?.renderer_version === "agent-task/v3"),
  ).toBe(true);
  expect(lock.data.role_agent_bindings).toEqual(defaults.role_agent_bindings);
  await page.route("**/rpc", async (route) => {
    if (route.request().postDataJSON().method !== "query.workspace") return route.fallback();
    const response = await route.fetch();
    const value = await response.json();
    if (value.result?.active_project?.id === p.id) value.result.active_project.workspace_definition = {
      repository: "fixture/layout", path: "/tmp/devdesk-layout-fixture", mainline: "main",
    };
    await route.fulfill({response,json:value});
  });
  await page.goto(`/?tab=projects&project_id=${p.id}`);
  const bindings = page.getByRole("group", { name: "流程所需 Role → Agent", exact: true });
  await expect(bindings.getByRole("combobox")).toHaveCount(6);
  await bindings.screenshot({path:"/tmp/devdesk-role-binding-rows.png"});
  const geometry = await bindings.locator(":scope > label").evaluateAll((rows) => rows.map((row) => {
    const text = Array.from(row.childNodes).find((node) => node.nodeType === Node.TEXT_NODE && node.textContent?.trim());
    const range = document.createRange(); range.selectNode(text!);
    const label = range.getBoundingClientRect();
    const select = row.querySelector("select")!.getBoundingClientRect();
    return {labelLeft:label.x, labelRight:label.right, labelCenter:label.y+label.height/2, selectLeft:select.x, selectCenter:select.y+select.height/2, top:select.top, bottom:select.bottom};
  }));
  for (let i = 0; i < geometry.length; i++) {
    expect(Math.abs(geometry[i].labelCenter - geometry[i].selectCenter)).toBeLessThan(8);
    expect(geometry[i].selectLeft).toBeGreaterThan(geometry[i].labelRight);
    if (i) expect(geometry[i].top).toBeGreaterThan(geometry[i-1].bottom);
  }
  await page.unrouteAll({behavior:"wait"});

});

test("配置页面保持 Tab 顺序，当前链接定位且 Tools 调用记录位于历史", async ({
  page,
}, info) => {
  await setup(page);
  const sidebar = page.getByRole("navigation");
  await expect(
    sidebar.getByRole("button", { name: "工作流程", exact: true }),
  ).toHaveCount(0);
  await expect(
    sidebar.getByRole("button", { name: "规范", exact: true }),
  ).toHaveCount(0);
  await expect(
    sidebar.getByRole("button", { name: "工具与技能接入", exact: true }),
  ).toHaveCount(0);
  await page.getByRole("button", { name: "流程与规范", exact: true }).click();
  await expect(
    page.getByRole("tablist", { name: "配置类别" }).getByRole("tab"),
  ).toHaveText(["Workflow", "Delivery", "Spec", "Standard"]);
  await page.getByRole("tab", { name: "Workflow", exact: true }).click();
  await expect(
    page.getByRole("button", { name: "新建 Workflow", exact: true }),
  ).toBeVisible();
  await page.getByRole("button", { name: "基础组件", exact: true }).click();
  await expect(
    page.getByRole("tablist", { name: "配置类别" }).getByRole("tab"),
  ).toHaveText([
    "Model",
    "Harness",
    "MCP",
    "Skills",
    "Tools",
    "Harness原生工具",
  ]);
  await page.getByRole("tab", { name: "Tools", exact: true }).click();
  await expect(
    page.getByRole("region", { name: "HTTP API 接入管理", exact: true }),
  ).toBeVisible();
  await expect(
    page.getByRole("tab", { name: "调用记录", exact: true }),
  ).toHaveCount(0);
  await page.screenshot({
    path: info.outputPath("combined-foundations.png"),
    fullPage: true,
  });
  await page.goto("/?tab=workflows&config_tab=standard");
  await expect(
    page.getByRole("tab", { name: "Standard", exact: true }),
  ).toHaveAttribute("aria-selected", "true");
  await expect(page).toHaveURL(/tab=workflows/);
  await page.goto("/?tab=components&config_tab=mcp");
  await expect(
    page.getByRole("tab", { name: "MCP", exact: true }),
  ).toHaveAttribute("aria-selected", "true");
  await expect(page).toHaveURL(/tab=components/);
  await page.goto("/?tab=history&history_scope=tools");
  await expect(page.getByLabel("历史范围", { exact: true })).toHaveValue(
    "tools",
  );
  await expect(
    page.getByRole("region", { name: "工具调用记录" }),
  ).toBeVisible();
  await expect(page).toHaveURL(/tab=history/);
  await page.reload();
  await expect(page.getByLabel("历史范围", { exact: true })).toHaveValue(
    "tools",
  );
  await page.getByLabel("调用记录归属", { exact: true }).selectOption("global");
  await expect(
    page.getByRole("heading", { name: "Tools调用记录", exact: true }),
  ).toBeVisible();
});

test("Delivery 定义可编辑，流程产出及前序引用保存后保留精确修订", async ({ page }, info) => {
  await setup(page);
  await page.getByRole("button", {name:"流程与规范",exact:true}).click();
  await page.getByRole("tab", {name:"Delivery",exact:true}).click();
  const name = "PRD browser " + crypto.randomUUID().slice(0,6);
  await page.getByLabel("名称",{exact:true}).fill(name);
  await page.getByRole("button",{name:"正文",exact:true}).click();
  await page.getByRole("textbox",{name:"正文",exact:true}).fill("## 目标\n\n描述范围和可观察验收，引用采纳后的文档。");
  await page.getByRole("button",{name:"创建 Delivery",exact:true}).click();
  await expect(page.getByRole("status")).toHaveText(/新修订已保存/);
  const types = await rpc(page,"query.configurations",{kind:"delivery_type",limit:100});
  expect(types.records.map((r:any)=>r.name)).toEqual(expect.arrayContaining(["PRD","Design Doc","Diff","Test Report",name]));
  const delivery = types.records.find((r:any)=>r.name === name);
  const defaults = await rpc(page,"query.configuration_defaults");
  const workflow = defaults.records.find((r:any)=>r.id === defaults.workflow_ref);
  const producer = workflow.data.steps.find((s:any)=>s.key === "requirement_adopt");
  const consumer = workflow.data.steps.find((s:any)=>s.key === "design_discussion");
  await page.goto(`/?tab=workflows&config_tab=workflow&workflow_id=${workflow.id}`);
  await page.getByRole("button",{name:"编辑流程",exact:true}).click();
  const editor = page.getByRole("region",{name:"Workflow 编辑器"});
  await expect(editor.getByRole("button",{name:"保存草稿",exact:true})).toBeEnabled();
  await editor.locator(".workflow-node").filter({hasText:producer.name}).click();
  await editor.getByRole("button",{name:"添加交付物产出",exact:true}).click();
  await editor.getByLabel("产出名称",{exact:true}).fill("prd");
  await editor.getByLabel("Delivery 类型修订",{exact:true}).selectOption(delivery.id);
  await editor.locator(".workflow-node").filter({hasText:consumer.name}).click();
  await editor.getByRole("button",{name:"引用前序交付物",exact:true}).click();
  const source = JSON.stringify({step:producer.key,port:"prd"});
  await editor.getByLabel("前序交付物",{exact:true}).selectOption(source);
  expect(await editor.getByLabel("前置步骤",{exact:true}).evaluate((select: HTMLSelectElement) => Array.from(select.selectedOptions, option => option.value))).toContain(producer.key);
  await editor.getByRole("button",{name:"保存可用版本",exact:true}).click();
  await expect(page.getByRole("status")).toHaveText(/可用版本已保存/);
  await page.reload();
  await editor.locator(".workflow-node").filter({hasText:consumer.name}).click();
  await expect(editor.getByLabel("前序交付物",{exact:true})).toHaveValue(source);
  const catalog = await rpc(page,"query.configurations",{kind:"workflow",include_history:true,limit:100});
  const saved = catalog.records.find((r:any)=>r.data.steps?.some((s:any)=>s.delivery_inputs?.material_1?.delivery_ref === delivery.id));
  expect(saved).toBeTruthy();
  expect(saved.data.steps.find((s:any)=>s.key === producer.key).delivery_outputs.prd).toEqual({delivery_ref:delivery.id,source_port:"document"});
  await page.screenshot({path:info.outputPath("delivery-reference.png"),fullPage:true});
});

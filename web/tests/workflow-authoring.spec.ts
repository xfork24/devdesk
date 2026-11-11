import { test, expect, Page } from "@playwright/test";
import { mkdtempSync } from "node:fs";
import { tmpdir } from "node:os";
import { join } from "node:path";
import { useMockCodexCatalog } from "./support/model-catalog";
useMockCodexCatalog();
async function setup(page: Page) {
  await page.goto("/");
  if (
    (await page.request.get("/capabilities").then((r) => r.json()))
      .setup_required
  ) {
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
    expect(row.data.tools).toContain("builtin-tool-codex-native-v1");
  expect(
    defaults.records.find(
      (r: any) =>
        r.id === defaults.role_agent_bindings[defaults.orchestrator_role_ref],
    ).data.tools,
  ).toContain("builtin-tool-workflow-inspect-v1");
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
  await role.getByRole("button", { name: "职责说明", exact: true }).click();
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
test("原生工具目录提供独立当前入口且不编辑 Schema", async ({ page }) => {
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
  await page
    .getByLabel("模型标识", { exact: true })
    .selectOption("fixture-model");
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
test("Workflow 通用步骤保存发布并恢复草稿，执行确认绑定确切输入", async ({
  page,
}, info) => {
  await setup(page);
  await page.getByRole("button", { name: "流程与规范", exact: true }).click();
  await page.getByRole("button", { name: "新建流程", exact: true }).click();
  const editor = page.getByRole("region", { name: "工作流程管理" });
  const name = "业务确认 " + crypto.randomUUID().slice(0, 6);
  await editor.getByLabel("流程名称", { exact: true }).fill(name);
  await editor.getByRole("button", { name: "添加步骤", exact: true }).click();
  await editor.getByLabel("步骤名称", { exact: true }).fill("确认调查范围");
  await editor.getByLabel("执行方式", { exact: true }).selectOption("control");
  await editor.getByLabel("完成确认", { exact: true }).selectOption("human");
  await expect(editor.getByLabel("标准动作", { exact: true })).toHaveCount(0);
  await editor.getByRole("button", { name: "保存草稿", exact: true }).click();
  await expect(
    editor.getByRole("status").filter({ hasText: /已保存|已发布|校验通过/ }),
  ).toHaveText(/草稿已保存/);
  await editor
    .getByLabel("步骤名称", { exact: true })
    .fill("确认范围 · 本地草稿");
  await page.reload();
  await expect(editor.getByLabel("步骤名称", { exact: true })).toHaveValue(
    "确认范围 · 本地草稿",
  );
  await editor.getByRole("button", { name: "校验流程", exact: true }).click();
  await expect(
    editor.getByRole("status").filter({ hasText: /已保存|已发布|校验通过/ }),
  ).toHaveText(/校验通过/);
  await editor.getByRole("button", { name: "发布流程", exact: true }).click();
  await expect(
    editor.getByRole("status").filter({ hasText: /已保存|已发布|校验通过/ }),
  ).toHaveText(/流程已发布/);
  await page.reload();
  const workflows = await rpc(page, "query.workflows", { limit: 100 });
  const workflow = workflows.records.find(
    (r: any) => r.kind === "workflow" && r.name === name,
  );
  expect(workflow.data.schema_version).toBe(5);
  expect(workflow.data.steps[0].executor.type).toBe("control");
  const project = await rpc(page, "project.create", {
    command_id: crypto.randomUUID(),
    payload: { title: "浏览器执行闭环" },
  });
  const instance = await rpc(page, "workflow.start", {
    project_id: project.id,
    command_id: crypto.randomUUID(),
    target: project.id,
    expected_revision: (
      await rpc(page, "query.workspace", { project_id: project.id })
    ).active_project.revision,
    payload: { workflow_ref: workflow.id, role_agent_bindings: {} },
  });
  await page.goto(`/?tab=execution&project_id=${project.id}`);
  await expect(
    page.getByRole("button", { name: /确认范围 · 本地草稿/ }).first(),
  ).toBeVisible();
  await page
    .getByRole("region", { name: "当前工作流", exact: true })
    .getByRole("button", { name: "执行步骤", exact: true })
    .click();
  await expect
    .poll(async () => {
      const result = await rpc(page, "query.workspace", {
        project_id: project.id,
      });
      return result.configurations.find((r: any) => r.id === instance.id)?.data
        .steps[0].status;
    })
    .toBe("waiting_confirmation");
  const state = await rpc(page, "query.workspace", { project_id: project.id });
  const row = state.configurations.find((r: any) => r.id === instance.id);
  await rpc(page, "workflow.step.confirm", {
    project_id: project.id,
    command_id: crypto.randomUUID(),
    target: row.id,
    expected_revision: row.revision,
    payload: {
      step: row.data.steps[0].step,
      input_digest: row.data.steps[0].input_digest,
      outputs: {},
    },
  });
  await page.reload();
  const execution = page.getByRole("region", {
    name: "当前工作流",
    exact: true,
  });
  await execution
    .getByRole("button", { name: "接纳结果", exact: true })
    .click();
  await execution
    .getByRole("button", { name: "完成流程", exact: true })
    .click();
  await expect
    .poll(
      async () =>
        (
          await rpc(page, "query.workspace", { project_id: project.id })
        ).configurations.find((r: any) => r.id === instance.id)?.data.status,
    )
    .toBe("completed");
  await page.screenshot({
    path: info.outputPath("configured-workflow.png"),
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
    await field.getByRole("button", { name: "正文", exact: true }).click();
    await field.getByRole("textbox", { name: "正文", exact: true }).fill(body);
    await field
      .getByRole("textbox", { name: "正文", exact: true })
      .press("Tab");
    await expect(field.getByRole("textbox")).toHaveCount(0);
    await expect(
      page.getByRole("button", { name: /编辑.*源码|预览正文/ }),
    ).toHaveCount(0);
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
    await field.getByRole("button", { name: "正文", exact: true }).click();
    await expect(
      field.getByRole("textbox", { name: "正文", exact: true }),
    ).toHaveValue(body);
    await page.getByLabel("名称", { exact: true }).click();
    await expect(field.getByRole("textbox")).toHaveCount(0);
    await expect(field.getByRole("table")).toBeVisible();
  }
});

test("阶段 Role 可继承，任务指令与子流程调用是独立配置", async ({ page }) => {
  await setup(page);
  const child = await rpc(page, "workflow.create", {
    command_id: crypto.randomUUID(),
    payload: {
      name: "可复用确认 " + crypto.randomUUID().slice(0, 6),
      document: {
        schema_version: 5,
        stages: [{ key: "work", name: "工作" }],
        steps: [
          {
            key: "done",
            name: "检查输入",
            stage: "work",
            executor: { type: "control" },
          },
        ],
      },
    },
  });
  const published = await rpc(page, "workflow.publish", {
    command_id: crypto.randomUUID(),
    target: child.id,
    expected_revision: child.revision,
    payload: {},
  });
  await page.getByRole("button", { name: "流程与规范", exact: true }).click();
  await page.getByRole("button", { name: "新建流程", exact: true }).click();
  const editor = page.getByRole("region", { name: "工作流程管理" });
  const name = "继承与组合 " + crypto.randomUUID().slice(0, 6);
  await editor.getByLabel("流程名称", { exact: true }).fill(name);
  await editor.getByRole("button", { name: "工作", exact: true }).click();
  await editor
    .getByLabel("阶段默认 Role", { exact: true })
    .selectOption("builtin-role-analyst");
  await editor.getByRole("button", { name: "添加步骤", exact: true }).click();
  await editor.getByLabel("步骤名称", { exact: true }).fill("调查上下文");
  await editor
    .getByRole("group", { name: "任务指令", exact: true })
    .getByRole("button", { name: "任务指令", exact: true })
    .click();
  await editor
    .getByRole("textbox", { name: "任务指令", exact: true })
    .fill("## 目标\n\n按所加载的 Skill 调查给定材料。");
  await editor.getByLabel("步骤名称", { exact: true }).click();
  await editor
    .getByRole("button", { name: "折叠阶段：工作", exact: true })
    .click();
  await expect(editor.getByRole("button", { name: /^调查上下文/ })).toHaveCount(
    0,
  );
  await editor
    .getByRole("button", { name: "展开阶段：工作", exact: true })
    .click();
  await editor.getByRole("button", { name: /^调查上下文/ }).click();
  await editor
    .getByRole("button", { name: "添加子流程调用", exact: true })
    .click();
  await editor
    .getByLabel("调用子流程", { exact: true })
    .selectOption(published.id);
  await expect(editor.getByLabel("任务指令", { exact: true })).toHaveCount(0);
  await editor.getByRole("button", { name: "发布流程", exact: true }).click();
  await expect(
    editor.getByRole("status").filter({ hasText: /已保存|已发布|校验通过/ }),
  ).toHaveText(/流程已发布/);
  const records = await rpc(page, "query.workflows", { limit: 100 });
  const workflow = records.records.find(
    (r: any) => r.kind === "workflow" && r.name === name,
  );
  const preview = await rpc(page, "query.workflow_task_preview", {
    id: workflow.id,
    step: workflow.data.steps[0].key,
  });
  expect(preview.task.task_instructions).toContain("按所加载的 Skill");
  expect(workflow.data.stages[0].default_role_ref).toBe("builtin-role-analyst");
  expect(workflow.data.steps[1].executor.workflow_ref).toBe(published.id);
  await page.reload();
  await expect(editor.getByLabel("流程名称", { exact: true })).toHaveValue(
    name,
  );
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
  expect(config.data.schema_version).toBe(3);
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
    (r: any) =>
      r.kind === "feature_workflow_configuration" && r.definition_id === f.id,
  );
  expect(lock.data.schema_version).toBe(3);
  expect(lock.data.configuration.role_agent_bindings).toEqual(
    defaults.role_agent_bindings,
  );
  const instance = state.configurations.find(
    (r: any) => r.id === lock.data.workflow_instance_id,
  );
  expect(instance.data.schema_version).toBe(5);
  expect(
    instance.data.plan.document.steps.every(
      (s: any) => s.executor && !s.action,
    ),
  ).toBe(true);
  await page.route("**/rpc", async (route) => {
    if (route.request().postDataJSON().method !== "query.workspace")
      return route.fallback();
    const response = await route.fetch();
    const value = await response.json();
    if (value.result?.active_project?.id === p.id)
      value.result.active_project.workspace_definition = {
        repository: "fixture/layout",
        path: "/tmp/devdesk-layout-fixture",
        mainline: "main",
      };
    await route.fulfill({ response, json: value });
  });
  await page.goto(`/?tab=projects&project_id=${p.id}`);
  const bindings = page.getByRole("group", {
    name: "流程所需 Role → Agent",
    exact: true,
  });
  await expect(bindings.getByRole("combobox")).toHaveCount(6);
  await bindings.screenshot({ path: "/tmp/devdesk-role-binding-rows.png" });
  const geometry = await bindings
    .locator(":scope > label")
    .evaluateAll((rows) =>
      rows.map((row) => {
        const text = Array.from(row.childNodes).find(
          (node) =>
            node.nodeType === Node.TEXT_NODE && node.textContent?.trim(),
        );
        const range = document.createRange();
        range.selectNode(text!);
        const label = range.getBoundingClientRect();
        const select = row.querySelector("select")!.getBoundingClientRect();
        return {
          labelLeft: label.x,
          labelRight: label.right,
          labelCenter: label.y + label.height / 2,
          selectLeft: select.x,
          selectCenter: select.y + select.height / 2,
          top: select.top,
          bottom: select.bottom,
        };
      }),
    );
  for (let i = 0; i < geometry.length; i++) {
    expect(
      Math.abs(geometry[i].labelCenter - geometry[i].selectCenter),
    ).toBeLessThan(8);
    expect(geometry[i].selectLeft).toBeGreaterThan(geometry[i].labelRight);
    if (i) expect(geometry[i].top).toBeGreaterThan(geometry[i - 1].bottom);
  }
  await page.unrouteAll({ behavior: "wait" });
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
    page.getByRole("button", { name: "新建流程", exact: true }),
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
    "project",
  );
  await expect(
    page.getByLabel("历史范围", { exact: true }).locator("option"),
  ).toHaveText(["当前项目", "Global"]);
  await expect(page.getByLabel("调用记录归属", { exact: true })).toHaveCount(0);
  await page.getByLabel("历史范围", { exact: true }).selectOption("global");
  await expect(page.getByRole("region", { name: "历史记录" })).toBeVisible();
  await expect(page).toHaveURL(/tab=history/);
  await page.reload();
  await expect(page.getByLabel("历史范围", { exact: true })).toHaveValue(
    "global",
  );
  await expect(
    page
      .getByRole("region", { name: "历史记录", exact: true })
      .getByRole("heading", { name: "历史记录", exact: true }),
  ).toBeVisible();
});

test("Delivery 定义和输入输出端口保存后保留精确修订", async ({
  page,
}, info) => {
  await setup(page);
  await page.getByRole("button", { name: "流程与规范", exact: true }).click();
  await page.getByRole("tab", { name: "Delivery", exact: true }).click();
  const name = "风险材料 " + crypto.randomUUID().slice(0, 6);
  await page.getByLabel("名称", { exact: true }).fill(name);
  await page.getByRole("button", { name: "正文", exact: true }).click();
  await page
    .getByRole("textbox", { name: "正文", exact: true })
    .fill("风险材料和真实来源。");
  await page
    .getByRole("button", { name: "创建 Delivery", exact: true })
    .click();
  await expect(page.getByRole("status")).toHaveText(/新修订已保存/);
  const types = await rpc(page, "query.configurations", {
    kind: "delivery_type",
    limit: 100,
  });
  const delivery = types.records.find((r: any) => r.name === name);
  await page.getByRole("tab", { name: "Workflow", exact: true }).click();
  await page.getByRole("button", { name: "新建流程", exact: true }).click();
  const editor = page.getByRole("region", { name: "工作流程管理" });
  await editor.getByLabel("流程名称", { exact: true }).fill(name + " 流程");
  await editor.getByRole("button", { name: "添加步骤", exact: true }).click();
  await editor.getByLabel("步骤名称", { exact: true }).fill("产生材料");
  await editor
    .getByLabel("执行 Role（留空继承阶段）", { exact: true })
    .selectOption("builtin-role-analyst");
  await editor
    .getByRole("group", { name: "任务指令", exact: true })
    .getByRole("button", { name: "任务指令", exact: true })
    .click();
  await editor
    .getByRole("textbox", { name: "任务指令", exact: true })
    .fill("提交输入材料中的风险。");
  await editor.getByLabel("步骤名称", { exact: true }).click();
  await editor.getByRole("button", { name: "添加输出", exact: true }).click();
  await editor.getByLabel("输出名称", { exact: true }).fill("risks");
  await editor.getByLabel("步骤名称", { exact: true }).click();
  await editor
    .getByLabel("交付类型 · risks", { exact: true })
    .selectOption(delivery.id);
  await editor.getByRole("button", { name: "添加步骤", exact: true }).click();
  await editor.getByLabel("步骤名称", { exact: true }).fill("确认材料");
  await editor.getByLabel("执行方式", { exact: true }).selectOption("control");
  await editor.getByRole("button", { name: "添加输入", exact: true }).click();
  const option = await editor
    .getByLabel("材料来源 · input_1", { exact: true })
    .locator("option")
    .last()
    .getAttribute("value");
  await editor
    .getByLabel("材料来源 · input_1", { exact: true })
    .selectOption(option!);
  await editor.getByRole("button", { name: "发布流程", exact: true }).click();
  await expect(
    editor.getByRole("status").filter({ hasText: /已保存|已发布|校验通过/ }),
  ).toHaveText(/流程已发布/);
  const catalog = await rpc(page, "query.workflows", { limit: 100 });
  const saved = catalog.records.find(
    (r: any) => r.kind === "workflow" && r.name === name + " 流程",
  );
  const [producer, consumer] = saved.data.steps;
  expect(producer.outputs.risks.delivery_ref).toBe(delivery.id);
  expect(consumer.inputs.input_1.source).toEqual({
    step: producer.key,
    port: "risks",
  });
  expect(consumer.dependencies).toContain(producer.key);
  await page.reload();
  await editor.getByRole("button", { name: /^确认材料/ }).click();
  await expect(
    editor.getByLabel("材料来源 · input_1", { exact: true }),
  ).toHaveValue(`${producer.key}:risks`);
  await page.screenshot({
    path: info.outputPath("delivery-reference.png"),
    fullPage: true,
  });
});

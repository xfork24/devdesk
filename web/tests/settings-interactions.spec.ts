import { test, expect, type Page } from "@playwright/test";
import { mkdtempSync } from "node:fs";
import { tmpdir } from "node:os";
import { join } from "node:path";

async function setup(page: Page) {
  await page.goto("/");
  if (
    (
      await page.request
        .get("/capabilities")
        .then((response) => response.json())
    ).setup_required
  ) {
    await page
      .getByLabel("数据目录")
      .fill(mkdtempSync(join(tmpdir(), "devdesk-settings-interactions-")));
    await page.getByRole("button", { name: "初始化 / 打开数据集" }).click();
  }
}

test("MiniMax 默认选择、行左侧反馈、自动模型目录与原生工具导航", async ({
  page,
}, info) => {
  await setup(page);
  const requested: string[] = [];
  let failZai = true;
  await page.route("**/rpc", async (route) => {
    const call = route.request().postDataJSON();
    let result: any;
    if (call.method === "settings.update")
      return route.fulfill({
        json: {
          jsonrpc: "2.0",
          id: call.id,
          error: { code: -32000, message: "Fixture failure" },
        },
      });
    if (call.method === "credentials.status")
      result = {
        configured: true,
        available: true,
        storage: "fixture",
        auth_kind: call.params.model_provider === "ollama" ? "none" : "api_key",
      };
    else if (call.method === "codex.auth.models") {
      requested.push("openai");
      result = {
        models: [{ model: "gpt-6.1-sol", displayName: "GPT-6.1 Sol" }],
      };
    } else if (call.method === "models.list") {
      const provider = call.params.model_provider;
      requested.push(provider);
      if (provider === "zai-coding-cn" && failZai) {
        failZai = false;
        return route.fulfill({
          json: {
            jsonrpc: "2.0",
            id: call.id,
            error: { code: -32000, message: "Fixture catalog unavailable" },
          },
        });
      }
      result = {
        source: "pinned_catalog",
        models: [
          {
            id: provider === "minimax-cn" ? "MiniMax-M2.7" : "glm-5.3",
            context_window: 200000,
            max_output_tokens: 8192,
          },
        ],
      };
    } else return route.fallback();
    return route.fulfill({ json: { jsonrpc: "2.0", id: call.id, result } });
  });
  await page.getByRole("button", { name: "设置", exact: true }).click();
  await expect(page.getByLabel("DSH 模型服务商", { exact: true })).toHaveValue(
    "minimax-cn",
  );
  const defaultHarness = page.getByLabel("默认 Agent Harness", { exact: true });
  await defaultHarness.selectOption("dsh");
  const feedback = page.getByText("默认 Harness 保存失败", { exact: true });
  await expect(feedback).toBeVisible();
  const row = defaultHarness.locator("..");
  const left = await row.evaluate(
    (element) =>
      element.getBoundingClientRect().left +
      parseFloat(getComputedStyle(element).paddingLeft),
  );
  expect(Math.abs((await feedback.boundingBox())!.x - left)).toBeLessThan(1);
  expect(
    await feedback.evaluate(
      (element) => getComputedStyle(element).backgroundColor,
    ),
  ).toBe("rgba(0, 0, 0, 0)");

  await page.getByRole("button", { name: "基础组件", exact: true }).click();
  const backend = page.getByLabel("模型后端", { exact: true });
  const model = page.getByRole("combobox", { name: "模型标识", exact: true });
  await expect(model.locator('option[value="gpt-6.1-sol"]')).toHaveCount(1);
  await expect(
    page.getByRole("textbox", { name: "模型标识", exact: true }),
  ).toHaveCount(0);
  await model.selectOption("gpt-6.1-sol");
  await backend.selectOption("minimax-cn");
  await expect(model).toHaveValue("");
  await expect(model.locator('option[value="MiniMax-M2.7"]')).toHaveCount(1);
  await model.selectOption("MiniMax-M2.7");
  await expect(page.getByLabel("上下文容量", { exact: true })).toHaveValue(
    "200000",
  );
  await expect(page.getByLabel("最大输出容量", { exact: true })).toHaveValue(
    "8192",
  );
  await backend.selectOption("zai-coding-cn");
  await expect(
    page.getByRole("alert").filter({ hasText: "Fixture catalog unavailable" }),
  ).toBeVisible();
  await page.getByRole("button", { name: "刷新模型目录", exact: true }).click();
  await expect(model.locator('option[value="glm-5.3"]')).toHaveCount(1);
  expect(requested).toContain("openai");
  expect(requested).toContain("minimax-cn");
  expect(
    requested.filter((provider) => provider === "zai-coding-cn"),
  ).toHaveLength(2);

  await page.getByRole("tab", { name: "Harness", exact: true }).click();
  await page.getByLabel("运行时后端", { exact: true }).selectOption("dsh");
  const providerRows = page
    .getByRole("group", { name: "支持的模型服务商", exact: true })
    .locator("label");
  await expect(providerRows).toHaveCount(4);
  for (const provider of await providerRows.all()) {
    const checkbox = (await provider.getByRole("checkbox").boundingBox())!;
    const description = (await provider.locator("span").boundingBox())!;
    expect(checkbox.x + checkbox.width).toBeLessThan(description.x);
    expect(
      Math.abs(
        checkbox.y +
          checkbox.height / 2 -
          description.y -
          description.height / 2,
      ),
    ).toBeLessThan(2);
  }
  await page.screenshot({
    path: info.outputPath("harness-provider-rows.png"),
    fullPage: true,
  });
  await page.getByRole("tab", { name: "Harness原生工具", exact: true }).click();
  const native = page.getByRole("region", {
    name: "原生工具目录",
    exact: true,
  });
  await expect(
    native.getByRole("heading", { name: "适用 Harness", exact: true }),
  ).toHaveCount(0);
  const dsh = native
    .locator(".native-tool-group")
    .filter({
      has: page.getByRole("heading", { name: "DSH 原生工具组", exact: true }),
    });
  await expect(dsh).toContainText("适用 Harness 版本：0.2.2-rc.1");
  await dsh.click();
  await expect(page.getByLabel("运行时后端", { exact: true })).toHaveValue(
    "dsh",
  );
  await expect(page.getByLabel("固定运行版本", { exact: true })).toHaveValue(
    "0.2.2-rc.1",
  );
  await page.unrouteAll({ behavior: "wait" });
});

test("Tools 调用记录采用事件行并在行内展开详情与审批", async ({
  page,
}, info) => {
  await setup(page);
  const calls = [
    {
      id: "call-success",
      name: "读取构建报告",
      status: "succeeded",
      text: "Fixture build report",
    },
    {
      id: "call-approval",
      name: "更新工单",
      status: "awaiting_approval",
      text: "Fixture pending update",
    },
  ].map((call, index) => ({
    id: call.id,
    name: call.name,
    revision: 1,
    created_at: Date.now() - index * 1000,
    data: {
      status: call.status,
      remote_effect: call.status === "succeeded" ? "applied" : "unknown",
      invocation: {
        tool: { revision_id: "fixture-tool", binding_ref: "fixture-binding" },
        agent_instance_id: "fixture-agent",
        arguments: { report: "build" },
      },
      receipt: {
        status: call.status,
        content: [{ type: "text", text: call.text }],
      },
    },
  }));
  let approved = false;
  await page.route("**/rpc", async (route) => {
    const call = route.request().postDataJSON();
    let result: any;
    if (call.method === "query.tool_calls")
      result = { records: calls, total: calls.length };
    else if (call.method === "tool.call.approve") {
      approved = call.params.payload.approve;
      calls[1].data.status = "succeeded";
      calls[1].data.receipt.status = "succeeded";
      result = { id: calls[1].id };
    } else return route.fallback();
    return route.fulfill({ json: { jsonrpc: "2.0", id: call.id, result } });
  });
  await page.getByRole("button", { name: "历史记录", exact: true }).click();
  await page.getByLabel("历史范围", { exact: true }).selectOption("tools");
  const history = page.getByRole("region", {
    name: "工具调用记录",
    exact: true,
  });
  await expect(history.locator(".history-row")).toHaveCount(2);
  await expect(history.getByRole("textbox")).toHaveCount(0);
  await expect(history).toContainText("按时间倒序 · 每页 50 条");
  await expect(history.locator(".pagination")).toContainText("第 1 页");
  await expect(history.locator(".configuration-layout")).toHaveCount(0);
  await expect(
    history.getByText("选择调用查看参数、权限、来源与结果。", { exact: true }),
  ).toHaveCount(0);
  await expect(history.getByRole("article", { name: "调用详情" })).toHaveCount(
    0,
  );
  const report = history.getByRole("button", {
    name: "读取构建报告",
    exact: true,
  });
  await report.click();
  await expect(report).toHaveAttribute("aria-expanded", "true");
  const detail = history.getByRole("article", { name: "调用详情" });
  await expect(detail).toContainText("Fixture build report");
  await expect(detail.locator("..")).toHaveClass("tool-call-history-item");
  await page.screenshot({
    path: info.outputPath("tool-call-event-history.png"),
    fullPage: true,
  });
  await report.click();
  await expect(detail).toHaveCount(0);
  await history.getByRole("button", { name: "更新工单", exact: true }).click();
  await history
    .getByRole("button", { name: "批准此调用", exact: true })
    .click();
  expect(approved).toBe(true);
  await expect(
    history.getByRole("article", { name: "调用详情" }),
  ).toContainText("succeeded");
  await page.unrouteAll({ behavior: "wait" });
});

test("角色页仅保留 Agent 定义，旧入口兼容且可在讨论内创建身份", async ({ page }) => {
  await setup(page);
  await page.goto("/?tab=actors&config_tab=agent_instance");
  await expect(page.getByRole("tab", { name: "Agent", exact: true })).toHaveAttribute("aria-selected", "true");
  await expect(page.getByRole("tab", { name: "Agent 定义", exact: true })).toHaveCount(0);
  await expect(page.getByRole("tablist").getByRole("tab")).toHaveCount(3);
  await expect(page).toHaveURL(/config_tab=agent(?:&|$)/);
  await page.getByRole("button", { name: "角色讨论", exact: true }).click();
  await page.getByLabel("讨论范围", { exact: true }).selectOption("global");
  const role = page.getByLabel("讨论角色", { exact: true });
  await expect(role).not.toHaveValue("");
  const agentId = await role.inputValue();
  const created = page.waitForResponse((response) => response.url().endsWith("/rpc") && response.request().postDataJSON()?.method === "agent.instance.create");
  await page.getByRole("button", { name: "新建讨论身份", exact: true }).click();
  const response = await created;
  const result = await response.json();
  expect(result.error).toBeUndefined();
  expect(response.request().postDataJSON().params.payload.agent_revision_id).toBe(agentId);
  expect(response.request().postDataJSON().params.payload.scope).toEqual({ kind: "global" });
  await expect(page.getByLabel("Agent", { exact: true })).toHaveValue(result.result.id);
  await page.goto(`/?tab=actors&config_tab=agent_instance&config_object=${result.result.id}`);
  await expect(page.getByRole("tab", { name: "Agent", exact: true })).toHaveAttribute("aria-selected", "true");
  await expect(page.getByLabel("名称", { exact: true })).toHaveValue(/Sol High/);
});


test("清理项目后旧链接返回可用工作台", async ({page}) => {
  await setup(page);
  await page.route("**/rpc", async (route) => {
    const call = route.request().postDataJSON();
    if (call.method !== "project.open_github" || call.params.target !== "deleted-project") return route.fallback();
    await route.fulfill({json:{jsonrpc:"2.0",id:call.id,error:{code:-32000,message:"项目不存在",data:{code:"not_found"}}}});
  });
  await page.goto("/?tab=projects&project_id=deleted-project&feature_id=deleted-feature&task_id=deleted-task");
  await expect(page.getByRole("heading", {name:"项目管理",exact:true}).first()).toBeVisible();
  await expect(page).not.toHaveURL(/deleted-project|deleted-feature|deleted-task/);
  await expect(page.getByText(/not_found · 项目不存在/)).toHaveCount(0);
});

test("MCP 和 Tools 在同一列表内新建与编辑，Skills 导入位于编辑区", async ({ page }, info) => {
  const errors: string[] = [];
  page.on("pageerror", error => errors.push(error.message));
  await setup(page);
  for (const [tab, kind, field] of [
    ["mcp", "MCP", "MCP URL"],
    ["http", "HTTP API", "API Base URL"],
  ]) {
    await page.goto(`/?tab=components&config_tab=${tab}`);
    const manager = page.getByRole("region", { name: `${kind} 接入管理`, exact: true });
    const list = manager.locator(".configuration-list");
    await expect(page.getByRole("region", { name: `${kind} 新建接入`, exact: true })).toHaveCount(0);
    await list.getByRole("button", { name: `新建 ${kind} 接入源`, exact: true }).click();
    const name = `${kind} unified ${crypto.randomUUID().slice(0, 6)}`;
    await manager.getByLabel("接入名称", { exact: true }).fill(name);
    await manager.getByLabel(field, { exact: true }).fill("https://example.test/api");
    await manager.getByRole("button", { name: "创建接入源", exact: true }).click();
    await expect(manager.getByRole("status")).toContainText("接入定义已保存");
    const item = list.getByRole("button", { name: new RegExp(name) });
    await expect(item).toHaveClass("selected");
    const itemBox = (await item.boundingBox())!;
    const formBox = (await manager.getByLabel("接入名称", { exact: true }).boundingBox())!;
    expect(itemBox.x + itemBox.width).toBeLessThan(formBox.x);
    await manager.getByLabel("接入名称", { exact: true }).fill(`${name} revised`);
    await manager.getByRole("button", { name: "保存接入新修订", exact: true }).click();
    await expect(manager.getByRole("status")).toContainText("接入定义已保存");
    await page.reload();
    await expect(manager.getByLabel("接入名称", { exact: true })).toHaveValue(`${name} revised`);
    await list.getByRole("button", { name: `新建 ${kind} 接入源`, exact: true }).click();
    await expect(manager.getByLabel("接入名称", { exact: true })).toHaveValue("");
    await expect(manager.getByRole("button", { name: "创建接入源", exact: true })).toBeVisible();
    await expect(page).not.toHaveURL(/config_object=/);
    await list.getByRole("button", { name: new RegExp(`${name} revised`) }).click();
    await expect(manager.getByLabel("接入名称", { exact: true })).toHaveValue(`${name} revised`);
    await page.screenshot({ path: info.outputPath(`${tab}-unified-editor.png`), fullPage: true });
  }
  await page.goto("/?tab=components&config_tab=skills");
  const skills = page.getByRole("region", { name: "Skill 管理", exact: true });
  const list = skills.locator(".configuration-list");
  await expect(page.getByRole("region", { name: "Skill 包导入", exact: true })).toHaveCount(0);
  await list.getByRole("button", { name: "导入 Skill 包", exact: true }).click();
  const importer = skills.getByRole("region", { name: "Skill 包导入", exact: true });
  await expect(importer).toBeVisible();
  const listBox = (await list.boundingBox())!;
  const importerBox = (await importer.boundingBox())!;
  expect(listBox.x + listBox.width).toBeLessThan(importerBox.x);
  await page.screenshot({ path: info.outputPath("skills-unified-editor.png"), fullPage: true });
  await list.getByRole("button", { name: "新建 Skill", exact: true }).click();
  await expect(importer).toHaveCount(0);
  await expect(skills.getByRole("button", { name: "创建 Skill", exact: true })).toBeVisible();
  expect(errors).toEqual([]);
});

import { test, expect, Page } from "@playwright/test";
import { mkdtempSync, readFileSync } from "node:fs";
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
      .fill(mkdtempSync(join(tmpdir(), "devdesk-integrations-web-")));
    await page.getByRole("button", { name: "初始化 / 打开数据集" }).click();
  }
  await expect(
    page.getByRole("button", { name: "基础组件", exact: true }),
  ).toBeVisible();
}
async function rpc(page: Page, method: string, params: any = {}) {
  const response = await page.request.post("/rpc", {
    data: { jsonrpc: "2.0", id: 1, method, params },
  });
  const result = await response.json();
  expect(result.error, JSON.stringify(result)).toBeUndefined();
  return result.result;
}
async function markdown(page: any, label: string, value: string) {
  const field = page.getByRole("group", { name: label, exact: true });
  await field.getByRole("button", { name: label, exact: true }).click();
  await field.getByRole("textbox", { name: label, exact: true }).fill(value);
  await field.getByRole("textbox", { name: label, exact: true }).press("Tab");
}
test("HTTP 接入发布动作、绑定确切授权与历史修订，原入口只保留 Model/Harness", async ({
  page,
}, info) => {
  await setup(page);
  await page.getByRole("button", { name: "基础组件", exact: true }).click();
  await page.getByRole("tab", { name: "Tools", exact: true }).click();
  const source = page.getByRole("region", { name: "HTTP API 接入管理" });
  const creator = page.getByRole("region", {
    name: "HTTP API 接入管理",
    exact: true,
  });
  const name = "Browser API " + crypto.randomUUID().slice(0, 6);
  await creator.getByLabel("接入名称", { exact: true }).fill(name);
  await creator.getByLabel("API Base URL").fill("https://example.test/v1");
  await markdown(
    creator,
    "接入说明",
    "## Ticket API\n\nRead **scoped** tickets.",
  );
  await creator
    .getByRole("button", { name: "创建接入源", exact: true })
    .click();
  await expect(creator.getByRole("status")).toContainText("接入定义已保存");
  const tool = page.getByRole("region", { name: "HTTP 动作管理" });
  const alias = "ticket_" + crypto.randomUUID().slice(0, 6);
  await tool.getByLabel("工具调用名称", { exact: true }).fill(alias);
  await markdown(tool, "工具说明", "Lookup one ticket.");
  await tool.getByLabel("相对 path", { exact: true }).fill("/tickets/{id}");
  await tool.getByLabel("调用参数 JSON Schema", { exact: true }).fill(
    JSON.stringify({
      type: "object",
      properties: { id: { type: "string" } },
      required: ["id"],
      additionalProperties: false,
    }),
  );
  await tool.getByRole("button", { name: "添加参数映射" }).click();
  await tool.getByLabel("参数名 1", { exact: true }).fill("id");
  await tool.getByLabel("输入 JSON Pointer 1", { exact: true }).fill("/id");
  await tool
    .getByRole("combobox", { name: "参数位置 1", exact: true })
    .selectOption("path");
  await tool
    .getByRole("button", { name: "发布 HTTP 动作", exact: true })
    .click();
  await expect(tool.getByRole("status")).toContainText("HTTP 工具已发布");
  const catalog = await rpc(page, "query.configurations", {
    kind: "tool",
    include_history: true,
  });
  const published = catalog.records.find(
    (r: any) => r.data.qualified_name === alias,
  );
  expect(published.data).not.toHaveProperty("output_schema");
  const binding = page.getByRole("region", { name: "接入绑定管理" });
  await binding.getByLabel("绑定名称", { exact: true }).fill("Account " + name);
  await binding
    .getByLabel("绑定允许的确切工具修订", { exact: true })
    .selectOption(published.id);
  await binding
    .getByRole("button", { name: "创建接入绑定", exact: true })
    .click();
  await expect(binding.getByRole("status")).toContainText("绑定已保存");
  await source.getByLabel("接入名称", { exact: true }).fill(name + " v2");
  await source.getByRole("button", { name: "保存接入新修订" }).click();
  await expect(source.getByRole("status")).toContainText("接入定义已保存");
  await page.reload();
  await expect(source.getByLabel("接入名称", { exact: true })).toHaveValue(
    name + " v2",
  );
  await source
    .getByRole("combobox", { name: "接入历史修订", exact: true })
    .selectOption({ label: "v1 · " + name });
  await expect(
    source.getByRole("heading", { name: "Ticket API", exact: true }),
  ).toBeVisible();
  await page.screenshot({
    path: info.outputPath("integrations-http.png"),
    fullPage: true,
  });
  await page.getByRole("button", { name: "基础组件", exact: true }).click();
  await expect(
    page.getByRole("tab", { name: "Model", exact: true }),
  ).toBeVisible();
  await expect(
    page.getByRole("tab", { name: "Tool", exact: true }),
  ).toHaveCount(0);
  await expect(
    page.getByRole("tab", { name: "Skill", exact: true }),
  ).toHaveCount(0);
});
test("Skills 导入保留 Markdown 原文与工具依赖，当前链接定位同一资源", async ({
  page,
}) => {
  await setup(page);
  await page.getByRole("button", { name: "基础组件", exact: true }).click();
  await page.getByRole("tab", { name: "Skills", exact: true }).click();
  await page
    .getByRole("button", { name: "导入 Skill 包", exact: true })
    .click();
  const importer = page.getByRole("region", { name: "Skill 包导入" });
  const name = "Browser Skill " + crypto.randomUUID().slice(0, 6);
  const body =
    "---\nname: scoped-review\ndescription: Preserve source, scope and evidence.\n---\n\n# Review, evidence\n\nUse [reference](references/check.md).\n\n```text\nNo installation\n```";
  await importer.getByLabel("导入 Skill 名称", { exact: true }).fill(name);
  await markdown(importer, "Skill 内容", body);
  await importer
    .getByLabel("Skill 必需工具", { exact: true })
    .selectOption("builtin-tool-codex-native:1");
  await page.getByRole("button", { name: "新建 Skill", exact: true }).click();
  await page
    .getByRole("button", { name: "导入 Skill 包", exact: true })
    .click();
  await expect(
    importer.getByLabel("导入 Skill 名称", { exact: true }),
  ).toHaveValue(name);
  await expect(
    importer.getByRole("group", { name: "Skill 内容", exact: true }),
  ).toContainText("Review, evidence");
  await importer
    .getByRole("button", { name: "导入 Skill 包", exact: true })
    .click();
  await expect(importer.getByRole("status")).toContainText("Skill 已导入");
  await expect(
    page.getByRole("button", { name: new RegExp(name + " v1") }),
  ).toBeVisible();
  const catalog = await rpc(page, "query.configurations", {
    kind: "skill",
    include_history: true,
  });
  const skill = catalog.records.find((r: any) => r.name === name);
  const pkg = await rpc(page, "skill.export", { id: skill.id });
  expect(pkg.body).toBe(body);
  expect(pkg.requires_tools).toEqual(["builtin-tool-codex-native:1"]);
  await page.goto(
    `/?tab=components&config_tab=skills&config_object=${skill.id}`,
  );
  await expect(page.getByLabel("名称", { exact: true })).toHaveValue(name);
  await expect(
    page
      .getByRole("group", { name: "正文", exact: true })
      .getByRole("heading", { name: "Review, evidence" }),
  ).toBeVisible();
});

test("复制接入源后刷新保留新建草稿及原对象", async ({ page }) => {
  await setup(page);
  const name = `Copy source ${crypto.randomUUID().slice(0, 6)}`;
  const original = await rpc(page, "integration.create", {
    command_id: crypto.randomUUID(),
    payload: {
      name,
      data: {
        source: { kind: "http_api", base_url: "https://example.test/v1" },
      },
    },
  });
  await page.goto(
    `/?tab=components&config_tab=http&config_object=${original.id}`,
  );
  const manager = page.getByRole("region", {
    name: "HTTP API 接入管理",
    exact: true,
  });
  await expect(manager.getByLabel("接入名称", { exact: true })).toHaveValue(
    name,
  );
  await manager
    .getByRole("button", { name: "复制接入源", exact: true })
    .click();
  await expect(manager.getByLabel("接入名称", { exact: true })).toHaveValue(
    `${name} · 副本`,
  );
  await expect(page).not.toHaveURL(/config_object=/);
  await page.reload();
  await expect(manager.getByLabel("接入名称", { exact: true })).toHaveValue(
    `${name} · 副本`,
  );
  await manager
    .getByRole("button", { name: "创建接入源", exact: true })
    .click();
  await expect(manager.getByRole("status")).toContainText("接入定义已保存");
  const sources = (
    await rpc(page, "query.integrations", { include_history: true })
  ).records;
  expect(sources.find((source: any) => source.id === original.id).name).toBe(
    name,
  );
  const copied = sources.find(
    (source: any) => source.name === `${name} · 副本`,
  );
  expect(copied.definition_id).not.toBe(original.definition_id);
  await rpc(page, "integration.delete", {
    command_id: crypto.randomUUID(),
    target: copied.id,
    expected_revision: copied.revision,
    payload: { reason: "review fixture deletion" },
  });
  await page.reload();
  await expect(manager.getByRole("alert")).toContainText("原接入源不可用");
  await expect(manager.getByLabel("接入名称", { exact: true })).toBeDisabled();
  await manager
    .getByRole("button", { name: "新建 HTTP API 接入源", exact: true })
    .click();
  await expect(manager.getByLabel("接入名称", { exact: true })).toBeEnabled();
  await expect(manager.getByLabel("接入名称", { exact: true })).toHaveValue("");
});

test("接入源加载完成前不能把已有草稿作为新建提交", async ({ page }) => {
  await setup(page);
  const name = `Loading source ${crypto.randomUUID().slice(0, 6)}`;
  const original = await rpc(page, "integration.create", {
    command_id: crypto.randomUUID(),
    payload: {
      name,
      data: {
        source: {
          kind: "mcp",
          transport: "streamable_http",
          url: "https://example.test/mcp",
          command: [],
          protocol_version: "2026-07-28",
        },
      },
    },
  });
  await page.goto(
    `/?tab=components&config_tab=mcp&config_object=${original.id}`,
  );
  const manager = page.getByRole("region", {
    name: "MCP 接入管理",
    exact: true,
  });
  await expect(manager.getByLabel("接入名称", { exact: true })).toHaveValue(
    name,
  );
  let release!: () => void;
  const loaded = new Promise<void>((resolve) => {
    release = resolve;
  });
  await page.route("**/rpc", async (route) => {
    if (route.request().postDataJSON().method === "query.integrations")
      await loaded;
    await route.fallback();
  });
  try {
    await page.reload();
    await expect(
      manager.getByLabel("接入名称", { exact: true }),
    ).toBeDisabled();
  } finally {
    release();
  }
  await expect(manager.getByLabel("接入名称", { exact: true })).toBeEnabled();
  await expect(
    manager.getByRole("button", { name: "保存接入新修订", exact: true }),
  ).toBeVisible();
  await page.unrouteAll({ behavior: "wait" });
});

test("MCP 页面实际发现 stdio 服务、只授权选中工具并查看 Docker 调用回执", async ({
  page,
}, info) => {
  await setup(page);
  const resources = await rpc(page, "query.permission_resources");
  const bindings = resources.targets
    .filter((r: any) =>
      [
        "builtin-harness-codex-managed:1",
        "builtin-sandbox-docker:1",
      ].includes(r.target.revision_id),
    )
    .map((r: any) => ({
      target: r.target,
      permission_revision_id:
        r.target.kind === "harness"
          ? "builtin-permission-harness-full-access:1"
          : "builtin-permission-sandbox-full-access:1",
      expected_revision: r.active?.revision || 0,
    }));
  await rpc(page, "permission.bindings.apply", {
    command_id: crypto.randomUUID(),
    payload: { bindings },
  });
  await page.getByRole("button", { name: "基础组件", exact: true }).click();
  await page.getByRole("tab", { name: "MCP", exact: true }).click();
  const source = page.getByRole("region", { name: "MCP 接入管理" });
  const name = "Browser MCP " + crypto.randomUUID().slice(0, 6);
  const creator = page.getByRole("region", {
    name: "MCP 接入管理",
    exact: true,
  });
  await creator.getByLabel("接入名称", { exact: true }).fill(name);
  await creator
    .getByRole("combobox", { name: "MCP 传输", exact: true })
    .selectOption("stdio");
  await creator
    .getByLabel("MCP 启动参数（JSON 数组）", { exact: true })
    .fill(
      JSON.stringify([
        "node",
        "--input-type=module",
        "-e",
        readFileSync(
          "../tests/fixtures/tool-integrations/mcp-service.mjs",
          "utf8",
        ),
      ]),
    );
  let releaseDefaults!: () => void;
  let defaultsRequested!: () => void;
  const defaultsGate = new Promise<void>((resolve) => { releaseDefaults = resolve; });
  const defaultsPending = new Promise<void>((resolve) => { defaultsRequested = resolve; });
  await page.route("**/rpc", async (route) => {
    if (route.request().postDataJSON().method === "query.configuration_defaults") {
      defaultsRequested();
      await defaultsGate;
    }
    return route.fallback();
  });
  await creator
    .getByRole("button", { name: "创建接入源", exact: true })
    .click();
  await expect(creator.getByRole("status")).toContainText("接入定义已保存");
  const account = page.getByRole("region", { name: "接入绑定管理" });
  await defaultsPending;
  try {
    await expect(account.getByLabel("绑定名称", { exact: true })).toBeDisabled();
  } finally {
    releaseDefaults();
  }
  await account
    .getByLabel("绑定名称", { exact: true })
    .fill("MCP Account " + name);
  await account
    .getByRole("button", { name: "创建接入绑定", exact: true })
    .click();
  await expect(account.getByRole("status")).toContainText("绑定已保存");
  await account
    .getByRole("button", { name: "发现 MCP 工具", exact: true })
    .click();
  await expect(
    account.getByRole("heading", { name: "发现的 MCP 工具", exact: true }),
  ).toBeVisible({ timeout: 90000 });
  await account.getByRole("checkbox", { name: "sum", exact: true }).check();
  await account
    .getByRole("combobox", { name: "动作影响 · sum", exact: true })
    .selectOption("read_only");
  await account
    .getByRole("button", { name: "发布所选 MCP 工具", exact: true })
    .click();
  await expect(account.getByRole("status")).toContainText("工具已发布");
  const sources = (
    await rpc(page, "query.integrations", { include_history: true })
  ).records;
  const src = sources.find(
    (r: any) => r.kind === "integration" && r.name === name,
  );
  const tools = (
    await rpc(page, "query.configurations", {
      kind: "tool",
      include_history: true,
    })
  ).records.filter((r: any) => r.data.integration_ref === src.id);
  expect(tools).toHaveLength(1);
  const tool = tools[0];
  await account
    .getByLabel("绑定允许的确切工具修订", { exact: true })
    .selectOption(tool.id);
  await account
    .getByRole("button", { name: "保存绑定新修订", exact: true })
    .click();
  await expect(account.getByRole("status")).toContainText("绑定已保存");
  await account
    .getByLabel("测试工具修订", { exact: true })
    .selectOption(tool.id);
  await account
    .getByLabel("测试调用参数", { exact: true })
    .fill('{"a":4,"b":9}');
  await account
    .getByRole("button", { name: "执行测试调用", exact: true })
    .click();
  await expect(
    account.getByRole("status").filter({ hasText: "调用已登记" }),
  ).toBeVisible();
  await expect
    .poll(
      async () => {
        const calls = await rpc(page, "query.tool_calls", {
          tool_ref: tool.id,
        });
        return calls.records[0]?.data.status;
      },
      { timeout: 60000 },
    )
    .toBe("succeeded");
  await page.getByRole("button", { name: "历史记录", exact: true }).click();
  await page.getByLabel("历史范围", { exact: true }).selectOption("global");
  const history = page.getByRole("region", { name: "历史记录" });
  await history.getByRole("button", { name: new RegExp(tool.name) }).click();
  await expect(
    history.getByRole("article", { name: "调用详情" }),
  ).toContainText("succeeded");
  await expect(
    history.getByRole("article", { name: "调用详情" }),
  ).toContainText("13");
  await page.screenshot({
    path: info.outputPath("integrations-mcp-receipt.png"),
    fullPage: true,
  });
});

test("HTTP 与 MCP 在列表内新建，接入源和发布工具可删除", async ({ page }) => {
  await setup(page);
  const cmd = (method: string, payload: any, target?: any) =>
    rpc(page, method, {
      command_id: crypto.randomUUID(),
      target: target?.id,
      expected_revision: target?.revision,
      payload,
    });
  for (const kind of ["http", "mcp"]) {
    const name = `Delete ${kind} ${crypto.randomUUID().slice(0, 6)}`;
    const source = await cmd("integration.create", {
      name,
      data: {
        source:
          kind === "http"
            ? { kind: "http_api", base_url: "https://example.test" }
            : {
                kind: "mcp",
                transport: "streamable_http",
                url: "https://mcp.example.test",
                command: [],
                protocol_version: "2026-07-28",
              },
      },
    });
    const tool = await cmd("integration.tool.create", {
      name: `Tool ${name}`,
      data: {
        schema_version: 2,
        qualified_name: `fixture_${crypto.randomUUID().slice(0, 6)}`,
        description: "Deletion fixture",
        integration_ref: source.id,
        implementation:
          kind === "http"
            ? {
                kind: "http_api",
                method: "GET",
                path: "/items",
                parameters: [],
              }
            : { kind: "mcp", name: "lookup", descriptor_hash: "a".repeat(64) },
        input_schema: { type: "object" },
        effect: "read_only",
      },
    });
    await page.goto(
      `/?tab=components&config_tab=${kind}&config_object=${source.id}`,
    );
    const label = kind === "http" ? "HTTP API" : "MCP";
    const manage = page.getByRole("region", {
      name: `${label} 接入管理`,
      exact: true,
    });
    await expect(manage.getByLabel("接入名称", { exact: true })).toHaveValue(
      name,
    );
    await expect(
      manage.locator(".configuration-list").getByRole("button", {
        name: `新建 ${label} 接入源`,
        exact: true,
      }),
    ).toBeVisible();
    const remove = page.getByRole("button", {
      name: `删除工具 · Tool ${name}`,
      exact: true,
    });
    await remove.click();
    await expect(remove.locator("..").getByRole("status")).toContainText(
      "工具已删除",
    );
    expect(
      (
        await rpc(page, "query.configurations", {
          kind: "tool",
          include_history: true,
        })
      ).records.some((r: any) => r.id === tool.id),
    ).toBe(false);
    const removeSource = manage.getByRole("button", {
      name: "删除接入源",
      exact: true,
    });
    await removeSource.click();
    await expect(removeSource.locator("..").getByRole("status")).toContainText(
      "接入源及所属动作、绑定已删除",
    );
    expect(
      (
        await rpc(page, "query.integrations", { include_history: true })
      ).records.some((r: any) => r.id === source.id),
    ).toBe(false);
    await manage
      .getByRole("button", { name: `新建 ${label} 接入源`, exact: true })
      .click();
    await expect(manage.getByLabel("接入名称", { exact: true })).toBeEnabled();
    await expect(manage.getByLabel("接入名称", { exact: true })).toHaveValue(
      "",
    );
  }
});

import { expect } from "@playwright/test";
import { test } from "./support/kernel";
import { useMockCodexCatalog } from "./support/model-catalog";

useMockCodexCatalog();

test("配置各 Tab 的共享目录按名称排序并默认选中首个已有条目", async ({
  page,
}, info) => {
  const tabs = [
    ["components", "model"],
    ["components", "harness"],
    ["components", "sandbox"],
    ["components", "skills"],
    ["actors", "role"],
    ["actors", "capability"],
    ["actors", "agent"],
    ["workflows", "delivery_type"],
    ["workflows", "spec"],
  ];
  const names = new Intl.Collator("zh-CN", { sensitivity: "base" });
  for (const [pageTab, tab] of tabs) {
    await page.goto(`/?tab=${pageTab}&config_tab=${tab}`);
    const list = page.locator(".configuration-list").first();
    const items = list.locator("button:has(strong)");
    await expect(items.first()).toBeVisible();
    await expect(items.first()).toHaveAttribute("aria-pressed", "true");
    const labels = await items.locator("strong").allTextContents();
    expect(labels).toEqual([...labels].sort(names.compare));
    await expect(list.locator(".new-resource")).toHaveAttribute(
      "aria-pressed",
      "false",
    );
    if (tab === "model") {
      await expect(items.first().locator("strong")).toHaveText("GLM 5.3 Flash");
      await page.screenshot({
        path: info.outputPath("model-default-selection.png"),
        fullPage: true,
      });
    }
  }
  await page.goto("/?tab=workflows&config_tab=workflow");
  await expect(page.getByLabel("流程名称", { exact: true })).not.toHaveValue(
    "",
  );
  await expect(page).toHaveURL(/workflow_id=/);
  await page.getByRole("button", { name: "返回流程列表", exact: true }).click();
  const workflows = page.locator(".configuration-list button:has(strong)");
  await expect(workflows.first()).toHaveAttribute("aria-pressed", "true");
  const workflowNames = await workflows.locator("strong").allTextContents();
  expect(workflowNames).toEqual([...workflowNames].sort(names.compare));

  for (const [pageTab, tab, label] of [
    ["workflows", "standard", "查看 Standard"],
    ["components", "permission", "版本化权限修订"],
  ]) {
    await page.goto(`/?tab=${pageTab}&config_tab=${tab}`);
    const select = page.getByLabel(label, { exact: true });
    await expect(select).not.toHaveValue("");
    const options = select.locator('option:not([value=""])');
    await expect(select).toHaveValue(
      (await options.first().getAttribute("value"))!,
    );
    const labels = await options.allTextContents();
    expect(labels).toEqual([...labels].sort(names.compare));
  }
  await page.goto("/?tab=components&config_tab=native");
  const native = page.locator(".native-tool-group h3");
  await expect(native.first()).toBeVisible();
  const nativeNames = await native.allTextContents();
  expect(nativeNames).toEqual([...nativeNames].sort(names.compare));
});

test("明确链接保留指定条目，新建草稿在切换 Tab 和重载后保留", async ({
  page,
}) => {
  await page.goto(
    "/?tab=components&config_tab=model&config_object=builtin-model-minimax-m3-1-flash%3A1",
  );
  const list = page.locator(".configuration-list").first();
  await expect(list.locator('button[aria-pressed="true"] strong')).toHaveText(
    "MiniMax M3.1 Flash",
  );
  await list.getByRole("button", { name: "新建 Model", exact: true }).click();
  const name = page.getByLabel("名称", { exact: true });
  await name.fill("未保存的 Model 草稿");
  await page.getByRole("tab", { name: "Harness", exact: true }).click();
  await page.getByRole("tab", { name: "Model", exact: true }).click();
  await expect(name).toHaveValue("未保存的 Model 草稿");
  await expect(list.locator(".new-resource")).toHaveAttribute(
    "aria-pressed",
    "true",
  );
  await page.reload();
  await expect(name).toHaveValue("未保存的 Model 草稿");
  await expect(list.locator(".new-resource")).toHaveAttribute(
    "aria-pressed",
    "true",
  );
});

test("MCP 与 HTTP 的接入源、绑定和动作共用排序与首项选择", async ({ page }) => {
  const sources = ["http_api", "mcp"].flatMap((kind) =>
    ["Zeta", "Alpha", "beta"].map((name, index) => ({
      id: `fixture-${kind}-${index}`,
      definition_id: `fixture-${kind}-${index}`,
      type: "integration",
      name,
      version: 1,
      revision: 1,
      scope: "global",
      data: {
        source: {
          kind,
          base_url: "https://example.test",
          transport: "stdio",
          command: ["node", "/work/fixture.mjs"],
        },
      },
    })),
  );
  const bindings = sources.flatMap((source) =>
    ["Zeta", "Alpha", "beta"].map((name, index) => ({
      id: `${source.id}-binding-${index}`,
      definition_id: `${source.id}-binding-${index}`,
      type: "integration_binding",
      name,
      version: 1,
      revision: 1,
      scope: "global",
      data: {
        integration_ref: source.id,
        harness_ref: "builtin-harness-codex-managed:1",
        sandbox_ref: "builtin-sandbox-docker:1",
        auth: { kind: "none" },
        allowed_origins: ["https://example.test"],
        allowed_tools: [],
        allow_private_network: false,
        timeout_ms: 30000,
        max_result_bytes: 1048576,
      },
    })),
  );
  const tools = sources
    .filter((source) => source.data.source.kind === "http_api")
    .flatMap((source) =>
      ["Zeta", "Alpha", "beta"].map((name, index) => ({
        id: `${source.id}-tool-${index}`,
        definition_id: `${source.id}-tool-${index}`,
        type: "tool",
        name,
        version: 1,
        revision: 1,
        scope: "global",
        data: {
          integration_ref: source.id,
          qualified_name: name,
          description: "",
          input_schema: { type: "object" },
          effect: "read_only",
          implementation: { kind: "http", method: "GET", path: "/" },
        },
      })),
    );
  await page.route("**/rpc", async (route) => {
    const call = route.request().postDataJSON();
    if (call.method === "query.integrations") {
      await route.fulfill({
        json: {
          jsonrpc: "2.0",
          id: call.id,
          result: {
            records: [...sources, ...bindings],
            total: sources.length + bindings.length,
          },
        },
      });
    } else if (call.method === "query.configuration_index") {
      const response = await route.fetch();
      const body = await response.json();
      body.result.records.push(...tools);
      body.result.total += tools.length;
      await route.fulfill({ json: body });
    } else await route.fallback();
  });
  for (const tab of ["http", "mcp"]) {
    await page.goto(`/?tab=components&config_tab=${tab}`);
    const lists = page.locator(".configuration-list");
    await expect(lists).toHaveCount(tab === "http" ? 3 : 2);
    for (const list of await lists.all()) {
      const items = list.locator("button:has(strong)");
      await expect(items.locator("strong")).toHaveText([
        "Alpha",
        "beta",
        "Zeta",
      ]);
      await expect(items.first()).toHaveAttribute("aria-pressed", "true");
    }
  }
});

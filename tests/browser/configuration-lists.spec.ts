import { expect, type Locator } from "@playwright/test";
import { test } from "./support/kernel";
import { useMockCodexCatalog } from "./support/model-catalog";

useMockCodexCatalog();

for (const phase of ["目录", "详情"] as const) {
  test(`切换 Skill 导入时${phase}尚未载入，保留导入草稿并允许手动选择`, async ({
    page,
  }) => {
    let release!: () => void;
    const pending = new Promise<void>((resolve) => (release = resolve));
    let loading = false;
    const method =
      phase === "目录"
        ? "query.configuration_index"
        : "query.configuration_records";
    await page.route("**/rpc", async (route) => {
      if (route.request().postDataJSON().method !== method)
        return route.fallback();
      loading = true;
      await pending;
      await route.fallback();
    });
    try {
      await page.goto("/?tab=components&config_tab=skills");
      await expect.poll(() => loading).toBe(true);
      const cancelled =
        phase === "详情"
          ? page.waitForEvent("requestfailed", {
              predicate: (request) =>
                request.url().endsWith("/rpc") &&
                request.postDataJSON().method === "query.configuration_records",
              timeout: 15000,
            })
          : Promise.resolve();
      await page
        .getByRole("region", { name: "Skill 管理", exact: true })
        .getByRole("button", { name: "导入 Skill 包", exact: true })
        .click();
      const importer = page.getByRole("region", { name: "Skill 包导入" });
      await importer.getByLabel("导入 Skill 名称").fill("保留导入草稿");
      await cancelled;
      release();
      await page.unrouteAll({ behavior: "wait" });
      await expect(importer).toBeVisible();
      await expect(importer.getByLabel("导入 Skill 名称")).toHaveValue(
        "保留导入草稿",
      );
      const entry = page
        .locator(".configuration-list button:has(strong)")
        .first();
      await entry.click();
      await expect(entry).toHaveAttribute("aria-pressed", "true");
      await expect(importer).not.toBeVisible();
    } finally {
      release();
      await page.unrouteAll({ behavior: "wait" });
    }
  });
}

test("旧 Sandbox 详情先于最新目录恢复时保留编辑器和未保存草稿", async ({
  page,
}) => {
  const original = "builtin-sandbox-docker:2";
  await page.goto(
    `/?tab=components&config_tab=sandbox&config_object=${encodeURIComponent(original)}`,
  );
  const editor = page.getByRole("region", {
    name: "Sandbox 管理",
    exact: true,
  });
  const selected = editor.getByLabel("Sandbox", { exact: true });
  await expect(selected).toHaveValue(original);
  await editor.getByLabel("名称", { exact: true }).fill("未保存的旧版本草稿");
  const record = await page.request.post("/rpc", {
    data: {
      jsonrpc: "2.0",
      id: 1,
      method: "query.configuration_records",
      params: { refs: [original] },
    },
  });
  const source = (await record.json()).result.records[0];
  const response = await page.request.post("/rpc", {
    data: {
      jsonrpc: "2.0",
      id: 1,
      method: "sandbox.revise",
      params: {
        command_id: crypto.randomUUID(),
        target: original,
        expected_revision: source.revision,
        payload: {
          name: "Sandbox 后继目录条目",
          scope: "global",
          data: {
            provider: "docker",
            runtime_version: "1",
            enforcement: "docker-worker/v1",
            platforms: ["linux/x86_64", "linux/aarch64"],
          },
        },
      },
    },
  });
  expect((await response.json()).error).toBeUndefined();

  let release!: () => void;
  const pending = new Promise<void>((resolve) => (release = resolve));
  await page.route("**/rpc", async (route) => {
    if (route.request().postDataJSON().method !== "query.model_providers")
      return route.fallback();
    await pending;
    await route.fallback();
  });
  let restored;
  try {
    await page.reload();
    await expect(selected).toHaveValue(original);
    restored = await selected.elementHandle();
  } finally {
    release();
  }
  await expect(
    editor.getByRole("button", { name: /Sandbox 后继目录条目/ }),
  ).toBeVisible();
  await expect(selected).toHaveValue(original);
  expect(await restored!.evaluate((node) => node.isConnected)).toBe(true);
  await expect(editor.getByLabel("名称", { exact: true })).toHaveValue(
    "未保存的旧版本草稿",
  );
});

test("目录读取晚于 Role 详情时保留已加载的职责正文", async ({ page }) => {
  await page.addInitScript(() => {
    localStorage.setItem(
      "devdesk:source-flat-v1:resource:global:agent",
      JSON.stringify({
        version: 2,
        value: {
          selected: "builtin-agent-glm-tester:6",
          name: "Test Engineer · GLM 5.3",
          scope: "global",
          data: {
            role_ref: "builtin-role-tester:1",
            capability_ref: "builtin-capability-tester-glm-dsh:6",
          },
        },
      }),
    );
  });
  let release!: () => void;
  const pending = new Promise<void>((resolve) => (release = resolve));
  await page.route("**/rpc", async (route) => {
    if (route.request().postDataJSON().method !== "query.model_providers")
      return route.fallback();
    await pending;
    await route.fallback();
  });
  const role = page.locator(".actor-role-summary");
  try {
    await page.goto(
      "/?tab=actors&config_tab=agent&config_object=builtin-agent-glm-tester%3A6",
    );
    await expect(
      role.getByRole("heading", { name: "职责", exact: true }),
    ).toBeVisible();
  } finally {
    release();
  }
  await expect(
    page.locator(".configuration-list button:has(strong)").first(),
  ).toBeVisible();
  await expect(
    role.getByRole("heading", { name: "职责", exact: true }),
  ).toBeVisible();
});

async function expectUniformRows(container: Locator) {
  const rows = await container.evaluate((node) => {
    const rows: { top: number; bottom: number }[] = [];
    for (const child of Array.from(node.children)) {
      const rect = child.getBoundingClientRect();
      if (!rect.height || !rect.width) continue;
      const row = rows.at(-1);
      if (row && Math.abs(row.top - rect.top) < 1)
        row.bottom = Math.max(row.bottom, rect.bottom);
      else rows.push({ top: rect.top, bottom: rect.bottom });
    }
    return rows;
  });
  expect(rows.length).toBeGreaterThan(1);
  for (let i = 1; i < rows.length; i++)
    expect(Math.abs(rows[i].top - rows[i - 1].bottom - 10)).toBeLessThan(1);
}

test("Role、Capability 与 Agent 的内容区块和段落间距统一为 10px", async ({
  page,
}, info) => {
  for (const width of [1440, 430]) {
    await page.setViewportSize({ width, height: 1000 });
    for (const [kind, reference, name] of [
      ["agent", "builtin-agent-glm-tester:6", "Test Engineer · GLM 5.3"],
      ["role", "builtin-role-tester:1", "测试"],
      [
        "capability",
        "builtin-capability-tester-glm-dsh:6",
        "Test Engineering Capability · GLM 5.3",
      ],
    ]) {
      await page.goto(
        `/?tab=actors&config_tab=${kind}&config_object=${encodeURIComponent(reference)}`,
      );
      await expect(
        page.getByLabel(kind === "role" ? "Role 名称" : "名称", {
          exact: true,
        }),
      ).toHaveValue(name);
      const form = page.locator("form.actor-configuration-form");
      if (kind === "capability") {
        await expect(
          form
            .getByLabel("Harness 精确版本", { exact: true })
            .locator("option:checked"),
        ).toHaveText("DeepSeek Harness · v2");
        await expect(
          form
            .getByLabel("Tool 精确版本", { exact: true })
            .locator("option:checked"),
        ).toHaveText("DeepSeek Native Tools · v2");
      }
      if (kind !== "capability")
        await expect(
          form.getByRole("heading", { name: "职责", exact: true }),
        ).toBeVisible();
      await expectUniformRows(
        kind === "role" ? form : form.locator(":scope > .form-grid"),
      );
      if (kind !== "role") await expectUniformRows(form);
      for (const preview of await form.locator(".markdown-preview").all())
        await expectUniformRows(preview);
      if (kind === "agent")
        await expectUniformRows(form.locator(".actor-role-summary"));
      await form.evaluate((node) => node.scrollIntoView({ block: "start" }));
      await page.screenshot({
        path: info.outputPath(`${kind}-spacing-${width}.png`),
      });
    }
  }
});

test("配置各 Tab 的共享目录按名称排序并默认选中首个已有条目", async ({
  page,
}, info) => {
  const tabs = [
    ["components", "model"],
    ["components", "harness"],
    ["components", "sandbox"],
    ["components", "skills"],
    ["components", "native"],
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
    if (tab === "native")
      await expect(list.locator(".new-resource")).toHaveCount(0);
    else
      await expect(list.locator(".new-resource")).toHaveAttribute(
        "aria-pressed",
        "false",
      );
    if (["harness", "sandbox"].includes(tab)) {
      const label = tab === "harness" ? "Harness" : "Sandbox";
      const select = page.getByLabel(label, { exact: true });
      await expect(select).not.toHaveValue("");
      await expect(
        select.locator("..").locator(".configuration-field-title a"),
      ).toHaveCount(0);
      await expect(
        page.getByLabel(`${label} 修订`, { exact: true }),
      ).toHaveCount(0);
    }
    if (tab === "model") {
      await expect(items.locator("strong")).toHaveText([
        "GLM 5.3",
        "GPT-6.2 Sol High",
        "MiniMax M3.1 Flash",
        "Pi · GLM 5.3",
        "Pi · MiniMax M3.1 Flash",
      ]);
      await page.screenshot({
        path: info.outputPath("model-default-selection.png"),
        fullPage: true,
      });
    }
    if (tab === "harness") {
      await expect(items.locator("strong")).toHaveText([
        "Codex",
        "DeepSeek Harness",
        "Pi Harness",
      ]);
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

  await page.goto("/?tab=workflows&config_tab=standard");
  const select = page.getByLabel("查看 Standard", { exact: true });
  await expect(select).not.toHaveValue("");
  const options = select.locator('option:not([value=""])');
  await expect(select).toHaveValue(
    (await options.first().getAttribute("value"))!,
  );
  const labels = await options.allTextContents();
  expect(labels).toEqual([...labels].sort(names.compare));

  await page.goto("/?tab=components&config_tab=permission");
  const harness = page.getByRole("group", { name: "Harness", exact: true });
  const sandbox = page.getByRole("group", { name: "Sandbox", exact: true });
  const harnessEntries = harness.locator("button:has(strong)");
  await expect(harnessEntries.first()).toHaveAttribute("aria-pressed", "true");
  await expect(harnessEntries.locator("strong")).toHaveText([
    "Harness Full Access",
    "Harness Read-Only",
    "Implementation Workspace",
    "Read-Only Workflow Step",
    "Test Authoring",
  ]);
  await expect(sandbox).toHaveJSProperty("open", false);
  await sandbox.getByText("Sandbox", { exact: true }).click();
  const sandboxEntry = sandbox.getByRole("button", {
    name: "Sandbox Full Access v2 · Global",
  });
  await expect(sandbox.locator("button strong")).toHaveText([
    "Sandbox Full Access",
  ]);
  await sandboxEntry.click();
  await expect(sandboxEntry).toHaveAttribute("aria-pressed", "true");
  await expect(harnessEntries.first()).toHaveAttribute("aria-pressed", "false");
  const editor = page.getByLabel("统一权限 JSON");
  await expect(editor).toHaveValue(/"target_kind": "sandbox"/);
  const draft = (await editor.inputValue()) + "\n";
  await editor.fill(draft);
  await sandbox.getByText("Sandbox", { exact: true }).click();
  await expect(sandboxEntry).toBeHidden();
  await expect(editor).toHaveValue(draft);
  await sandbox.getByText("Sandbox", { exact: true }).click();
  await expect(sandboxEntry).toHaveAttribute("aria-pressed", "true");
  await sandbox.getByRole("button", { name: "新建 Sandbox 权限" }).click();
  await expect(editor).toHaveValue(/"target_kind": "sandbox"/);
  await expect(sandboxEntry).toHaveAttribute("aria-pressed", "false");
  await expect(
    sandbox.getByRole("button", { name: "新建 Sandbox 权限" }),
  ).toHaveAttribute("aria-pressed", "true");
  expect(new URL(page.url()).searchParams.has("permission_ref")).toBe(false);
  await page.goto("/?tab=components&config_tab=native");
  const native = page
    .getByRole("region", { name: "原生工具目录" })
    .locator(".configuration-list button strong");
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

test("Model 管理停用未绑定的模型并保留确切修订", async ({ page }) => {
  await page.goto("/");
  const created = await page.request.post("/rpc", {
    data: {
      jsonrpc: "2.0",
      id: 1,
      method: "project.create",
      params: {
        command_id: crypto.randomUUID(),
        payload: { title: "Model retirement", repository: "fixture/model" },
      },
    },
  });
  const { result: project, error } = await created.json();
  expect(error).toBeUndefined();
  const { result: unbound, error: createError } = await (
    await page.request.post("/rpc", {
      data: {
        jsonrpc: "2.0",
        id: 1,
        method: "model.create",
        params: {
          command_id: crypto.randomUUID(),
          payload: {
            name: "Unbound model",
            scope: "global",
            data: {
              provider: "openai",
              model: "fixture-unbound-model",
              capabilities: ["structured-output", "tool-calling"],
              parameters: {},
            },
          },
        },
      },
    })
  ).json();
  expect(createError).toBeUndefined();
  const model = unbound.id;
  const route = `/?tab=components&config_tab=model&project_id=${project.id}`;
  await page.goto(`${route}&config_object=${encodeURIComponent(model)}`);
  const manager = page.getByRole("region", { name: "Model 管理", exact: true });
  const entry = manager.getByRole("button", { name: /^Unbound model/ });
  await expect(entry).toBeVisible();
  await expect(manager.getByLabel("名称", { exact: true })).toHaveValue(
    "Unbound model",
  );
  await manager
    .getByRole("button", { name: "删除 Model", exact: true })
    .click();
  await expect(manager.getByText("已停用；历史保留。")).toBeVisible();
  await expect(entry).toHaveCount(0);
  await page.goto(route);
  await expect(
    manager.getByRole("button", { name: /^GPT-6\.2 Sol High/ }),
  ).toBeVisible();
  await expect(entry).toHaveCount(0);

  const exact = await page.request.post("/rpc", {
    data: {
      jsonrpc: "2.0",
      id: 1,
      method: "query.configuration_records",
      params: { refs: [model] },
    },
  });
  expect((await exact.json()).result.records[0].id).toBe(model);
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
        harness_ref: "builtin-harness-codex-managed:2",
        sandbox_ref: "builtin-sandbox-docker:2",
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

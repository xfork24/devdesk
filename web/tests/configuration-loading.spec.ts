import { expect } from "@playwright/test";
import { test } from "./support/kernel";

test("配置页面用轻量启动和项目目录，配置变更通过 SSE 更新版本", async ({
  page,
}) => {
  const methods: string[] = [];
  const versions: string[] = [];
  page.on("request", (request) => {
    if (request.url().endsWith("/rpc"))
      methods.push(request.postDataJSON().method);
  });
  page.on("response", async (response) => {
    if (!response.url().endsWith("/rpc")) return;
    const call = response.request().postDataJSON();
    if (call.method === "query.ui_bootstrap") {
      const value = await response.json();
      if (value.result?.catalog_version)
        versions.push(JSON.stringify(value.result.catalog_version));
    }
  });
  await page.goto("/?tab=actors&feature_id=retained-selection");
  await expect(
    page
      .getByRole("region", { name: "角色管理" })
      .locator(".configuration-list button")
      .first(),
  ).toBeVisible();
  await expect
    .poll(() => methods.includes("query.project_directory"))
    .toBe(true);
  await expect.poll(() => versions.length).toBeGreaterThan(0);
  expect(methods).not.toContain("query.workspace");
  const initial = versions.at(-1);
  const response = await page.request.post("/rpc", {
    data: {
      jsonrpc: "2.0",
      id: 1,
      method: "role.create",
      params: {
        command_id: crypto.randomUUID(),
        payload: {
          name: "SSE 配置版本",
          scope: "global",
          data: { instructions: "只用于配置读取回归" },
        },
      },
    },
  });
  expect((await response.json()).error).toBeUndefined();
  await expect.poll(() => versions.at(-1)).not.toBe(initial);
  await expect(
    page
      .getByRole("region", { name: "角色管理" })
      .locator(".configuration-list"),
  ).toContainText("SSE 配置版本");
  expect(methods).not.toContain("query.workspace");
  await page.getByRole("button", { name: "流程与规范", exact: true }).click();
  await expect(
    page
      .getByRole("region", { name: "工作流程管理" })
      .locator(".configuration-list button")
      .first(),
  ).toBeVisible();
  expect(methods).not.toContain("query.workspace");
});

test("配置页保留未保存草稿，导航与事件更新不重置输入", async ({ page }) => {
  await page.goto("/?tab=actors");
  await page.getByRole("button", { name: "新建 Role", exact: true }).click();
  const title = page.getByLabel("Role 名称", { exact: true });
  await title.fill("尚未保存的角色");
  await page.request.post("/rpc", {
    data: {
      jsonrpc: "2.0",
      id: 1,
      method: "role.create",
      params: {
        command_id: crypto.randomUUID(),
        payload: {
          name: "另一个标签页发布的角色",
          scope: "global",
          data: { instructions: "触发后台配置刷新" },
        },
      },
    },
  });
  await expect(
    page
      .getByRole("region", { name: "角色管理" })
      .locator(".configuration-list"),
  ).toContainText("另一个标签页发布的角色");
  await expect(title).toHaveValue("尚未保存的角色");
  await page.getByRole("button", { name: "流程与规范", exact: true }).click();
  await page
    .getByRole("button", { name: /^角色与(?:Agent|代理)$/, exact: true })
    .click();
  await expect(title).toHaveValue("尚未保存的角色");
});

test("项目预览后台刷新保留可操作表单与角色绑定", async ({ page }) => {
  await page.goto("/");
  const created = await page.request.post("/rpc", {
    data: {
      jsonrpc: "2.0",
      id: 1,
      method: "project.create",
      params: {
        command_id: crypto.randomUUID(),
        payload: {
          title: "Refresh preview",
          repository: "fixture/refresh-preview",
        },
      },
    },
  });
  expect((await created.json()).error).toBeUndefined();
  await page.goto("/?tab=projects");
  const panel = page.getByRole("region", { name: "项目配置" });
  const save = panel.getByRole("button", { name: "保存项目配置", exact: true });
  await expect(save).toBeEnabled();
  let release = () => {};
  let pending = 0;
  const held = new Promise<void>((resolve) => {
    release = resolve;
  });
  await page.route("**/rpc", async (route) => {
    if (
      route.request().postDataJSON().method !== "query.project_standard_preview"
    )
      return route.continue();
    const response = await route.fetch();
    pending += 1;
    await held;
    await route.fulfill({ response });
  });
  try {
    await page.request.post("/rpc", {
      data: {
        jsonrpc: "2.0",
        id: 2,
        method: "role.create",
        params: {
          command_id: crypto.randomUUID(),
          payload: {
            name: "Refresh signal",
            scope: "global",
            data: { instructions: "Refresh saved configuration" },
          },
        },
      },
    });
    await expect.poll(() => pending).toBeGreaterThan(0);
    await expect(save).toBeEnabled();
    await expect(
      panel
        .getByRole("group", { name: "流程所需 Role → Agent", exact: true })
        .getByRole("combobox"),
    ).toHaveCount(6);
  } finally {
    release();
    await page.unrouteAll({ behavior: "wait" });
  }
});

test("配置索引跨页复用，正文只在选择对象后读取", async ({ page }) => {
  const methods: string[] = [];
  page.on("request", (r) => {
    if (r.url().endsWith("/rpc")) methods.push(r.postDataJSON().method);
  });
  await page.goto("/?tab=actors");
  const list = page
    .getByRole("region", { name: "角色管理" })
    .locator(".configuration-list button:has(strong)");
  await expect(list.first()).toBeVisible();
  expect(methods).not.toContain("query.configurations");
  expect(methods).not.toContain("query.configuration_records");
  await list.first().click();
  await expect(page.getByLabel("Role 名称", { exact: true })).not.toHaveValue(
    "",
  );
  await expect
    .poll(
      () => methods.filter((m) => m === "query.configuration_records").length,
    )
    .toBe(1);
  const indexes = methods.filter(
    (m) => m === "query.configuration_index",
  ).length;
  await page.getByRole("button", { name: "流程与规范", exact: true }).click();
  await expect(
    page
      .getByRole("region", { name: "工作流程管理" })
      .locator(".configuration-list button:has(strong)")
      .first(),
  ).toBeVisible();
  const afterB = methods.filter(
    (m) => m === "query.configuration_index",
  ).length;
  await page
    .getByRole("button", { name: /^角色与(?:Agent|代理)$/, exact: true })
    .click();
  await expect(list.first()).toBeVisible();
  expect(methods.filter((m) => m === "query.configuration_index").length).toBe(
    afterB,
  );
  expect(afterB).toBeGreaterThanOrEqual(indexes);
  expect(methods).not.toContain("query.workflows");
});

test("模型目录慢或失败时仍能编辑；切回复用，换账号隔离", async ({ page }) => {
  let account = "one",
    calls = 0,
    release!: () => void,
    fail = false;
  const held = new Promise<void>((done) => (release = done));
  await page.route("**/rpc", async (route) => {
    const call = route.request().postDataJSON();
    if (call.method === "query.model_catalog_identity")
      return route.fulfill({
        json: {
          jsonrpc: "2.0",
          id: call.id,
          result: { identity: account, available: true },
        },
      });
    if (call.method !== "codex.auth.models") return route.fallback();
    calls++;
    await held;
    return route.fulfill({
      json: {
        jsonrpc: "2.0",
        id: call.id,
        ...(fail
          ? { error: { message: "offline" } }
          : { result: { models: [{ model: `private-${account}` }] } }),
      },
    });
  });
  await page.goto("/?tab=components&config_tab=model");
  const panel = page.getByRole("region", { name: "Model 管理" });
  await expect(
    panel.locator(".configuration-list button:has(strong)").first(),
  ).toBeVisible();
  await panel.getByRole("button", { name: /GPT-6.1 Sol High/ }).click();
  const select = page.getByLabel("模型标识", { exact: true });
  await expect(select).toBeEnabled();
  await expect(select).not.toHaveValue("");
  await page.getByLabel("名称", { exact: true }).fill("目录未完成仍可编辑");
  await expect.poll(() => calls).toBe(1);
  await page.getByRole("tab", { name: "Harness", exact: true }).click();
  await page.getByRole("tab", { name: "Model", exact: true }).click();
  await expect(select).toBeEnabled();
  await expect(page.getByText(/正在后台刷新模型目录/)).toBeVisible();
  expect(calls).toBe(1);
  release();
  await expect(select.locator('option[value="private-one"]')).toHaveCount(1);
  await page.getByRole("tab", { name: "Harness", exact: true }).click();
  await page.getByRole("tab", { name: "Model", exact: true }).click();
  await expect(select.locator('option[value="private-one"]')).toHaveCount(1);
  expect(calls).toBe(1);
  account = "two";
  fail = true;
  await page.getByRole("button", { name: "刷新模型目录", exact: true }).click();
  await expect(select.locator('option[value="private-one"]')).toHaveCount(0);
  await expect(page.getByText(/offline/)).toBeVisible();
  await expect(select).toBeEnabled();
  const failed = calls;
  await page.getByRole("tab", { name: "Harness", exact: true }).click();
  await page.getByRole("tab", { name: "Model", exact: true }).click();
  await expect(page.getByText(/offline/)).toBeVisible();
  expect(calls).toBe(failed);
  await expect(page.getByLabel("名称", { exact: true })).toHaveValue(
    "目录未完成仍可编辑",
  );
});

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
  await page.goto("/?tab=actors");
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
  await page.getByRole("button", { name: "流程与规范", exact: true }).click();
  await page
    .getByRole("button", { name: /^角色与(?:Agent|代理)$/, exact: true })
    .click();
  await expect(title).toHaveValue("尚未保存的角色");
});

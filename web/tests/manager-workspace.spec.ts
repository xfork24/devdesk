import { test, expect } from "@playwright/test";
import { mkdtempSync } from "node:fs";
import { tmpdir } from "node:os";
import { join } from "node:path";

test("没有 Feature 时项目经理工作台提供独立的组合与会话入口", async ({
  page,
}) => {
  await page.goto("/");
  const capabilities = await page.request
    .get("/capabilities")
    .then((r) => r.json());
  if (capabilities.setup_required) {
    await page
      .getByLabel("数据目录")
      .fill(mkdtempSync(join(tmpdir(), "devdesk-manager-ui-")));
    await page.getByRole("button", { name: "初始化 / 打开数据集" }).click();
  }
  const created = await page.request
    .post("/rpc", {
      data: {
        jsonrpc: "2.0",
        id: 1,
        method: "project.create",
        params: {
          command_id: crypto.randomUUID(),
          payload: { title: "Manager without Feature", repository:"https://github.com/fixture/manager.git" },
        },
      },
    })
    .then((r) => r.json());
  expect(created.error, JSON.stringify(created)).toBeUndefined();
  await page.reload();
  await page.getByRole("button", { name: "项目管理", exact: true }).click();
  const workspace = page.getByRole("region", { name: "项目经理工作台" });
  await expect(workspace).toBeVisible();
  await expect(workspace.getByLabel("工作台 Agent 精确修订")).toBeVisible();
  await expect(workspace.getByLabel("工作台授权上下文")).toBeVisible();
  await expect(workspace.getByLabel("给项目经理的指令")).toBeVisible();
  await expect(workspace.getByLabel("切换调度模式")).toHaveCount(0);
  const result = await page.request
    .post("/rpc", {
      data: {
        jsonrpc: "2.0",
        id: 2,
        method: "query.project_knowledge",
        params: { project_id: created.result.id },
      },
    })
    .then((r) => r.json());
  expect(result.error, JSON.stringify(result)).toBeUndefined();
  expect(result.result.counts.features).toBe(0);
  expect(result.result.counts.tasks).toBe(0);
});

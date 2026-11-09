import { test, expect, Page } from "@playwright/test";
import { mkdtempSync } from "node:fs";
import { tmpdir } from "node:os";
import { join } from "node:path";
async function setup(page: Page) {
  await page.goto("/");
  if (await page.getByLabel("数据目录").count()) {
    await page
      .getByLabel("数据目录")
      .fill(mkdtempSync(join(tmpdir(), "devdesk-workflow-web-")));
    await page.getByRole("button", { name: "初始化 / 打开数据集" }).click();
  }
  await page.getByRole("button", { name: "工作流程", exact: true }).click();
}
test("Workflow 编辑、版本发布和刷新恢复使用真实 RPC 与 SQLite", async ({
  page,
}, info) => {
  await setup(page);
  await page
    .getByLabel("流程名称", { exact: true })
    .fill("浏览器工作流 " + crypto.randomUUID().slice(0, 6));
  await page
    .getByRole("button", { name: "新建 Workflow", exact: true })
    .click();
  await expect(
    page.getByText("草稿 revision 1", { exact: false }),
  ).toBeVisible();
  await page
    .getByRole("button", { name: "需求头脑风暴", exact: true })
    .first()
    .click();
  await page
    .getByLabel("讨论目标", { exact: true })
    .fill("明确需求、验收和错误路径，输出可采纳需求。");
  await page
    .getByLabel("推进策略", { exact: true })
    .selectOption("human_confirmation");
  await page.getByRole("button", { name: "保存草稿", exact: true }).click();
  await expect(
    page.getByRole("status").filter({ hasText: "已保存" }),
  ).toBeVisible();
  await page.getByRole("button", { name: "校验流程", exact: true }).click();
  await expect(page.getByText("校验通过", { exact: true })).toBeVisible();
  await page.getByRole("button", { name: "预演", exact: true }).click();
  await expect(
    page.getByText("演练 · 不产生工程完成事实", { exact: true }),
  ).toBeVisible();
  await expect(
    page.locator("li").filter({ hasText: "等待前置" }).first(),
  ).toBeVisible();
  await page
    .getByLabel("样本已产生输出的步骤")
    .selectOption(["initial_snapshot"]);
  await page
    .getByRole("button", { name: "按样本重新演练", exact: true })
    .click();
  await expect(
    page.locator("li").filter({ hasText: "等待交互" }).first(),
  ).toBeVisible();
  await page.getByRole("button", { name: "发布版本", exact: true }).click();
  await expect(page.getByText("发布版本 v1", { exact: false })).toBeVisible();
  await page
    .getByRole("button", { name: "需求头脑风暴", exact: true })
    .first()
    .click();
  await expect(page.getByLabel("讨论目标", { exact: true })).toBeDisabled();
  await page.reload();
  await expect(
    page.getByRole("heading", { name: "Workflow 定义与版本", exact: true }),
  ).toBeVisible();
  await page
    .getByRole("button", { name: "浏览器工作流", exact: false })
    .last()
    .click();
  await page.screenshot({
    path: info.outputPath("workflow-manager.png"),
    fullPage: true,
  });
});
test("Agent 组合管理不依赖 GitHub 连接且可以创建 Model 和 Tool", async ({
  page,
}) => {
  await setup(page);
  await page.getByRole("button", { name: "规范与身份", exact: true }).click();
  await expect(
    page.getByRole("heading", { name: "Agent 组合与实例" }),
  ).toBeVisible();
  await page.getByRole("button", { name: "Model", exact: true }).click();
  const panel = page
    .locator("section")
    .filter({ has: page.getByRole("heading", { name: "Agent 组合与实例" }) });
  await panel.getByLabel("名称", { exact: true }).fill("Browser Model");
  await panel.getByLabel("模型标识", { exact: true }).fill("fixture-model");
  await panel.getByRole("button", { name: "创建定义", exact: true }).click();
  await expect(
    panel.getByRole("button", { name: /Browser Model v1/ }),
  ).toBeVisible();
  await panel.getByRole("button", { name: "Tool", exact: true }).click();
  await panel
    .getByLabel("名称", { exact: true })
    .fill("Browser experience tool");
  await panel.getByLabel("执行处理器").selectOption("experience.lookup");
  await panel.getByRole("button", { name: "创建定义", exact: true }).click();
  await expect(
    panel.getByRole("button", { name: /Browser experience tool v1/ }),
  ).toBeVisible();
});

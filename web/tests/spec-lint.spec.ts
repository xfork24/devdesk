import { test, expect } from "@playwright/test";
import { mkdtempSync } from "node:fs";
import { tmpdir } from "node:os";
import { join } from "node:path";

test("Spec 草稿自检显示机器结构结论并保留语义判断边界", async ({ page }) => {
  await page.goto("/");
  const capabilities = await page.request
    .get("/capabilities")
    .then((r) => r.json());
  if (capabilities.setup_required) {
    await page
      .getByLabel("数据目录")
      .fill(mkdtempSync(join(tmpdir(), "devdesk-spec-lint-")));
    await page.getByRole("button", { name: "初始化 / 打开数据集" }).click();
  }
  await page.getByRole("button", { name: "流程与规范", exact: true }).click();
  await page.getByRole("tab", { name: "Spec", exact: true }).click();
  await page.getByRole("button", { name: /可执行 Markdown 结构规范/ }).click();
  const panel = page.getByLabel("规范结构自检");
  await panel.locator("summary").click();
  await panel.getByLabel("待检查内容").fill("short");
  await panel.getByRole("button", { name: "运行结构自检" }).click();
  await expect(panel.getByRole("status")).toContainText("结构检查未通过");
  await panel
    .getByLabel("待检查内容")
    .fill("# 结论\n这是具体的结论内容。\n# 依据\n这是实际依据的位置。");
  await panel.getByRole("button", { name: "运行结构自检" }).click();
  await expect(panel.getByRole("status")).toContainText("结构检查通过");
  await expect(panel.getByRole("status")).toContainText("语义质量仍需明确判断");
});

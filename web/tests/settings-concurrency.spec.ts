import { test, expect, type Page } from "@playwright/test";
import { mkdtempSync } from "node:fs";
import { tmpdir } from "node:os";
import { join } from "node:path";

async function settings(page: Page) {
  await page.goto("/");
  if ((await (await page.request.get("/capabilities")).json()).setup_required) {
    await page
      .getByLabel("数据目录")
      .fill(mkdtempSync(join(tmpdir(), "devdesk-settings-concurrency-")));
    await page.getByRole("button", { name: "初始化 / 打开数据集" }).click();
    await expect(page.getByLabel("数据目录")).toHaveCount(0);
  }
  await page.getByRole("button", { name: "设置", exact: true }).click();
  await expect(
    page.getByRole("heading", { name: "全局并发额度", exact: true }),
  ).toBeVisible();
}

test("两个前端修改同一设置时拒绝过期版本并保留草稿", async ({
  page,
  browser,
}) => {
  await settings(page);
  const other = await browser.newContext();
  try {
    const second = await other.newPage();
    await settings(second);
    await page.getByRole("button", { name: "调整", exact: true }).click();
    const firstDialog = page.getByRole("dialog", { name: "修改全局并发" });
    await firstDialog.getByLabel("并发额度 1—32").fill("7");

    await second.getByRole("button", { name: "调整", exact: true }).click();
    const secondDialog = second.getByRole("dialog", { name: "修改全局并发" });
    await secondDialog.getByLabel("并发额度 1—32").fill("6");
    const committed = second.waitForResponse((response) => {
      if (!response.url().endsWith("/rpc")) return false;
      return response.request().postDataJSON()?.method === "settings.update";
    });
    await secondDialog
      .getByRole("button", { name: "确认", exact: true })
      .click();
    const receipt = await (await committed).json();
    expect(receipt.result).toEqual({ global_concurrency: 6 });
    expect(receipt.meta.commit.namespace).toEqual({ kind: "configuration" });
    expect(receipt.meta.commit.versions).toHaveLength(1);
    await expect(secondDialog).toHaveCount(0);

    await firstDialog
      .getByRole("button", { name: "确认", exact: true })
      .click();
    await expect(firstDialog.getByRole("alert")).toContainText(
      "revision_conflict",
    );
    await expect(firstDialog.getByLabel("并发额度 1—32")).toHaveValue("7");
    // Retrying the retained edit must keep its original version, even after SSE refreshes.
    await firstDialog
      .getByRole("button", { name: "确认", exact: true })
      .click();
    await expect(firstDialog.getByRole("alert")).toContainText(
      "revision_conflict",
    );
    const authoritative = await page.request.post("/rpc", {
      data: { jsonrpc: "2.0", id: 1, method: "query.settings", params: {} },
    });
    expect((await authoritative.json()).result.global_concurrency).toBe(6);
  } finally {
    await other.close();
  }
});

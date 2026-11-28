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
    page.getByRole("heading", { name: "并发额度设置", exact: true }),
  ).toBeVisible();
}

test("两个前端修改同一设置时拒绝过期版本并保留草稿", async ({
  page,
  browser,
}) => {
  await settings(page);
  const summary = (front: Page) =>
    front.locator(".setting-row").filter({
      has: front.getByRole("heading", { name: "并发额度设置", exact: true }),
    });
  await expect(summary(page)).toContainText(
    "全局 32 个 Run · 单项目默认 8 个 Run",
  );
  const other = await browser.newContext();
  try {
    const second = await other.newPage();
    await settings(second);
    await page.getByRole("button", { name: "调整", exact: true }).click();
    const firstDialog = page.getByRole("dialog", { name: "并发额度设置" });
    await firstDialog.getByLabel("全局并发额度").fill("7");

    await second.getByRole("button", { name: "调整", exact: true }).click();
    const secondDialog = second.getByRole("dialog", { name: "并发额度设置" });
    await secondDialog.getByLabel("全局并发额度").fill("6");
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
    await expect(firstDialog.getByLabel("全局并发额度")).toHaveValue("7");
    // Retrying the retained edit must keep its original version, even after SSE refreshes.
    await firstDialog
      .getByRole("button", { name: "确认", exact: true })
      .click();
    await expect(firstDialog.getByRole("alert")).toContainText(
      "revision_conflict",
    );
    await firstDialog
      .getByRole("button", { name: "关闭", exact: true })
      .click();
    await second.getByRole("button", { name: "调整", exact: true }).click();
    await secondDialog.getByLabel("单项目默认并发额度").fill("11");
    await secondDialog
      .getByRole("button", { name: "确认", exact: true })
      .click();
    await expect(secondDialog).toHaveCount(0);
    await expect(summary(page)).toContainText(
      "全局 6 个 Run · 单项目默认 11 个 Run",
    );
    await second.reload();
    await expect(summary(second)).toContainText(
      "全局 6 个 Run · 单项目默认 11 个 Run",
    );
    const authoritative = await page.request.post("/rpc", {
      data: { jsonrpc: "2.0", id: 1, method: "query.settings", params: {} },
    });
    expect((await authoritative.json()).result).toMatchObject({
      global_concurrency: 6,
      default_project_concurrency: 11,
    });
  } finally {
    await other.close();
  }
});

test("凭据冲突保留密钥草稿、原始服务商修订和命令身份", async ({
  page,
  browser,
}) => {
  const other = await browser.newContext();
  let version = 0;
  let statusReads = 0;
  const submissions: any[] = [];
  const configure = async (target: Page) => {
    await target.route("**/rpc", async (route) => {
      const call = route.request().postDataJSON();
      if (call.method === "credentials.status") {
        statusReads++;
        return route.fulfill({
          json: {
            jsonrpc: "2.0",
            id: call.id,
            result: {
              configured: version > 0,
              available: true,
              storage: "test_vault",
              auth_kind: "api_key",
              provider_ref: `fixture-provider:${version + 1}`,
              credential_version: {
                resource: { kind: "setting", id: "credentials.dsh.minimax-cn" },
                revision: version,
              },
            },
          },
        });
      }
      if (call.method !== "credentials.configure") return route.fallback();
      submissions.push(call.params);
      if (call.params.expected_revision !== version)
        return route.fulfill({
          json: {
            jsonrpc: "2.0",
            id: call.id,
            error: {
              message: "credential binding changed",
              data: { code: "revision_conflict" },
            },
          },
        });
      version++;
      return route.fulfill({
        json: {
          jsonrpc: "2.0",
          id: call.id,
          result: {
            configured: true,
            available: true,
            storage: "test_vault",
            auth_kind: "api_key",
            provider_ref: call.params.provider_ref,
            credential_version: {
              resource: { kind: "setting", id: "credentials.dsh.minimax-cn" },
              revision: version,
            },
          },
        },
      });
    });
    await settings(target);
  };
  try {
    const second = await other.newPage();
    await configure(page);
    await configure(second);
    const firstDsh = page.getByRole("region", {
      name: "DeepSeek Harness",
      exact: true,
    });
    const secondDsh = second.getByRole("region", {
      name: "DeepSeek Harness",
      exact: true,
    });
    const firstSecret = firstDsh.getByLabel("配置或更换 API key", {
      exact: true,
    });
    await firstSecret.fill("FIRST_DUMMY_CREDENTIAL");
    await secondDsh
      .getByLabel("配置或更换 API key", { exact: true })
      .fill("SECOND_DUMMY_CREDENTIAL");
    await secondDsh
      .getByRole("button", { name: "保存密钥", exact: true })
      .click();
    await expect(
      secondDsh.getByLabel("配置或更换 API key", { exact: true }),
    ).toHaveValue("");
    const before = statusReads;
    await page.evaluate(() =>
      window.dispatchEvent(new Event("devdesk:runtime-options-changed")),
    );
    await expect.poll(() => statusReads).toBeGreaterThan(before);
    await expect(firstSecret).toHaveValue("FIRST_DUMMY_CREDENTIAL");
    await firstDsh
      .getByRole("button", { name: "保存密钥", exact: true })
      .click();
    await expect(page.getByText(/revision_conflict/)).toBeVisible();
    await expect(firstSecret).toHaveValue("FIRST_DUMMY_CREDENTIAL");
    await firstDsh
      .getByRole("button", { name: "保存密钥", exact: true })
      .click();
    await expect.poll(() => submissions.length).toBe(3);
    expect(submissions[1].expected_revision).toBe(0);
    expect(submissions[1].provider_ref).toBe("fixture-provider:1");
    expect(submissions[2]).toEqual(submissions[1]);
    expect(version).toBe(1);
  } finally {
    await other.close();
  }
});

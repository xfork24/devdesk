import { test as base, expect, type Page } from "@playwright/test";
import { spawn } from "node:child_process";
import {
  mkdtempSync,
  readdirSync,
  readFileSync,
  openSync,
  closeSync,
} from "node:fs";
import { createHash } from "node:crypto";
import { createServer } from "node:net";
import { tmpdir } from "node:os";
import { join, resolve } from "node:path";
import { once } from "node:events";

const test = base.extend<{ appUrl: string }>({
  // An empty destructuring pattern is Playwright's way to ignore fixture args.
  // eslint-disable-next-line no-empty-pattern
  appUrl: async ({}, use) => {
    const socket = createServer();
    await new Promise<void>((done) => socket.listen(0, "127.0.0.1", done));
    const port = (socket.address() as { port: number }).port;
    await new Promise<void>((done) => socket.close(() => done()));
    const root = mkdtempSync(join(tmpdir(), "devdesk-template-reset-e2e-"));
    const log = openSync(join(root, "host.log"), "w");
    const process = spawn(
      resolve(
        `../target/${globalThis.process.env.DEVDESK_E2E_PROFILE ?? "debug"}/devdesk-host`,
      ),
      ["--port", String(port), "--data", join(root, "data")],
      { stdio: ["ignore", log, log] },
    );
    closeSync(log);
    const url = `http://127.0.0.1:${port}`;
    try {
      await expect
        .poll(async () => {
          try {
            return (await fetch(`${url}/capabilities`)).status;
          } catch {
            return 0;
          }
        })
        .toBeGreaterThan(0);
      await use(url);
    } finally {
      if (process.exitCode === null) {
        const exited = once(process, "exit");
        process.kill("SIGTERM");
        await exited;
      }
    }
  },
});

test.afterEach(async ({ page }) => {
  await page.unrouteAll({ behavior: "wait" });
});

async function rpc(page: Page, url: string, method: string, params = {}) {
  const value = await page.request
    .post(`${url}/rpc`, { data: { jsonrpc: "2.0", id: 1, method, params } })
    .then((r) => r.json());
  expect(value.error, JSON.stringify(value)).toBeUndefined();
  return value.result;
}
function resourceDigest() {
  const root = resolve("../resources/configuration"),
    hash = createHash("sha256");
  function visit(path: string) {
    for (const entry of readdirSync(path, { withFileTypes: true }).sort(
      (a, b) => a.name.localeCompare(b.name),
    )) {
      const file = join(path, entry.name);
      if (entry.isDirectory()) visit(file);
      else {
        hash.update(file.slice(root.length));
        hash.update(readFileSync(file));
      }
    }
  }
  visit(root);
  return hash.digest("hex");
}

test("UI 编辑保存数据库副本，确认重置恢复默认并隔离旧草稿", async ({
  page,
  appUrl,
}, info) => {
  const errors: string[] = [];
  page.on("pageerror", (error) => errors.push(error.message));
  const before = resourceDigest();
  await page.goto(`${appUrl}/?tab=actors&config_tab=role`);
  await page
    .getByRole("button", { name: /需求分析师/ })
    .first()
    .click();
  await page.getByLabel("Role 名称", { exact: true }).fill("自定义需求角色");
  await page
    .getByRole("button", { name: "保存 Role 新版本", exact: true })
    .click();
  await expect(
    page.getByText("Role 已保存；其他配置保持其已选修订。", { exact: true }),
  ).toBeVisible();
  await page.reload();
  await expect(
    page.getByRole("button", { name: /自定义需求角色/ }),
  ).toBeVisible();
  const original = await rpc(page, appUrl, "query.roles", {
    include_history: true,
    limit: 100,
  });
  const edited = original.records.find((r: any) => r.name === "自定义需求角色");
  expect(edited).toBeTruthy();
  const defaults = await rpc(page, appUrl, "query.configuration_defaults");
  const customStandard = await rpc(page, appUrl, "standard.capture", {
    command_id: crypto.randomUUID(),
    payload: {
      name: "需被完全删除的自建 Standard",
      root_workflow_refs: [defaults.workflow_ref],
    },
  });
  await page.getByRole("button", { name: "新建 Role", exact: true }).click();
  await page.getByLabel("Role 名称", { exact: true }).fill("未保存草稿");
  await page.getByRole("button", { name: "设置", exact: true }).click();
  await expect(
    page.getByRole("button", { name: "启用新版出厂默认", exact: true }),
  ).toHaveCount(0);
  await expect(
    page.getByRole("button", { name: "恢复默认配置", exact: true }),
  ).toHaveCount(0);
  await page.getByRole("button", { name: "重置模板定义", exact: true }).click();
  const dialog = page.getByRole("dialog", { name: "确认重置模板定义" });
  await dialog
    .getByRole("button", { name: "恢复默认配置", exact: true })
    .click();
  await expect(dialog).not.toBeVisible();
  expect(
    (
      await rpc(page, appUrl, "query.roles", {
        include_history: true,
        limit: 100,
      })
    ).records.some((r: any) => r.id === edited.id),
  ).toBe(true);
  expect(
    (await rpc(page, appUrl, "query.standard", { id: customStandard.id }))
      .record.name,
  ).toBe("需被完全删除的自建 Standard");
  await page.getByRole("button", { name: "重置模板定义", exact: true }).click();
  await expect(dialog).toContainText(
    "项目、仓库、执行记录、会话、经验和登录凭据保留",
  );
  await page.screenshot({
    path: info.outputPath("reset-preview.png"),
    fullPage: true,
  });
  await dialog.getByRole("button", { name: "取消", exact: true }).click();
  expect(
    (await rpc(page, appUrl, "query.roles", { limit: 100 })).records.some(
      (r: any) => r.name === "自定义需求角色",
    ),
  ).toBe(true);
  await page.getByRole("button", { name: "重置模板定义", exact: true }).click();
  await dialog.getByRole("button", { name: "重置配置", exact: true }).click();
  await expect(
    page.getByRole("status").filter({ hasText: "模板定义已恢复默认" }),
  ).toBeVisible();
  expect((await rpc(page, appUrl, "query.template_catalog")).generation).toBe(
    2,
  );
  const current = await rpc(page, appUrl, "query.roles", {
    include_history: true,
    limit: 100,
  });
  expect(current.records.some((r: any) => r.id === edited.id)).toBe(false);
  const historical = await rpc(page, appUrl, "query.roles", {
    include_archived: true,
    include_history: true,
    limit: 100,
  });
  expect(historical.records.some((r: any) => r.id === edited.id)).toBe(false);
  const standards = await rpc(page, appUrl, "query.configuration_index", {
    kind: "standard",
    include_archived: true,
    include_history: true,
  });
  expect(standards.records.some((r: any) => r.id === customStandard.id)).toBe(
    false,
  );
  await page.getByRole("button", { name: "角色与代理", exact: true }).click();
  await expect
    .poll(() =>
      current.records.some(
        (role: any) =>
          role.id === new URL(page.url()).searchParams.get("config_object"),
      ),
    )
    .toBe(true);
  const selected = current.records.find(
    (role: any) =>
      role.id === new URL(page.url()).searchParams.get("config_object"),
  );
  await expect(page.getByLabel("Role 名称", { exact: true })).toHaveValue(
    selected.name,
  );
  await expect(
    page.getByRole("button", { name: /需求分析师/ }).first(),
  ).toBeVisible();
  expect(resourceDigest()).toBe(before);
  expect(errors).toEqual([]);
  await page.screenshot({
    path: info.outputPath("reset-roles.png"),
    fullPage: true,
  });
});

test("操作后关闭弹窗，失败可重新打开并读取最新预览", async ({
  page,
  appUrl,
}) => {
  await page.goto(`${appUrl}/?tab=settings`);
  await page.getByRole("button", { name: "重置模板定义", exact: true }).click();
  const dialog = page.getByRole("dialog", { name: "确认重置模板定义" });
  await expect(
    dialog.getByRole("button", { name: "重新预览", exact: true }),
  ).toHaveCount(0);
  await expect(
    dialog.getByRole("button", { name: "重置配置", exact: true }),
  ).toBeEnabled();
  await rpc(page, appUrl, "delivery_type.create", {
    command_id: crypto.randomUUID(),
    payload: {
      spec_refs: ["builtin-spec-stage-prd:2"],
      name: "Concurrent delivery",
      data: { format: "markdown", body: "concurrent" },
    },
  });
  await dialog.getByRole("button", { name: "重置配置", exact: true }).click();
  await expect(dialog).not.toBeVisible();
  await expect(
    page
      .getByRole("region", { name: "模板与额度设置", exact: true })
      .getByRole("alert"),
  ).toContainText("请重新打开重置窗口");
  expect((await rpc(page, appUrl, "query.template_catalog")).generation).toBe(
    1,
  );
  await page.getByRole("button", { name: "重置模板定义", exact: true }).click();
  let failed = false;
  await page.route("**/rpc", async (route) => {
    const call = route.request().postDataJSON();
    if (call.method === "configuration.templates.reset" && !failed) {
      failed = true;
      return route.fulfill({
        json: {
          jsonrpc: "2.0",
          id: call.id,
          error: { code: -32000, message: "Fixture storage failure" },
        },
      });
    }
    return route.fallback();
  });
  await dialog.getByRole("button", { name: "重置配置", exact: true }).click();
  await expect(dialog).not.toBeVisible();
  await expect(
    page
      .getByRole("region", { name: "模板与额度设置", exact: true })
      .getByRole("alert"),
  ).toContainText("Fixture storage failure");
  await page.getByRole("button", { name: "重置模板定义", exact: true }).click();
  await dialog.getByRole("button", { name: "重置配置", exact: true }).click();
  await expect(
    page.getByRole("status").filter({ hasText: "模板定义已恢复默认" }),
  ).toBeVisible();
  await page.getByRole("button", { name: "重置模板定义", exact: true }).click();
  await dialog.getByRole("button", { name: "重置配置", exact: true }).click();
  await expect(
    page.getByRole("status").filter({ hasText: "模板定义已是当前默认版本" }),
  ).toBeVisible();
  expect((await rpc(page, appUrl, "query.template_catalog")).generation).toBe(
    2,
  );
});

test("恢复默认配置失败时也关闭弹窗并在设置页显示错误", async ({
  page,
  appUrl,
}) => {
  await page.goto(`${appUrl}/?tab=settings`);
  await page.getByRole("button", { name: "重置模板定义", exact: true }).click();
  const dialog = page.getByRole("dialog", { name: "确认重置模板定义" });
  await page.route("**/rpc", async (route) => {
    const call = route.request().postDataJSON();
    if (
      call.method === "configuration.templates.reset" &&
      call.params.payload.preserve_custom
    )
      return route.fulfill({
        json: {
          jsonrpc: "2.0",
          id: call.id,
          error: { code: -32000, message: "恢复失败测试" },
        },
      });
    return route.fallback();
  });
  await dialog
    .getByRole("button", { name: "恢复默认配置", exact: true })
    .click();
  await expect(dialog).not.toBeVisible();
  await expect(
    page
      .getByRole("region", { name: "模板与额度设置", exact: true })
      .getByRole("alert"),
  ).toContainText("恢复失败测试");
});

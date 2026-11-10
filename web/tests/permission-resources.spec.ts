import { test, expect } from "@playwright/test";
import {
  mkdtempSync,
  readFileSync,
  writeFileSync,
  realpathSync,
} from "node:fs";
import { tmpdir } from "node:os";
import { basename, join } from "node:path";

test("统一权限文件与 UI 修订在运行时绑定确切目标版本", async ({
  page,
}, testInfo) => {
  const consoleErrors: string[] = [];
  page.on("pageerror", (e) => consoleErrors.push(e.message));
  await page.goto("/");
  if (
    (await page.request.get("/capabilities").then((r) => r.json()))
      .setup_required
  ) {
    await page
      .getByLabel("数据目录")
      .fill(mkdtempSync(join(tmpdir(), "devdesk-permission-web-")));
    await page.getByRole("button", { name: /初始化/ }).click();
  }
  const rpc = async (method: string, params = {}) => {
    const reply = await page.request
      .post("/rpc", { data: { jsonrpc: "2.0", id: 1, method, params } })
      .then((r) => r.json());
    expect(reply.error).toBeUndefined();
    return reply.result;
  };
  await page.getByRole("button", { name: "设置", exact: true }).click();
  await expect(
    page.getByRole("heading", { name: "Permission 定义与版本绑定" }),
  ).toBeVisible();
  await expect(
    page.getByText("旧权限配置兼容入口", { exact: true }),
  ).toHaveCount(0);
  const catalog = await rpc("query.permission_resources");
  const target = catalog.targets.find(
    (t: any) => t.provider === "codex" && t.supported,
  );
  await page
    .getByLabel("权限目标版本", { exact: true })
    .selectOption(target.target.revision_id);
  await page.getByText("构建、平台与安装登记", { exact: true }).click();
  await expect(
    page
      .locator("details")
      .filter({
        has: page.getByText("构建、平台与安装登记", { exact: true }),
      })
      .locator("pre"),
  ).toContainText("linux/aarch64");
  await page.getByRole("button", { name: "新建权限草稿" }).click();
  const identity = `web-${crypto.randomUUID()}`;
  const policy = {
    filesystem: { mode: "read_only" },
    network: { mode: "enabled" },
    tools: { mode: "all_available" },
    approval: "never",
  };
  await page.getByLabel("统一权限 JSON").fill(
    JSON.stringify({
      identity,
      name: "拒绝未知字段",
      target_kind: "harness",
      policy,
      unexpected: true,
    }),
  );
  await page.getByRole("button", { name: "创建独立权限" }).click();
  await expect(
    page.getByText("未知权限字段：unexpected", { exact: true }),
  ).toBeVisible();
  expect(
    (await rpc("query.permission_resources")).profiles.some(
      (p: any) => p.identity === identity,
    ),
  ).toBe(false);
  await page.getByLabel("统一权限 JSON").fill(
    JSON.stringify({
      identity,
      name: "Web 受限评审",
      target_kind: "harness",
      policy,
    }),
  );
  await page.getByRole("button", { name: "创建独立权限" }).click();
  await expect(
    page.getByText("已创建权限修订，尚未应用绑定。", { exact: true }),
  ).toBeVisible();
  const first = (await rpc("query.permission_resources")).profiles.find(
    (p: any) => p.identity === identity,
  );
  await page.getByLabel("目标资源权限", { exact: true }).selectOption(first.id);
  const eligible = await page
    .getByLabel("目标资源权限", { exact: true })
    .locator("option")
    .evaluateAll((nodes) =>
      nodes.map((n) => (n as HTMLOptionElement).value).filter(Boolean),
    );
  expect(eligible).toContain(first.id);
  const allProfiles = (await rpc("query.permission_resources")).profiles;
  expect(
    allProfiles
      .filter((p: any) => eligible.includes(p.id))
      .every((p: any) => p.target_kind === "harness"),
  ).toBe(true);
  await page.getByRole("button", { name: "预检目标绑定" }).click();
  await expect(
    page.getByText("目标兼容性预检通过，仍需实际执行范围校验。", {
      exact: true,
    }),
  ).toBeVisible();
  await page.getByRole("button", { name: "应用到该目标版本" }).click();
  await expect(page.getByText(/活动绑定已更新/)).toBeVisible();
  const active = async () =>
    (await rpc("query.permission_resources")).targets.find(
      (t: any) => t.target.revision_id === target.target.revision_id,
    ).active;
  const bound = await active();
  await page.getByLabel("统一权限 JSON").fill(
    JSON.stringify({
      ...first,
      name: "Web 新修订",
      policy: { ...policy, network: { mode: "disabled" } },
    }),
  );
  await page.getByRole("button", { name: "保存权限新修订" }).click();
  await expect(
    page.getByText("新修订已保存，原绑定保持不变。", { exact: true }),
  ).toBeVisible();
  expect(await active()).toEqual(bound);
  const download = page.waitForEvent("download");
  await page.getByRole("button", { name: "导出权限", exact: true }).click();
  const exported = JSON.parse(
    readFileSync((await (await download).path())!, "utf8"),
  );
  expect(exported.profiles.map((p: any) => p.version)).toEqual([1, 2]);
  const second = (await rpc("query.permission_resources")).profiles.find(
    (p: any) => p.identity === identity && p.version === 2,
  );
  await page
    .getByLabel("目标资源权限", { exact: true })
    .selectOption(second.id);
  await page.getByRole("button", { name: "应用到该目标版本" }).click();
  await expect
    .poll(async () => (await active()).revision)
    .toBe(bound.revision + 1);
  await page.reload();
  await page.getByRole("button", { name: "设置", exact: true }).click();
  await page
    .getByLabel("权限目标版本", { exact: true })
    .selectOption(target.target.revision_id);
  await page
    .getByLabel("版本化权限修订", { exact: true })
    .selectOption(first.id);
  await page.getByLabel("目标资源权限", { exact: true }).selectOption(first.id);
  await page.getByRole("button", { name: "应用到该目标版本" }).click();
  await expect
    .poll(async () => (await active()).revision)
    .toBe(bound.revision + 2);
  const dataDirectory = (await rpc("permission.source.browse")).path;
  const source = mkdtempSync(join(dataDirectory, "团队 预设-"));
  const file = {
    ...first,
    id: `file-${crypto.randomUUID()}`,
    identity: `file-${crypto.randomUUID()}`,
    name: "运行时新增文件",
  };
  writeFileSync(join(source, "new.json"), JSON.stringify(file));
  await page.getByText("预定义来源与历史", { exact: true }).click();
  await expect(page.getByLabel("新增预设来源目录")).toHaveCount(0);
  await expect(page.getByLabel("当前预设目录")).toHaveText(
    catalog.source.builtin_path,
  );
  await page.getByRole("button", { name: "打开文件夹", exact: true }).click();
  const picker = page.getByRole("dialog", { name: "选择预设文件夹" });
  await expect(picker).toBeVisible();
  await picker.getByRole("button", { name: "数据目录", exact: true }).click();
  await picker
    .getByRole("button", { name: basename(source), exact: true })
    .click();
  await expect(picker.getByLabel("正在浏览的文件夹")).toHaveText(
    realpathSync(source),
  );
  await picker.screenshot({
    path: testInfo.outputPath("preset-directory-picker.png"),
  });
  await page.setViewportSize({ width: 390, height: 844 });
  await picker.screenshot({
    path: testInfo.outputPath("preset-directory-picker-mobile.png"),
  });
  expect((await picker.boundingBox())!.width).toBeLessThanOrEqual(390);
  await page.setViewportSize({ width: 1440, height: 1000 });
  await picker.getByRole("button", { name: "选择此文件夹" }).click();
  await expect(picker).not.toBeVisible();
  await expect(page.getByLabel("当前预设目录")).toHaveText(
    realpathSync(source),
  );
  expect(
    (await rpc("query.permission_resources")).profiles.some(
      (p: any) => p.id === file.id,
    ),
  ).toBe(false);
  await page.getByRole("button", { name: "重新加载预设", exact: true }).click();
  await expect(page.getByText(/预设重新加载完成/)).toBeVisible();
  expect(
    (await rpc("query.permission_resources")).profiles.some(
      (p: any) => p.id === file.id,
    ),
  ).toBe(true);
  expect((await active()).revision).toBe(bound.revision + 2);
  // Selection survives a fresh settings view; cancelling leaves it untouched.
  await page.reload();
  await page.getByRole("button", { name: "设置", exact: true }).click();
  await page.getByText("预定义来源与历史", { exact: true }).click();
  await expect(page.getByLabel("当前预设目录")).toHaveText(
    realpathSync(source),
  );
  await page.getByRole("button", { name: "打开文件夹", exact: true }).click();
  await picker.getByRole("button", { name: "上一级文件夹" }).click();
  await picker.press("Escape");
  await expect(picker).not.toBeVisible();
  await expect(page.getByLabel("当前预设目录")).toHaveText(
    realpathSync(source),
  );
  await page
    .getByRole("button", { name: "重新加载内置预设", exact: true })
    .click();
  await expect(page.getByText(/内置预设重新加载完成/)).toBeVisible();
  await expect(page.getByLabel("当前预设目录")).toHaveText(
    catalog.source.builtin_path,
  );
  expect((await rpc("query.permission_resources")).source.is_builtin).toBe(
    true,
  );
  expect(
    (await rpc("query.permission_resources")).profiles.some(
      (p: any) => p.id === file.id,
    ),
  ).toBe(true);
  expect((await active()).revision).toBe(bound.revision + 2);
  await page.getByLabel("导入权限文件").setInputFiles({
    name: "permission-profiles.json",
    mimeType: "application/json",
    buffer: Buffer.from(JSON.stringify(exported)),
  });
  await expect(
    page.getByText("权限已导入，尚未改变活动绑定。", { exact: true }),
  ).toBeVisible();
  expect(consoleErrors).toEqual([]);
});

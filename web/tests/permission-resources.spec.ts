import { test, expect } from "@playwright/test";
import {
  mkdtempSync,
  readFileSync,
  writeFileSync,
  realpathSync,
} from "node:fs";
import { tmpdir } from "node:os";
import { basename, join } from "node:path";

const ref = (p: any) => `${p.identity}:${p.revision}`;

test("权限定义迁入基础组件，目标绑定与来源历史按组件管理", async ({
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
  ).toHaveCount(0);
  await expect(page.getByLabel("统一权限 JSON")).toHaveCount(0);
  await expect(page.getByLabel("执行授权配置")).toHaveCount(0);
  await expect(
    page.getByText("旧权限配置兼容入口", { exact: true }),
  ).toHaveCount(0);
  const catalog = await rpc("query.permission_resources");
  const target = catalog.targets.find(
    (t: any) => t.provider === "codex" && t.supported,
  );
  async function openHarness() {
    await page.goto(
      `/?tab=components&config_tab=harness&config_object=${encodeURIComponent(target.target.revision_id)}`,
    );
    await expect(
      page.getByRole("region", { name: "Harness 权限绑定" }),
    ).toBeVisible();
  }
  async function openPermissions(profileId?: string) {
    await page.goto("/?tab=components&config_tab=permission");
    await expect(
      page.getByRole("region", { name: "Permission 定义", exact: true }),
    ).toBeVisible();
    if (profileId)
      await page.getByLabel("版本化权限修订").selectOption(profileId);
  }
  await openHarness();
  await page.getByText("构建、平台与安装登记", { exact: true }).click();
  await expect(
    page
      .locator("details")
      .filter({
        has: page.getByText("构建、平台与安装登记", { exact: true }),
      })
      .locator("pre"),
  ).toContainText("linux/aarch64");
  await openPermissions();
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
      type: "permission_profile",
      identity,
      revision: 1,
      scope: "global",
      name: "拒绝未知字段",
      data: { schema_version: 2, target_kind: "harness", policy },
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
      type: "permission_profile",
      identity,
      revision: 1,
      scope: "global",
      name: "Web 受限评审",
      data: { schema_version: 2, target_kind: "harness", policy },
    }),
  );
  await page.getByRole("button", { name: "创建独立权限" }).click();
  await expect(
    page.getByText("已创建权限修订，尚未应用绑定。", { exact: true }),
  ).toBeVisible();
  const first = (await rpc("query.permission_resources")).profiles.find(
    (p: any) => p.identity === identity,
  );
  await openHarness();
  await page
    .getByLabel("目标资源权限", { exact: true })
    .selectOption(ref(first));
  const eligible = await page
    .getByLabel("目标资源权限", { exact: true })
    .locator("option")
    .evaluateAll((nodes) =>
      nodes.map((n) => (n as HTMLOptionElement).value).filter(Boolean),
    );
  expect(eligible).toContain(ref(first));
  const allProfiles = (await rpc("query.permission_resources")).profiles;
  expect(
    allProfiles
      .filter((p: any) => eligible.includes(ref(p)))
      .every((p: any) => p.data.target_kind === "harness"),
  ).toBe(true);
  await page.getByRole("button", { name: "预检目标绑定" }).click();
  await expect(
    page.getByText("目标兼容性预检通过，执行范围由 Profile 解析。", {
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
  await openPermissions(ref(first));
  await page.getByLabel("统一权限 JSON").fill(
    JSON.stringify({
      ...first,
      name: "Web 新修订",
      data: {
        ...first.data,
        policy: { ...policy, network: { mode: "disabled" } },
      },
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
  expect(exported.profiles.map((p: any) => p.revision)).toEqual([1, 2]);
  const second = (await rpc("query.permission_resources")).profiles.find(
    (p: any) => p.identity === identity && p.revision === 2,
  );
  await openHarness();
  await page
    .getByLabel("目标资源权限", { exact: true })
    .selectOption(ref(second));
  await page.getByRole("button", { name: "应用到该目标版本" }).click();
  await expect
    .poll(async () => (await active()).revision)
    .toBe(bound.revision + 1);
  await page.reload();
  await expect(
    page.getByRole("region", { name: "Harness 权限绑定" }),
  ).toBeVisible();
  await page
    .getByLabel("目标资源权限", { exact: true })
    .selectOption(ref(first));
  await page.getByRole("button", { name: "应用到该目标版本" }).click();
  await expect
    .poll(async () => (await active()).revision)
    .toBe(bound.revision + 2);
  await openPermissions(ref(first));
  const dataDirectory = (await rpc("permission.source.browse")).path;
  const source = mkdtempSync(join(dataDirectory, "团队 预设-"));
  const file = {
    ...first,
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
      (p: any) => ref(p) === ref(file),
    ),
  ).toBe(false);
  await page.getByRole("button", { name: "重新加载预设", exact: true }).click();
  await expect(page.getByText(/预设重新加载完成/)).toBeVisible();
  expect(
    (await rpc("query.permission_resources")).profiles.some(
      (p: any) => ref(p) === ref(file),
    ),
  ).toBe(true);
  expect((await active()).revision).toBe(bound.revision + 2);
  // Selection survives a fresh settings view; cancelling leaves it untouched.
  await page.reload();
  await openPermissions(ref(first));
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
      (p: any) => ref(p) === ref(file),
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
  await expect(
    page.getByRole("heading", { name: "目标版本绑定", exact: true }),
  ).toHaveCount(0);
  await expect(
    page.getByText("执行授权（Permission Context）", { exact: true }),
  ).toHaveCount(0);
  await expect(
    page.getByRole("textbox", { name: "执行授权配置", exact: true }),
  ).toHaveCount(0);
});

test("Sandbox 版本独立绑定权限，范围预览固定当前目标并可查看旧版本", async ({
  page,
}, info) => {
  await page.goto("/");
  if (
    (await page.request.get("/capabilities").then((r) => r.json()))
      .setup_required
  ) {
    await page
      .getByLabel("数据目录")
      .fill(mkdtempSync(join(tmpdir(), "devdesk-sandbox-web-")));
    await page.getByRole("button", { name: "初始化 / 打开数据集" }).click();
  }
  const rpc = async (method: string, params: any = {}) => {
    const result = await page.request
      .post("/rpc", { data: { jsonrpc: "2.0", id: 1, method, params } })
      .then((r) => r.json());
    expect(result.error, JSON.stringify(result)).toBeUndefined();
    return result.result;
  };
  await page.goto("/?tab=components&config_tab=sandbox");
  await expect(
    page.getByRole("tablist", { name: "配置类别" }).getByRole("tab"),
  ).toHaveText([
    "Model",
    "Harness",
    "Sandbox",
    "Permission",
    "MCP",
    "Skills",
    "Tools",
    "Harness原生工具",
  ]);
  const editor = page.getByRole("region", {
    name: "Sandbox 管理",
    exact: true,
  });
  await editor
    .getByLabel("名称", { exact: true })
    .fill("浏览器 Sandbox 独立版本");
  await expect(
    editor.getByRole("button", { name: "应用到该目标版本" }),
  ).toHaveCount(0);
  await editor
    .getByRole("button", { name: "创建 Sandbox", exact: true })
    .click();
  const panel = editor.getByRole("region", { name: "Sandbox 权限绑定" });
  await expect(panel).toBeVisible();
  const original = new URL(page.url()).searchParams.get("config_object")!;
  const available = await rpc("query.permission_resources", {
    target_ref: original,
  });
  const compatible = await panel
    .getByLabel("目标资源权限")
    .locator("option")
    .evaluateAll((nodes) =>
      nodes.map((n) => (n as HTMLOptionElement).value).filter(Boolean),
    );
  expect(compatible.length).toBeGreaterThan(0);
  expect(
    available.profiles
      .filter((p: any) => compatible.includes(ref(p)))
      .every((p: any) => p.data.target_kind === "sandbox"),
  ).toBe(true);
  await panel
    .getByLabel("目标资源权限")
    .selectOption("builtin-permission-sandbox-full-access:1");
  await panel
    .getByRole("button", { name: "预检目标绑定", exact: true })
    .click();
  await expect(
    panel.getByText("目标兼容性预检通过，执行范围由 Profile 解析。", {
      exact: true,
    }),
  ).toBeVisible();
  await panel
    .getByRole("button", { name: "应用到该目标版本", exact: true })
    .click();
  await expect(panel.getByText(/活动绑定已更新/)).toBeVisible();
  const before = (
    await rpc("query.permission_resources", { target_ref: original })
  ).targets.find((t: any) => t.target.revision_id === original).active;
  await panel.getByText("实际执行范围预览", { exact: true }).click();
  const harness = available.targets.find(
    (t: any) =>
      t.target.kind === "harness" && t.provider === "codex" && t.supported,
  );
  await panel
    .getByLabel("Harness 组合目标")
    .selectOption(harness.target.revision_id);
  const preview = page.waitForRequest(
    (request) =>
      request.url().endsWith("/rpc") &&
      request.postDataJSON()?.method === "permission.binding.preflight" &&
      !!request.postDataJSON().params.selection,
  );
  await panel.getByRole("button", { name: "预览两层实际权限" }).click();
  expect((await preview).postDataJSON().params).toMatchObject({
    harness_ref: harness.target.revision_id,
    selection: { sandbox_ref: original },
  });
  await expect(
    panel.getByText("完整范围预览通过；正式执行仍由协调器核验并冻结。", {
      exact: true,
    }),
  ).toBeVisible();
  await panel.getByText("目标绑定历史", { exact: true }).click();
  await expect(
    panel.getByText("绑定 v1 · 当前", { exact: true }),
  ).toBeVisible();
  await panel.getByText("权限修订与来源历史", { exact: true }).click();
  await panel.getByText("权限解析结果", { exact: true }).click();
  await page.screenshot({
    path: info.outputPath("sandbox-permission-binding.png"),
    fullPage: true,
  });
  await page.setViewportSize({ width: 600, height: 900 });
  await panel.screenshot({
    path: info.outputPath("sandbox-permission-binding-mobile.png"),
  });
  expect(
    (await panel.boundingBox())!.x + (await panel.boundingBox())!.width,
  ).toBeLessThanOrEqual(600);
  await page.setViewportSize({ width: 1440, height: 1000 });
  await editor
    .getByLabel("名称", { exact: true })
    .fill("浏览器 Sandbox 后继版本");
  await editor
    .getByRole("button", { name: "保存 Sandbox 新版本", exact: true })
    .click();
  await expect(editor.getByLabel("Sandbox 修订")).not.toHaveValue(original);
  await expect(panel.getByText(/当前绑定：尚未分配/)).toBeVisible();
  await expect(panel.getByLabel("目标资源权限")).toHaveValue("");
  await editor.getByLabel("Sandbox 修订").selectOption(original);
  await expect(panel.getByText(/当前绑定：.*revision 1/)).toBeVisible();
  expect(
    (
      await rpc("query.permission_resources", { target_ref: original })
    ).targets.find((t: any) => t.target.revision_id === original).active,
  ).toEqual(before);
  await page.reload();
  await expect(editor.getByLabel("Sandbox 修订")).toHaveValue(original);
  await expect(panel.getByText(/当前绑定：.*revision 1/)).toBeVisible();
  await panel.getByRole("link", { name: "查看 Permission 定义与来源" }).click();
  await expect(page.getByLabel("权限类型", { exact: true })).toHaveValue(
    "sandbox",
  );
  await expect(page.getByLabel("版本化权限修订")).toHaveValue(
    "builtin-permission-sandbox-full-access:1",
  );
  await page.getByRole("button", { name: "设置", exact: true }).click();
  await expect(
    page.getByRole("region", { name: /权限绑定|Permission 定义/ }),
  ).toHaveCount(0);
});

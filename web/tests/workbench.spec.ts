import { test, expect, Page } from "@playwright/test";
import { mkdtempSync } from "node:fs";
import { tmpdir } from "node:os";
import { join } from "node:path";
async function ensureSetup(page: Page) {
  await page.goto("/");
  const capabilities = await page.request
    .get("/capabilities")
    .then((r) => r.json());
  if (capabilities.setup_required) {
    await page
      .getByLabel("数据目录")
      .fill(mkdtempSync(join(tmpdir(), "devdesk-web-")));
    await page.getByRole("button", { name: /初始化/ }).click();
  }
}
async function confirm(page: Page) {
  await page
    .getByRole("dialog")
    .getByRole("button", { name: "确认", exact: true })
    .click();
  await expect(page.getByRole("dialog")).toHaveCount(0);
}
async function createTask(page: Page, title: string, acceptance?: string) {
  await page.getByRole("button", { name: "创建 Task", exact: true }).click();
  await page.getByLabel("Task 名称").fill(title);
  if (acceptance) await page.getByLabel(/实现的验收条目标识/).fill(acceptance);
  await confirm(page);
}
async function passDelivery(page: Page, kind: string) {
  const d = page
    .locator(".delivery")
    .filter({ has: page.getByRole("heading", { name: kind, exact: true }) })
    .first();
  await expect(d).toBeVisible();
  await d.getByRole("button", { name: "通过", exact: true }).click();
  await confirm(page);
  await expect(d.locator(".status-accepted")).toBeVisible();
}
async function mergeTask(page: Page, title: string) {
  const task = page
    .locator(".task-card")
    .filter({ has: page.getByRole("heading", { name: title, exact: true }) });
  await task.getByRole("button", { name: "申请 Merge" }).click();
  const d = page
    .locator(".delivery")
    .filter({
      has: page.getByRole("heading", { name: "Merge 候选", exact: true }),
    })
    .first();
  await expect(d).toBeVisible();
  await d.getByRole("button", { name: "通过", exact: true }).click();
  await confirm(page);
  await page.getByRole("button", { name: "接纳模拟候选", exact: true }).click();
  await expect(task.locator(".status-completed")).toBeVisible();
}
test("完整 Web 闭环、双 Task 并行、Review、mainline、文档及历史", async ({
  page,
}, testInfo) => {
  const errors: string[] = [];
  page.on("pageerror", (e) => errors.push(String(e)));
  await page.goto("/");
  await page
    .getByLabel("数据目录")
    .fill(mkdtempSync(join(tmpdir(), "devdesk-web-")));
  await page.getByRole("button", { name: /初始化/ }).click();
  await expect(
    page.getByRole("heading", { name: "工作总览", exact: true }),
  ).toBeVisible();
  await page.getByRole("button", { name: "新项目", exact: true }).click();
  await page.getByLabel("项目名称").fill("DevDesk E2E");
  await page.getByLabel(/Workspace 路径/).fill("/logical/demo");
  await confirm(page);
  await page.getByRole("button", { name: "新 Feature", exact: true }).click();
  await page.getByLabel("Feature 名称").fill("可追踪的导出流程");
  await confirm(page);
  await page.getByRole("button", { name: "文档与上下文", exact: true }).click();
  await page.getByRole("button", { name: "新建修订" }).click();
  await page.getByLabel("标题", { exact: true }).fill("CSV 导出需求");
  await page
    .getByLabel("Markdown 内容")
    .fill("## 需求\n导出 CSV。\n## 验收条件\n支持空列表和错误路径。");
  await page
    .getByLabel(/需求验收条目/)
    .fill("CSV-EMPTY | 空列表返回表头\nCSV-ERROR | 错误路径可观察");
  await confirm(page);
  await page.getByRole("button", { name: "执行与交付", exact: true }).click();
  await page.getByRole("button", { name: "规划 Attempt", exact: true }).click();
  await confirm(page);
  const planning = page.locator(".attempt").first();
  await expect(
    planning.locator(".status-waiting_review").first(),
  ).toBeVisible();
  await planning
    .getByRole("button", { name: "提交 Requirement", exact: true })
    .click();
  await passDelivery(page, "需求");
  await planning
    .getByRole("button", {
      name: "提交 Design / Task / Test 计划",
      exact: true,
    })
    .click();
  await passDelivery(page, "设计 / Task / Test 计划");
  await createTask(page, "导出接口", "CSV-EMPTY");
  await createTask(page, "错误处理", "CSV-ERROR");
  await expect(
    page.locator(".task-card").filter({ hasText: "导出接口" }),
  ).toContainText("CSV-EMPTY");
  await expect(
    page.locator(".task-card").filter({ hasText: "错误处理" }),
  ).toContainText("CSV-ERROR");
  const tasks = page.locator(".task-card");
  await tasks
    .filter({ hasText: "导出接口" })
    .getByRole("button", { name: "启动 Attempt" })
    .click();
  await confirm(page);
  await tasks
    .filter({ hasText: "错误处理" })
    .getByRole("button", { name: "启动 Attempt" })
    .click();
  await confirm(page);
  await expect(
    page
      .locator(".attempt")
      .filter({ has: page.getByRole("heading", { name: /Task 实现/ }) })
      .locator(".status-waiting_review"),
  ).toHaveCount(2);
  for (const a of await page
    .locator(".attempt")
    .filter({ has: page.getByRole("heading", { name: /Task 实现/ }) })
    .all()) {
    await a.getByRole("button", { name: "提交交付", exact: true }).click();
  }
  await expect(
    page.locator(".delivery").filter({
      has: page.getByRole("heading", { name: "Task 实现", exact: true }),
    }),
  ).toHaveCount(2);
  for (const d of await page
    .locator(".delivery")
    .filter({
      has: page.getByRole("heading", { name: "Task 实现", exact: true }),
    })
    .all()) {
    await d.getByRole("button", { name: "通过", exact: true }).click();
    await confirm(page);
  }
  await mergeTask(page, "导出接口");
  await mergeTask(page, "错误处理");
  await page.getByRole("button", { name: "收尾 Attempt", exact: true }).click();
  await confirm(page);
  const closure = page.locator(".attempt").first();
  await expect(closure.locator(".status-waiting_review").first()).toBeVisible();
  await closure.getByRole("button", { name: "提交交付", exact: true }).click();
  await passDelivery(page, "Feature 收尾");
  await page
    .getByRole("button", { name: "Feature → mainline", exact: true })
    .click();
  await passDelivery(page, "Merge 候选");
  await page.getByRole("button", { name: "接纳模拟候选", exact: true }).click();
  await expect(page.locator(".feature-picker .status-completed")).toBeVisible();
  await page.reload();
  await page.getByRole("button", { name: "执行与交付", exact: true }).click();
  await page
    .getByLabel("当前 Feature", { exact: true })
    .selectOption({ label: "可追踪的导出流程" });
  await expect(page.locator(".feature-picker .status-completed")).toBeVisible();
  await page.getByRole("button", { name: "文档与上下文", exact: true }).click();
  await expect(
    page.getByRole("heading", { name: "Artifact 文件与报告", exact: true }),
  ).toBeVisible();
  await page.getByRole("button", { name: "打开", exact: true }).first().click();
  await expect(page.getByRole("dialog")).toContainText("simulated");
  await page.getByRole("button", { name: "关闭", exact: true }).click();
  await page.getByRole("button", { name: "历史记录", exact: true }).click();
  await expect(page.locator(".history-row")).not.toHaveCount(0);
  await page.getByRole("button", { name: "规范与身份", exact: true }).click();
  await expect(
    page.getByRole("heading", { name: "Harness / Sandbox 能力" }),
  ).toBeVisible();
  await page.getByRole("button", { name: "设置", exact: true }).click();
  await page.getByRole("button", { name: "预览", exact: true }).first().click();
  await expect(page.getByRole("dialog")).toContainText("预览结束");
  await page.getByRole("button", { name: "关闭", exact: true }).click();
  await page.getByRole("button", { name: "工作总览", exact: true }).click();
  await page.screenshot({
    path: testInfo.outputPath("workbench.png"),
    fullPage: true,
  });
  expect(errors).toEqual([]);
});

test("Web 异常回路：技术重试、Review退回、新Attempt、Merge重试与手动继续", async ({
  page,
}) => {
  const errors: string[] = [];
  page.on("pageerror", (e) => errors.push(String(e)));
  await ensureSetup(page);
  await expect(
    page.getByRole("heading", { name: "工作总览", exact: true }),
  ).toBeVisible();
  await page.getByRole("button", { name: "新项目", exact: true }).click();
  await page.getByLabel("项目名称").fill("异常回路");
  await confirm(page);
  await page.getByRole("button", { name: "新 Feature", exact: true }).click();
  await page.getByLabel("Feature 名称").fill("可恢复执行");
  await confirm(page);
  await page.getByRole("button", { name: "执行与交付", exact: true }).click();
  await page.getByRole("button", { name: "规划 Attempt", exact: true }).click();
  await confirm(page);
  let a = page.locator(".attempt").first();
  await expect(a.locator(".status-waiting_review").first()).toBeVisible();
  await a
    .getByRole("button", { name: "提交 Requirement", exact: true })
    .click();
  await passDelivery(page, "需求");
  await a
    .getByRole("button", {
      name: "提交 Design / Task / Test 计划",
      exact: true,
    })
    .click();
  await passDelivery(page, "设计 / Task / Test 计划");
  await createTask(page, "故障任务");
  await page
    .getByLabel("模拟场景", { exact: true })
    .selectOption("technical_failure");
  await page.getByRole("button", { name: "启动 Attempt", exact: true }).click();
  await confirm(page);
  a = page.locator(".attempt").first();
  await expect(a.locator(".status-failed").first()).toBeVisible();
  await a.getByRole("button", { name: "重试", exact: true }).click();
  await expect(a.locator(".status-waiting_review").first()).toBeVisible();
  await a.getByRole("button", { name: "提交交付", exact: true }).click();
  let d = page
    .locator(".delivery")
    .filter({
      has: page.getByRole("heading", { name: "Task 实现", exact: true }),
    })
    .first();
  await d.getByRole("button", { name: "退回", exact: true }).click();
  await page.getByLabel("评审理由").fill("需要修改错误路径说明");
  await confirm(page);
  await expect(a.locator(".status-rejected").first()).toBeVisible();
  await page.getByLabel("模拟场景", { exact: true }).selectOption("success");
  await page.getByRole("button", { name: "启动 Attempt", exact: true }).click();
  await confirm(page);
  a = page.locator(".attempt").first();
  await expect(a.locator(".status-waiting_review").first()).toBeVisible();
  await a.getByRole("button", { name: "提交交付", exact: true }).click();
  d = page
    .locator(".delivery")
    .filter({
      has: page.getByRole("heading", { name: "Task 实现", exact: true }),
    })
    .first();
  await d.getByRole("button", { name: "Agent 评审", exact: true }).click();
  await expect(d.locator(".status-accepted")).toBeVisible();
  await page
    .getByLabel("模拟场景", { exact: true })
    .selectOption("technical_failure");
  await page.getByRole("button", { name: "申请 Merge", exact: true }).click();
  let m = page
    .locator(".delivery")
    .filter({
      has: page.getByRole("heading", { name: "Task → Feature", exact: true }),
    })
    .first();
  await expect(m.locator(".status-failed")).toBeVisible();
  await m.getByRole("button", { name: "重试", exact: true }).click();
  d = page
    .locator(".delivery")
    .filter({
      has: page.getByRole("heading", { name: "Merge 候选", exact: true }),
    })
    .first();
  await expect(d).toBeVisible();
  await d.getByRole("button", { name: "退回", exact: true }).click();
  await page.getByLabel("评审理由").fill("重新构造候选");
  await confirm(page);
  await page.getByLabel("模拟场景", { exact: true }).selectOption("hold");
  await page.getByRole("button", { name: "申请 Merge", exact: true }).click();
  m = page
    .locator(".delivery")
    .filter({
      has: page.getByRole("heading", { name: "Task → Feature", exact: true }),
    })
    .first();
  await m.getByRole("button", { name: "继续下一步", exact: true }).click();
  await expect(
    page.locator(".delivery").filter({
      has: page.getByRole("heading", { name: "Merge 候选", exact: true }),
    }),
  ).toHaveCount(2);
  await passDelivery(page, "Merge 候选");
  await m.getByRole("button", { name: "接纳模拟候选", exact: true }).click();
  await expect(page.locator(".task-card .status-completed")).toBeVisible();
  await page.getByRole("button", { name: "规范与身份", exact: true }).click();
  const standard = page
    .locator(".document-row")
    .filter({ hasText: "工程默认 Standard" })
    .first();
  await standard.getByRole("button", { name: "克隆修订" }).click();
  await page.getByLabel("用途 slots JSON（purpose / spec）").fill("not json");
  await page
    .getByRole("dialog")
    .getByRole("button", { name: "确认", exact: true })
    .click();
  await expect(page.getByRole("dialog").getByRole("alert")).toBeVisible();
  await expect(
    page.getByLabel("用途 slots JSON（purpose / spec）"),
  ).toHaveValue("not json");
  await page
    .getByRole("dialog")
    .getByRole("button", { name: "关闭", exact: true })
    .click();
  expect(errors).toEqual([]);
});

test("详情分页与 revision 冲突保留输入", async ({ page }) => {
  await ensureSetup(page);
  await expect(
    page.getByRole("heading", { name: "工作总览", exact: true }),
  ).toBeVisible();
  let counter = 0;
  const api = async (method: string, params: Record<string, unknown>) => {
    const response = await page.request.post("/rpc", {
      data: { jsonrpc: "2.0", id: ++counter, method, params },
    });
    const body = await response.json();
    expect(body.error).toBeUndefined();
    return body.result;
  };
  const command = async (
    method: string,
    target: string | null,
    payload: Record<string, unknown>,
    revision?: number,
  ) =>
    api(method, {
      command_id: crypto.randomUUID(),
      target,
      expected_revision: revision ?? null,
      payload,
    });
  const pid = (await command("project.create", null, { title: "分页验证" })).id;
  const fid = (
    await command("feature.create", null, {
      project_id: pid,
      title: "超过一页的任务",
    })
  ).id;
  const get = async () => api("query.workspace", { feature_id: fid });
  let state = await get();
  const aid = (
    await command(
      "attempt.start",
      fid,
      { phase: "planning" },
      state.detail.feature.revision,
    )
  ).id;
  await expect
    .poll(async () => {
      state = await get();
      return state.detail.attempts[0].status;
    })
    .toBe("waiting_review");
  for (const kind of ["requirement", "plan"]) {
    await expect
      .poll(async () => {
        state = await get();
        return state.detail.attempts[0].status;
      })
      .toBe("waiting_review");
    state = await get();
    const did = (
      await command(
        "delivery.submit",
        aid,
        { kind },
        state.detail.attempts[0].revision,
      )
    ).id;
    await command("review.submit", did, {
      decision: "pass",
      reason: "测试夹具：正式API完成规划",
    });
  }
  for (let n = 0; n < 51; n++) {
    state = await get();
    await command(
      "task.create",
      fid,
      { title: `分页任务 ${n}` },
      state.detail.feature.revision,
    );
  }
  await page.reload();
  await page.getByLabel("当前项目", { exact: true }).selectOption(pid);
  await page.getByRole("button", { name: "执行与交付", exact: true }).click();
  await page.getByLabel("当前 Feature", { exact: true }).selectOption(fid);
  const panel = page.locator("section.panel").filter({
    has: page.getByRole("heading", { name: "独立 Tasks", exact: true }),
  });
  await expect(panel.locator(".task-card")).toHaveCount(50);
  await panel.getByRole("button", { name: "下一页", exact: true }).click();
  await expect(
    panel.getByRole("heading", { name: "分页任务 50", exact: true }),
  ).toBeVisible();
  await expect(panel.locator(".task-card")).toHaveCount(1);
  await panel
    .getByRole("button", { name: "启动 Attempt", exact: true })
    .click();
  state = await api("query.workspace", {
    feature_id: fid,
    detail_pages: { tasks: 1 },
  });
  const task = state.detail.tasks[0];
  await command("binding.update", task.id, task.binding, task.revision);
  await page
    .getByRole("dialog")
    .getByRole("button", { name: "确认", exact: true })
    .click();
  await expect(page.getByRole("dialog").getByRole("alert")).toContainText(
    "revision_conflict",
  );
  await page
    .getByRole("dialog")
    .getByRole("button", { name: "关闭", exact: true })
    .click();
});

test("持久化清理回执和人工重试可从 Web 操作", async ({ page }) => {
  await ensureSetup(page);
  await page.getByRole("button", { name: "新项目", exact: true }).click();
  await page.getByLabel("项目名称").fill("清理契约");
  await confirm(page);
  await page.getByRole("button", { name: "新 Feature", exact: true }).click();
  await page.getByLabel("Feature 名称").fill("可核对的资源释放");
  await confirm(page);
  await page.getByRole("button", { name: "执行与交付", exact: true }).click();
  await page
    .getByLabel("模拟场景", { exact: true })
    .selectOption("cleanup_failure");
  await page.getByRole("button", { name: "规划 Attempt", exact: true }).click();
  await confirm(page);
  const recovery = page.getByRole("region", { name: "外部操作核对" });
  await recovery.getByRole("button", { name: "重试资源清理" }).first().click();
  const retry = recovery
    .locator(".document-row")
    .filter({ hasText: "retry_of=" })
    .first();
  await expect(retry).toContainText("applied");
  await retry.getByRole("button", { name: "查看回执" }).click();
  await expect(page.getByRole("dialog")).toContainText('"status": "released"');
  await expect(page.getByRole("dialog")).toContainText('"adapter"');
  await page.getByRole("button", { name: "关闭", exact: true }).click();
});

test("打开项目切换独立数据库，重载保持选择和共享配置", async ({ page }) => {
  const errors: string[] = [];
  page.on("pageerror", (e) => errors.push(String(e)));
  await ensureSetup(page);
  await page.getByRole("button", { name: "新项目", exact: true }).click();
  await page.getByLabel("项目名称").fill("隔离项目 A");
  await confirm(page);
  const selector = page.getByLabel("当前项目", { exact: true });
  await expect(selector.locator("option:checked")).toHaveText("隔离项目 A");
  const a = await selector.inputValue();
  await page.getByRole("button", { name: "新 Feature", exact: true }).click();
  await page.getByLabel("Feature 名称").fill("只属于 A 的 Feature");
  await confirm(page);
  await page.getByRole("button", { name: "执行与交付", exact: true }).click();
  await expect(
    page.getByLabel("当前 Feature", { exact: true }).locator("option:checked"),
  ).toHaveText("只属于 A 的 Feature");
  await page.getByRole("button", { name: "工作总览", exact: true }).click();
  await page.getByRole("button", { name: "新项目", exact: true }).click();
  await page.getByLabel("项目名称").fill("隔离项目 B");
  await confirm(page);
  await expect(selector.locator("option:checked")).toHaveText("隔离项目 B");
  const b = await selector.inputValue();
  await page.getByRole("button", { name: "执行与交付", exact: true }).click();
  await expect(
    page.getByLabel("当前 Feature", { exact: true }).locator("option"),
  ).not.toContainText(["只属于 A 的 Feature"]);
  await page.getByRole("button", { name: "工作总览", exact: true }).click();
  await page.getByRole("button", { name: "新 Feature", exact: true }).click();
  await page.getByLabel("Feature 名称").fill("只属于 B 的 Feature");
  await confirm(page);
  await page.getByRole("button", { name: "设置", exact: true }).click();
  await expect(
    page
      .locator(".setting-row")
      .filter({
        has: page.getByRole("heading", { name: "当前项目数据库", exact: true }),
      }),
  ).toContainText(`/projects/${b}/engineering.sqlite3`);
  await expect(
    page
      .locator(".setting-row")
      .filter({
        has: page.getByRole("heading", { name: "全局配置数据库", exact: true }),
      }),
  ).toContainText("global.sqlite3");
  await page.getByRole("button", { name: "调整", exact: true }).click();
  await page.getByLabel("并发额度 1—32").fill("3");
  await confirm(page);
  await selector.selectOption(a);
  await expect(
    page
      .locator(".setting-row")
      .filter({
        has: page.getByRole("heading", { name: "当前项目数据库", exact: true }),
      }),
  ).toContainText(`/projects/${a}/engineering.sqlite3`);
  await expect(
    page.getByText("3 个 Run · 单项目默认 2 个", { exact: true }),
  ).toBeVisible();
  await page.getByRole("button", { name: "执行与交付", exact: true }).click();
  await expect(
    page.getByLabel("当前 Feature", { exact: true }).locator("option"),
  ).toContainText(["选择 Feature", "只属于 A 的 Feature"]);
  await expect(
    page.getByLabel("当前 Feature", { exact: true }).locator("option"),
  ).not.toContainText(["只属于 B 的 Feature"]);
  await page.reload();
  await expect(selector.locator("option:checked")).toHaveText("隔离项目 A");
  await page.getByRole("button", { name: "执行与交付", exact: true }).click();
  await selector.selectOption(b);
  await expect(
    page.getByLabel("当前 Feature", { exact: true }).locator("option"),
  ).toContainText(["选择 Feature", "只属于 B 的 Feature"]);
  expect(errors).toEqual([]);
});

test("真实接入设置偏好、密钥表单与浏览器 OAuth 链接", async ({
  page,
  context,
}) => {
  await ensureSetup(page);
  await page.route("**/rpc", async (route) => {
    const request = route.request().postDataJSON();
    if (request.method === "query.workspace") {
      const response = await route.fetch();
      const body = await response.json();
      body.result.runtime_installations = [
        {
          id: "browser-fixture",
          key: {
            provider: "codex",
            version: "fixture",
            os: "macos",
            arch: "aarch64",
          },
          artifact: { canonical_path: "/fixture/codex" },
        },
      ];
      await route.fulfill({ json: body });
    } else if (request.method === "codex.auth.start") {
      await route.fulfill({
        json: {
          jsonrpc: "2.0",
          id: request.id,
          result: {
            loginId: "fixture-login",
            authUrl: "https://auth.openai.com/authorize?fixture=true",
          },
        },
      });
    } else if (request.method === "credentials.status") {
      await route.fulfill({
        json: {
          jsonrpc: "2.0",
          id: request.id,
          result: { configured: false, storage: "test_vault" },
        },
      });
    } else if (request.method === "credentials.configure") {
      expect(request.params.provider).toBe("dsh");
      await route.fulfill({
        json: {
          jsonrpc: "2.0",
          id: request.id,
          result: { configured: true, storage: "test_vault" },
        },
      });
    } else await route.continue();
  });
  await page.getByRole("button", { name: "设置", exact: true }).click();
  await page.getByLabel("默认 Agent Harness").selectOption("dsh");
  await expect(page.getByLabel("默认 Agent Harness")).toHaveValue("dsh");
  await expect(page.getByLabel("默认 Agent Harness")).toBeEnabled();
  await page.reload();
  await page.getByRole("button", { name: "设置", exact: true }).click();
  await expect(page.getByLabel("默认 Agent Harness")).toHaveValue("dsh");
  const password = page.getByLabel("配置或更换 API key");
  await expect(password).toHaveAttribute("type", "password");
  // Explicit synthetic fixture, intercepted before the host: this never writes a real Keychain item.
  await password.fill("synthetic-browser-fixture");
  await page.getByRole("button", { name: "保存密钥", exact: true }).click();
  await expect(password).toHaveValue("");
  await expect(
    page.getByText("已保存到系统凭据库", { exact: true }),
  ).toBeVisible();
  await expect(
    page.getByRole("button", { name: "登记本机 Codex", exact: true }),
  ).toBeEnabled();
  await context.route("https://auth.openai.com/**", (route) =>
    route.fulfill({ body: "Synthetic OAuth navigation fixture" }),
  );
  const popupPromise = page.waitForEvent("popup");
  await page.getByRole("button", { name: "网页登录", exact: true }).click();
  const popup = await popupPromise;
  await expect(popup).toHaveURL(
    "https://auth.openai.com/authorize?fixture=true",
  );
  await popup.close();
  await page.getByLabel("默认 Agent Harness").selectOption("codex");
});

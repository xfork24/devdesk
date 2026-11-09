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
// Browser fixtures exercise the real-only Web contract without contacting GitHub or a model.
// Actual repository receipts, Git, and provenance are covered by Rust integration tests.
async function githubFixture(page: Page) {
  const response = await page.request.post("/rpc", {
    data: { jsonrpc: "2.0", id: 1, method: "query.workspace", params: {} },
  });
  const base = (await response.json()).result;
  let project: any;
  let operation: any;
  const commands: any[] = [];
  let preparations: any[] = [];
  await page.route("**/capabilities", (route) =>
    route.fulfill({
      json: { setup_required: false, active_project_id: project?.id },
    }),
  );
  await page.route("**/rpc", async (route) => {
    const request = route.request().postDataJSON();
    let result: any;
    let error: any;
    switch (request.method) {
      case "project.open_github":
        commands.push(request);
        if (request.params.payload.repository === "invalid") {
          error = {
            message: "请填写 GitHub owner/repo",
            data: { code: "invalid_params" },
          };
          break;
        }
        if (!project)
          project = {
            id: crypto.randomUUID(),
            title: "浏览器 GitHub 项目",
            execution_mode: "simulation",
            revision: 1,
            concurrency: 2,
            binding: {},
            workspace_definition: {
              repository: "fixture/repository",
              mainline: "main",
            },
          };
        operation = {
          id: "connection-fixture",
          revision: 1,
          action: { kind: "connect" },
          status: "prepared",
          reason: "",
        };
        result = { id: project.id };
        break;
      case "query.workspace":
        result = {
          ...base,
          active_project: project,
          active_project_id: project?.id,
          projects: project ? [project] : [],
          total_projects: project ? 1 : 0,
          features: [],
          total_features: 0,
          detail: null,
          runtime_installations: [],
        };
        break;
      case "query.configurations":
        result = {
          records: [
            {
              id: "configured-analyst",
              kind: "agent",
              name: "配置的需求分析师",
              version: 1,
              scope: { kind: "global" },
              data: {
                role: "analyst",
                model_ref: "configured-model",
                harness_ref: "configured-harness",
                skills: [],
                tools: [],
                permissions: [],
              },
            },
            {
              id: "configured-model",
              kind: "model",
              data: { model: "fixture-model", provider: "openai" },
            },
            {
              id: "configured-harness",
              kind: "harness",
              data: { provider: "codex", runtime_version: "fixture" },
            },
          ],
          total: 3,
        };
        break;
      case "query.repository":
        result = { operations: [operation], worktrees: [] };
        break;
      case "runtime.preparation_status":
        result = { preparations };
        break;
      case "session.list":
        result = { sessions: [] };
        break;
      case "session.open":
        commands.push(request);
        error = {
          message: "请先完成 Codex 官方网页登录",
          data: { code: "authentication_required" },
        };
        break;
      case "repository.reconcile":
        commands.push(request);
        operation = {
          ...operation,
          status: "needs_reconciliation",
          reason: "reconcile_requested",
        };
        result = { id: operation.id };
        break;
      default:
        await route.continue();
        return;
    }
    await route.fulfill({
      json: {
        jsonrpc: "2.0",
        id: request.id,
        ...(error ? { error } : { result }),
      },
    });
  });
  return {
    commands,
    ready: () => {
      project.execution_mode = "live";
      operation.status = "completed";
    },
    fail: () => {
      operation.status = "needs_reconciliation";
      operation.reason = "GitHub 账号没有仓库写权限";
    },
    preparing: (status: string, message: string) => {
      preparations = [{ provider: "dsh", status, message }];
    },
  };
}
async function openGitHub(page: Page) {
  await page
    .getByRole("button", { name: "打开 GitHub 项目", exact: true })
    .last()
    .click();
  await page
    .getByLabel("GitHub 仓库（owner/repo 或 URL）")
    .fill("fixture/repository");
  await confirm(page);
}

test("打开 GitHub 项目自动接入，完成前阻止工程操作，重载保留仓库", async ({
  page,
}, testInfo) => {
  await ensureSetup(page);
  const fixture = await githubFixture(page);
  await page.reload();
  await openGitHub(page);
  await expect(
    page.getByRole("heading", { name: "GitHub 仓库", exact: true }),
  ).toBeVisible();
  await expect(
    page.getByRole("button", { name: "新 Feature", exact: true }),
  ).toBeDisabled();
  expect(fixture.commands[0].method).toBe("project.open_github");
  expect(fixture.commands[0].params.payload).toMatchObject({
    repository: "fixture/repository",
    mainline: "main",
  });
  await expect(page.getByText(/Simulation mode|模拟项目|模拟场景/)).toHaveCount(
    0,
  );
  fixture.ready();
  await expect(page.getByText("GitHub 已接入", { exact: true })).toBeVisible();
  await expect(
    page.getByRole("button", { name: "新 Feature", exact: true }),
  ).toBeEnabled();
  await page.reload();
  await expect(page.getByLabel("当前项目", { exact: true })).toContainText(
    "浏览器 GitHub 项目",
  );
  await page.screenshot({
    path: testInfo.outputPath("github-workbench.png"),
    fullPage: true,
  });
});

test("仓库输入错误保留表单，失败接入可核对重试", async ({ page }) => {
  await ensureSetup(page);
  const fixture = await githubFixture(page);
  await page.reload();
  await page
    .getByRole("button", { name: "打开 GitHub 项目", exact: true })
    .last()
    .click();
  const input = page.getByLabel("GitHub 仓库（owner/repo 或 URL）");
  await input.fill("invalid");
  await page
    .getByRole("dialog")
    .getByRole("button", { name: "确认", exact: true })
    .click();
  await expect(page.getByRole("dialog").getByRole("alert")).toContainText(
    "invalid_params",
  );
  await expect(input).toHaveValue("invalid");
  await input.fill("fixture/repository");
  await confirm(page);
  fixture.fail();
  await expect(
    page.getByText("GitHub 账号没有仓库写权限", { exact: true }),
  ).toBeVisible();
  await expect(
    page.getByText("仓库接入未完成，请根据下方原因核对后重试", { exact: true }),
  ).toBeVisible();
  await page
    .getByRole("button", { name: "核对并重试原操作", exact: true })
    .click();
  await expect
    .poll(() =>
      fixture.commands.some((c) => c.method === "repository.reconcile"),
    )
    .toBe(true);
  await expect(
    page.getByText("仓库接入 · 等待后台核对", { exact: true }),
  ).toBeVisible();
  await expect(
    page.getByRole("button", { name: "核对并重试原操作", exact: true }),
  ).toBeDisabled();
  await expect(page.locator("body")).not.toContainText("reconcile_requested");
});

test("完整 Agent 无需手工登记安装，显示准备及失败原因", async ({ page }) => {
  await ensureSetup(page);
  const fixture = await githubFixture(page);
  await page.reload();
  await openGitHub(page);
  fixture.ready();
  await page.reload();
  await page.getByRole("button", { name: "角色讨论", exact: true }).click();
  await expect(page.getByLabel("讨论角色", { exact: true })).toHaveValue(
    "configured-analyst",
  );
  await expect(
    page.getByRole("button", { name: "开始 / 续用讨论", exact: true }),
  ).toBeEnabled();
  fixture.preparing("preparing", "正在构建所选 Harness 的共享运行时");
  await expect(
    page.getByRole("status").filter({ hasText: "正在构建" }),
  ).toBeVisible();
  await page
    .getByRole("button", { name: "开始 / 续用讨论", exact: true })
    .click();
  await expect(page.getByRole("alert")).toContainText(
    "authentication_required",
  );
  const command = fixture.commands.find((c) => c.method === "session.open");
  expect(command.params.payload.agent).toBe("configured-analyst");
  expect(command.params.payload).not.toHaveProperty("model");
  expect(command.params.payload).not.toHaveProperty("provider");
  expect(command.params.payload).not.toHaveProperty("scenario");
  fixture.preparing("failed", "Docker 连接失败，请启动 Docker 后重试");
  await expect(
    page.getByRole("status").filter({ hasText: "Docker 连接失败" }),
  ).toBeVisible();
  await page.getByRole("button", { name: "设置", exact: true }).click();
  await expect(
    page.getByRole("button", { name: /登记.*镜像|登记本机/ }),
  ).toHaveCount(0);
  await expect(
    page.getByText("尚未使用 Harness；首次执行时自动准备", { exact: true }),
  ).toBeVisible();
});

test("后端保留的模拟项目不进入 Web 项目列表和工程内容", async ({ page }) => {
  await ensureSetup(page);
  const response = await page.request.post("/rpc", {
    data: {
      jsonrpc: "2.0",
      id: 1,
      method: "project.create",
      params: {
        command_id: crypto.randomUUID(),
        payload: { title: "仅后端的历史演示项目" },
      },
    },
  });
  expect((await response.json()).error).toBeUndefined();
  await page.reload();
  await expect(page.getByLabel("当前项目", { exact: true })).not.toContainText(
    "仅后端的历史演示项目",
  );
  await expect(
    page.getByRole("button", { name: "新 Feature", exact: true }),
  ).toBeDisabled();
  for (const name of ["工作总览", "执行与交付", "规范与身份", "设置"]) {
    await page.getByRole("button", { name, exact: true }).click();
    await expect(page.locator("body")).not.toContainText(
      /Simulation mode|模拟项目|模拟场景|origin=simulated|sim-harness|模拟工程执行身份/,
    );
  }
});

test("真实接入设置偏好、密钥表单与浏览器 OAuth 链接", async ({
  page,
  context,
}) => {
  await ensureSetup(page);
  let loginStarts = 0;
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
      if (++loginStarts === 1) {
        await route.fulfill({
          json: {
            jsonrpc: "2.0",
            id: request.id,
            error: {
              message: "认证请求超时",
              data: { code: "needs_reconciliation" },
            },
          },
        });
        return;
      }
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
  ).toHaveCount(0);
  await expect(
    page.getByRole("button", { name: "网页登录", exact: true }),
  ).toBeEnabled();
  await context.route("https://auth.openai.com/**", (route) =>
    route.fulfill({ body: "Synthetic OAuth navigation fixture" }),
  );
  await page.getByRole("button", { name: "网页登录", exact: true }).click();
  await expect(
    page.getByText(/无法开始登录，可再次点击重试.*needs_reconciliation/),
  ).toBeVisible();
  await expect(
    page.getByRole("button", { name: "网页登录", exact: true }),
  ).toBeEnabled();
  const popupPromise = page.waitForEvent("popup");
  await page.getByRole("button", { name: "网页登录", exact: true }).click();
  const popup = await popupPromise;
  await expect(popup).toHaveURL(
    "https://auth.openai.com/authorize?fixture=true",
  );
  await popup.close();
  await page.reload();
  await page.getByRole("button", { name: "设置", exact: true }).click();
  await page.evaluate(() => {
    window.open = () => null;
  });
  await page.getByRole("button", { name: "网页登录", exact: true }).click();
  await expect(
    page.getByRole("link", { name: "打开官方授权网页", exact: true }),
  ).toHaveAttribute("href", "https://auth.openai.com/authorize?fixture=true");
  await expect(page.getByText(/浏览器未打开新窗口/)).toBeVisible();
  await page.getByLabel("默认 Agent Harness").selectOption("codex");
  await expect(page.getByLabel("默认 Agent Harness")).toBeEnabled();
  // SSE can refresh the workspace while the final settings request completes.
  // Drain intercepted requests before Playwright disposes their API responses.
  await page.unrouteAll({ behavior: "wait" });
});

test("Permission 原生配置预检、修订与重载保持绑定", async ({
  page,
}, testInfo) => {
  await ensureSetup(page);
  await page.getByRole("button", { name: "设置", exact: true }).click();
  const panel = page
    .locator(".setting-row")
    .filter({
      has: page.getByRole("heading", {
        name: "运行权限 Permission",
        exact: true,
      }),
    });
  await expect(panel.getByLabel("权限修订", { exact: true })).toHaveValue(
    "permission-container-full-access-v1",
  );
  await panel
    .getByLabel("权限修订", { exact: true })
    .selectOption("permission-read-only-v1");
  await panel
    .getByRole("button", { name: "预览最终权限", exact: true })
    .click();
  await expect(
    panel.getByText("预检通过，未启动模型或容器。", { exact: true }),
  ).toBeVisible();
  await panel
    .getByRole("button", { name: "保存权限绑定", exact: true })
    .click();
  await expect(panel.getByText(/权限已保存，后续会话使用新配置/)).toBeVisible();
  await page.reload();
  await page.getByRole("button", { name: "设置", exact: true }).click();
  await expect(panel.getByLabel("权限修订", { exact: true })).toHaveValue(
    "permission-read-only-v1",
  );
  await panel
    .getByLabel("权限字段覆盖", { exact: true })
    .fill(
      JSON.stringify({
        network: { mode: "allowlist", destinations: ["example.com"] },
      }),
    );
  await panel
    .getByRole("button", { name: "预览最终权限", exact: true })
    .click();
  await expect(
    panel
      .getByRole("status")
      .filter({ hasText: /permission_unsupported|需要受控代理/ }),
  ).toBeVisible();
  await panel.getByText("创建或修订可复用权限", { exact: true }).click();
  await panel.getByLabel("新权限名称", { exact: true }).fill("浏览器权限定义");
  await panel
    .getByLabel("新权限策略", { exact: true })
    .fill(
      JSON.stringify({
        filesystem: { mode: "read_only" },
        network: { mode: "enabled" },
        tools: { mode: "all_available" },
      }),
    );
  await panel.getByRole("button", { name: "创建权限", exact: true }).click();
  await expect(
    panel
      .getByLabel("权限修订", { exact: true })
      .locator("option")
      .filter({ hasText: "浏览器权限定义" }),
  ).toHaveCount(1);
  await panel.getByLabel("权限字段覆盖", { exact: true }).fill("{}");
  await panel
    .getByLabel("权限修订", { exact: true })
    .selectOption("permission-container-full-access-v1");
  await panel
    .getByRole("button", { name: "保存权限绑定", exact: true })
    .click();
  await expect(panel.getByText(/权限已保存，后续会话使用新配置/)).toBeVisible();
  await page.screenshot({
    path: testInfo.outputPath(
      `permission-settings-${process.env.DEVDESK_E2E_PROFILE ?? "debug"}.png`,
    ),
    fullPage: true,
  });
});

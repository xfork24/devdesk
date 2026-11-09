import { test, expect, type Page, type Locator } from "@playwright/test";
import { mkdtempSync } from "node:fs";
import { tmpdir } from "node:os";
import { join } from "node:path";
async function setup(page: Page) {
  await page.goto("/");
  if ((await page.request.get("/capabilities").then((r) => r.json())).setup_required) {
    await page
      .getByLabel("数据目录")
      .fill(mkdtempSync(join(tmpdir(), "devdesk-usability-")));
    await page.getByRole("button", { name: "初始化 / 打开数据集" }).click();
  }
}
async function rpc(page: Page, method: string, params = {}) {
  const value = await page.request
    .post("/rpc", { data: { jsonrpc: "2.0", id: 1, method, params } })
    .then((r) => r.json());
  expect(value.error, JSON.stringify(value)).toBeUndefined();
  return value.result;
}
async function near(button: Locator, text: string) {
  const box = button.locator("..");
  const feedback = box.locator(".action-feedback");
  await expect(feedback).toContainText(text);
  const b = await button.boundingBox(),
    f = await feedback.boundingBox();
  expect(f!.y - (b!.y + b!.height)).toBeLessThan(20);
  expect(f!.y).toBeGreaterThanOrEqual(b!.y + b!.height);
}
test("反馈紧邻触发按钮，新建入口位于左侧顶部，运行版本来自目录", async ({
  page,
}, info) => {
  await setup(page);
  await page.route("**/rpc", async (route) => {
    const c = route.request().postDataJSON();
    if (c.method === "codex.auth.status")
      return route.fulfill({
        json: { jsonrpc: "2.0", id: c.id, result: { authenticated: true } },
      });
    return route.continue();
  });
  await page.getByRole("button", { name: "设置", exact: true }).click();
  const login = page.getByRole("button", { name: "检查登录状态", exact: true });
  await login.click();
  await near(login, "Codex OAuth 已登录");
  await page.screenshot({
    path: info.outputPath("login-feedback.png"),
    fullPage: true,
  });
  for (const [nav, tabs] of [
    ["基础组件", ["Model", "Harness"]],
    ["流程与规范", ["Spec", "Standard"]],
    ["角色与 Agent", ["Role", "Capability", "Agent"]],
  ] as const) {
    await page.getByRole("button", { name: nav, exact: true }).click();
    for (const tab of tabs) {
      await page.getByRole("tab", { name: tab, exact: true }).click();
      await expect(
        page.locator(".configuration-list").getByRole("button").first(),
      ).toHaveText(`新建 ${tab}`);
      if (tab === "Harness") {
        const version = page.getByRole("combobox", {
          name: "固定运行版本",
          exact: true,
        });
        await expect(version).toBeVisible();
        await expect(version.locator("option")).toContainText([
          "请选择支持的运行版本",
          "0.167.0",
        ]);
        await page
          .getByLabel("运行时后端", { exact: true })
          .selectOption("dsh");
        await expect(version).toHaveValue("0.2.2-rc.1");
      }
    }
  }
  await page.getByRole("button", { name: "基础组件", exact: true }).click();
  await page.getByRole("tab", { name: "Harness原生工具", exact: true }).click();
  await expect(
    page.getByRole("heading", { name: "DSH 原生工具组", exact: true }),
  ).toBeVisible();
  const catalog = await rpc(page, "query.configurations", {
    limit: 100,
    include_history: true,
  });
  const prose = catalog.records.filter(
    (r: any) =>
      ["spec", "standard", "role", "skill"].includes(r.kind) &&
      r.definition_id.startsWith("builtin-") &&
      !r.definition_id.includes("stage-skill"),
  );
  for (const row of prose.filter(
    (r: any) =>
      r.kind !== "skill" ||
      /^builtin-skill-(engineering-context|engineering-method|analyst|architect|developer|tester|reviewer)$/.test(
        r.definition_id,
      ),
  )) {
    const body = row.data.body || row.data.instructions;
    expect(body.length, row.id).toBeGreaterThan(350);
    expect((body.match(/^## /gm) || []).length, row.id).toBeGreaterThanOrEqual(
      6,
    );
  }
});
test("全局角色讨论按 AgentID 确定经验归属，无重复经验范围选择", async ({
  page,
}, info) => {
  await setup(page);
  await page.getByRole("button", { name: "角色讨论", exact: true }).click();
  await page.getByLabel("讨论范围", { exact: true }).selectOption("global");
  await expect(page.getByText("新实例经验范围", { exact: true })).toHaveCount(
    0,
  );
  const create = page.getByRole("button", {
    name: "创建全局 AgentID",
    exact: true,
  });
  await expect(create).toBeEnabled();
  await create.click();
  await expect(page.getByLabel("AgentID", { exact: true })).not.toHaveValue("");
  await expect(
    page.getByText(
      "该 AgentID 的经验范围：全局 / 跨项目；已有工作按此身份保留。",
      { exact: true },
    ),
  ).toBeVisible();
  await expect(
    page.getByRole("button", { name: "开始 / 续用讨论", exact: true }),
  ).toBeEnabled();
  await page.screenshot({
    path: info.outputPath("global-discussion.png"),
    fullPage: true,
  });
});
test("创建 Feature 提供全部同项目的前序选项，并说明关联语义", async ({
  page,
}) => {
  await setup(page);
  const base = await rpc(page, "query.workspace");
  const pid = crypto.randomUUID();
  const features = Array.from({ length: 125 }, (_, i) => ({
    id: crypto.randomUUID(),
    title: `同项目 Feature ${i + 1}`,
    status: "planning",
    project_id: pid,
    revision: 1,
  }));
  await page.route("**/capabilities", (route) =>
    route.fulfill({
      json: {
        setup_required: false,
        active_project_id: pid,
        protocol_version: 1,
      },
    }),
  );
  await page.route("**/rpc", async (route) => {
    const c = route.request().postDataJSON();
    if (c.method !== "query.workspace") return route.continue();
    const p = {
      id: pid,
      title: "Fixture Project",
      execution_mode: "live",
      revision: 1,
      workspace_definition: { repository: "fixture/repo", mainline: "main" },
    };
    const offset = c.params.offset || 0,
      limit = c.params.limit || 50;
    return route.fulfill({
      json: {
        jsonrpc: "2.0",
        id: c.id,
        result: {
          ...base,
          active_project: p,
          active_project_id: pid,
          projects: [p],
          features: features.slice(offset, offset + limit),
          total_features: features.length,
          repository_ready: true,
          configuration_ready: true,
          configurations: [
            {
              kind: "project_configuration",
              id: "fixture-config",
              version: 1,
              data: {},
            },
          ],
        },
      },
    });
  });
  await page.goto("/?tab=overview");
  await page.getByRole("button", { name: "新 Feature", exact: true }).click();
  const select = page
    .getByRole("dialog")
    .getByRole("combobox", { name: "关联前序 Feature（可选）", exact: true });
  await expect(select.locator("option")).toHaveCount(126);
  await select.selectOption(features[124].id);
  await expect(
    page.getByText(
      "用于记录同项目 Feature 的前后关联；新 Feature 的代码基线仍为项目主干。",
      { exact: true },
    ),
  ).toBeVisible();
});

test("运行时偏好只保存明确安装和模型，Codex 与已配置 DSH 模型目录可选择", async ({
  page,
}, info) => {
  await setup(page);
  const base = await rpc(page, "query.workspace");
  const pid = crypto.randomUUID();
  let project: any = {
    id: pid,
    title: "Runtime fixture",
    execution_mode: "live",
    revision: 1,
    workspace_definition: { repository: "fixture/repo", mainline: "main" },
  };
  const installation = (id: string, provider: string, os = "linux") => ({
    id,
    key: {
      provider,
      version: provider === "codex" ? "0.167.0" : "0.2.2-rc.1",
      os,
      arch: "aarch64",
    },
    artifact: {
      kind: os === "linux" ? "docker_image" : "executable",
      image_id: "fixture",
    },
    execution_supported: os === "linux",
    usage_reason:
      os === "macos"
        ? "本机 Codex 用于登录及模型目录；项目执行使用隔离的 Docker worker"
        : null,
  });
  const installs = [
    installation("codex-docker", "codex"),
    installation("codex-native", "codex", "macos"),
    installation("dsh-docker", "dsh"),
  ];
  const saved: any[] = [];
  await page.route("**/capabilities", (route) =>
    route.fulfill({
      json: {
        setup_required: false,
        active_project_id: pid,
        protocol_version: 1,
      },
    }),
  );
  await page.route("**/rpc", async (route) => {
    const c = route.request().postDataJSON();
    let result: any;
    if (c.method === "query.workspace")
      result = {
        ...base,
        active_project: project,
        active_project_id: pid,
        projects: [project],
        features: [],
        repository_ready: true,
        configuration_ready: true,
        runtime_installations: installs,
      };
    else if (c.method === "query.runtime_options")
      result = {
        default_harness: "codex",
        host_os: "macos",
        host_arch: "aarch64",
        installations: installs,
        recipes: [
          { provider: "codex", version: "0.167.0", arch: "aarch64" },
          { provider: "dsh", version: "0.2.2-rc.1", arch: "aarch64" },
        ],
        model_providers: [
          {
            id: "deepseek",
            label: "DeepSeek 官方 API",
            configured: true,
            available: true,
            auth_kind: "api_key",
          },
          {
            id: "zai-coding-cn",
            label: "智谱 Coding 国内",
            configured: true,
            available: true,
            auth_kind: "api_key",
          },
        ],
      };
    else if (c.method === "codex.auth.models")
      result = {
        models: [{ model: "gpt-6.1-sol", displayName: "GPT-6.1 Sol" }],
      };
    else if (c.method === "models.list")
      result = {
        source:
          c.params.model_provider === "deepseek"
            ? "provider_endpoint"
            : "pinned_catalog",
        models: [
          {
            id:
              c.params.model_provider === "deepseek"
                ? "deepseek-v4-pro"
                : "glm-5.3",
            context_window: 1000000,
            max_output_tokens: 131072,
          },
        ],
      };
    else if (c.method === "project.update") {
      saved.push(c.params.payload.runtime_preferences);
      project = {
        ...project,
        ...c.params.payload,
        revision: project.revision + 1,
      };
      result = { id: pid };
    } else if (c.params.project_id === pid) {
      const params = { ...c.params };
      delete params.project_id;
      delete params.local;
      const response = await route.fetch({
        postData: JSON.stringify({ ...c, params }),
      });
      return route.fulfill({ response });
    } else return route.continue();
    return route.fulfill({ json: { jsonrpc: "2.0", id: c.id, result } });
  });
  await page.goto("/?tab=settings");
  const harness = page.getByLabel("项目角色 Harness", { exact: true });
  await expect(harness.locator("option").first()).toContainText(
    "系统默认Harness",
  );
  const installsSelect = page.getByLabel("项目角色安装", { exact: true });
  await expect(
    installsSelect.locator('option[value="codex-native"]'),
  ).toBeDisabled();
  await expect(installsSelect).not.toContainText("按需准备并复用运行时");
  await installsSelect.selectOption("codex-docker");
  const model = page.getByRole("combobox", {
    name: "项目角色模型",
    exact: true,
  });
  await expect(model.locator('option[value="gpt-6.1-sol"]')).toHaveCount(1);
  await model.selectOption("gpt-6.1-sol");
  await page.getByRole("button", { name: "保存偏好", exact: true }).click();
  expect(saved[0].default).toMatchObject({
    provider: "codex",
    use_system_default: true,
    installation_id: "codex-docker",
    model: "gpt-6.1-sol",
    model_provider: "openai",
  });
  await harness.selectOption("dsh");
  await installsSelect.selectOption("dsh-docker");
  await expect(model.locator('option[value="deepseek-v4-pro"]')).toHaveCount(1);
  await page
    .getByLabel("偏好模型服务商", { exact: true })
    .selectOption("zai-coding-cn");
  await expect(model.locator('option[value="glm-5.3"]')).toHaveCount(1);
  await model.selectOption("glm-5.3");
  const save = page.getByRole("button", { name: "保存偏好", exact: true });
  await save.click();
  await near(save, "偏好已保存");
  expect(saved[1].default).toMatchObject({
    provider: "dsh",
    use_system_default: false,
    installation_id: "dsh-docker",
    model: "glm-5.3",
    model_provider: "zai-coding-cn",
    context_window: 1000000,
    max_output_tokens: 131072,
  });
  await page.screenshot({
    path: info.outputPath("explicit-runtime-models.png"),
    fullPage: true,
  });
});

test("切换 AgentID 后清空原会话，历史按该身份查询", async ({ page }) => {
  await setup(page);
  await page.getByRole("button", { name: "角色讨论", exact: true }).click();
  await page.getByLabel("讨论范围", { exact: true }).selectOption("global");
  const create = page.getByRole("button", {
    name: "创建全局 AgentID",
    exact: true,
  });
  const identity = page.getByLabel("AgentID", { exact: true });
  await expect(create).toBeEnabled();
  const previous = await identity.inputValue();
  await create.click();
  await expect(identity).not.toHaveValue(previous);
  const first = await identity.inputValue();
  const agent = await page.getByLabel("讨论角色", { exact: true }).inputValue();
  const requests: any[] = [];
  const sessions: any[] = [];
  await page.route("**/rpc", async (route) => {
    const c = route.request().postDataJSON();
    let result: any;
    if (c.method === "session.open") {
      requests.push(c);
      let session = sessions.find(
        (s) =>
          s.runtime_binding.agent_configuration.agent_instance_id ===
          c.params.payload.agent_instance_id,
      );
      if (!session) {
        session = {
          id: crypto.randomUUID(),
          owner_id: "global",
          scope: "global",
          agent,
          revision: 1,
          lifecycle: "idle",
          runtime_binding: {
            installation: { key: { provider: "codex" } },
            agent_configuration: {
              agent_instance_id: c.params.payload.agent_instance_id,
            },
            model: "gpt-6.1-sol",
            work_type: "discussion",
          },
        };
        sessions.push(session);
      }
      result = { id: session.id };
    } else if (c.method === "query.sessions" && c.params.scope === "global") {
      requests.push(c);
      const selected = sessions.filter(
        (s) =>
          !c.params.agent_instance_id ||
          s.runtime_binding.agent_configuration.agent_instance_id ===
            c.params.agent_instance_id,
      );
      result = { sessions: selected, total: selected.length };
    } else if (c.method === "query.session" && c.params.scope === "global") {
      const session = sessions.find((s) => s.id === c.params.id);
      result = {
        session,
        events: c.params.cursor
          ? []
          : [
              {
                id: "answer",
                kind: "assistant",
                content:
                  session.runtime_binding.agent_configuration
                    .agent_instance_id === first
                    ? "第一个身份的已完成工作"
                    : "第二个身份的新会话",
                mode: "full",
              },
            ],
        next_cursor: 1,
        turns: [],
        executions: [],
        interactions: [],
        proposals: [],
      };
    } else return route.continue();
    return route.fulfill({ json: { jsonrpc: "2.0", id: c.id, result } });
  });
  const open = page.getByRole("button", {
    name: "开始 / 续用讨论",
    exact: true,
  });
  await open.click();
  await expect(
    page.getByText("第一个身份的已完成工作", { exact: true }),
  ).toBeVisible();
  await create.click();
  await expect(identity).not.toHaveValue(first);
  const second = await identity.inputValue();
  await expect(
    page.getByText("第一个身份的已完成工作", { exact: true }),
  ).toHaveCount(0);
  await expect(page.getByLabel("讨论会话", { exact: true })).toHaveValue("");
  await expect
    .poll(() =>
      requests.some(
        (r) =>
          r.method === "query.sessions" &&
          r.params.agent_instance_id === second,
      ),
    )
    .toBe(true);
  await open.click();
  await expect(
    page.getByText("第二个身份的新会话", { exact: true }),
  ).toBeVisible();
  expect(
    requests.filter((r) => r.method === "session.open").at(-1).params.payload
      .agent_instance_id,
  ).toBe(second);
  await expect(identity).toHaveValue(second);
});

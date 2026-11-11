import { test, expect, type Page, type Locator } from "@playwright/test";
import { mkdtempSync } from "node:fs";
import { tmpdir } from "node:os";
import { join } from "node:path";
import { useMockCodexCatalog } from "./support/model-catalog";
useMockCodexCatalog();
async function setup(page: Page) {
  await page.goto("/");
  if (
    (await page.request.get("/capabilities").then((r) => r.json()))
      .setup_required
  ) {
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
async function createGlobalAgent(page: Page, name: string) {
  const defaults = await rpc(page, "query.configuration_defaults");
  const definition = defaults.records.find((r: any) => r.id === defaults.role_agent_bindings["builtin-role-analyst:1"]);
  const created = await rpc(page, "agent.instance.create", {
    command_id: crypto.randomUUID(), payload: { name, scope: {kind:"global"}, agent_revision_id:definition.id },
  });
  return { instance: created.id, agent: definition.id };
}
async function rowFeedback(button: Locator, text: string) {
  const box = button.locator("..");
  const feedback = box.locator(".action-feedback");
  await expect(feedback).toContainText(text);
  const b = await button.boundingBox(),
    f = await feedback.boundingBox();
  expect(f!.y - (b!.y + b!.height)).toBeLessThan(20);
  expect(f!.y).toBeGreaterThanOrEqual(b!.y + b!.height);
  const rowLeft = await button.evaluate((element) => {
    let row = element.parentElement!;
    while (getComputedStyle(row).display === "contents") row = row.parentElement!;
    return row.getBoundingClientRect().left + parseFloat(getComputedStyle(row).paddingLeft);
  });
  expect(Math.abs(f!.x - rowLeft)).toBeLessThan(1);
  const style = await feedback.evaluate((element) => ({ background: getComputedStyle(element).backgroundColor, border: getComputedStyle(element).borderTopWidth, padding: getComputedStyle(element).paddingLeft }));
  expect(style).toEqual({ background: "rgba(0, 0, 0, 0)", border: "0px", padding: "0px" });
}
test("反馈位于当前行左侧且无背景框，新建入口位于左侧顶部", async ({
  page,
}, info) => {
  await setup(page);
  await page.route("**/rpc", async (route) => {
    const c = route.request().postDataJSON();
    if (c.method === "codex.auth.status")
      return route.fulfill({
        json: { jsonrpc: "2.0", id: c.id, result: { authenticated: true } },
      });
    return route.fallback();
  });
  await page.getByRole("button", { name: "设置", exact: true }).click();
  const login = page.getByRole("button", { name: "检查登录状态", exact: true });
  await login.click();
  await rowFeedback(login, "Codex OAuth 已登录");
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
      if (tab === "Standard") {
        await expect(page.getByText("从 Workflow 生成快照", { exact: true })).toBeVisible();
        await expect(page.getByRole("button", {name:"添加覆盖规则"})).toHaveCount(0);
        continue;
      }
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

});
test("全局角色讨论按 Agent 确定经验归属，无重复经验范围选择", async ({
  page,
}, info) => {
  await setup(page);
  const created = await createGlobalAgent(page, "Global agent " + crypto.randomUUID());
  await page.getByRole("button", { name: "角色讨论", exact: true }).click();
  await page.getByLabel("讨论范围", { exact: true }).selectOption("global");
  await page.getByLabel("讨论角色", {exact:true}).selectOption(created.agent);
  await page.getByLabel("Agent", { exact: true }).selectOption(created.instance);
  await expect(page.getByText("新实例经验范围", {exact:true})).toHaveCount(0);
  await expect(page.getByRole("button", {name:/创建.*Agent/})).toHaveCount(0);
  await expect(
    page.getByText(
      "该 Agent 的经验范围：全局 / 跨项目；已有工作按此身份保留。",
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
    if (c.method !== "query.workspace") return route.fallback();
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


test("切换 Agent 后清空原会话，历史按该身份查询", async ({ page }) => {
  await setup(page);
  const firstCreated = await createGlobalAgent(page, "First agent " + crypto.randomUUID());
  const secondCreated = await createGlobalAgent(page, "Second agent " + crypto.randomUUID());
  await page.getByRole("button", { name: "角色讨论", exact: true }).click();
  await page.getByLabel("讨论范围", { exact: true }).selectOption("global");
  await page.getByLabel("讨论角色", {exact:true}).selectOption(firstCreated.agent);
  const identity = page.getByLabel("Agent", { exact: true });
  await identity.selectOption(firstCreated.instance);
  const first = firstCreated.instance;
  const agent = firstCreated.agent;
  const requests: any[] = [];
  const sessions: any[] = [];
  let holdSecondList = true;
  let releaseSecondList!: () => void;
  const secondListReady = new Promise<void>((resolve) => {
    releaseSecondList = resolve;
  });
  await page.route("**/rpc", async (route) => {
    const c = route.request().postDataJSON();
    let result: any;
    if (c.method === "session.open") {
      requests.push(c);
      let session = !c.params.payload.force_new && sessions.findLast(
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
                    work_type: "discussion",
          },
        };
        sessions.push(session);
      }
      result = { id: session.id };
    } else if (c.method === "query.sessions" && c.params.scope === "global") {
      requests.push(c);
      if (holdSecondList && c.params.agent_instance_id === secondCreated.instance)
        await secondListReady;
      const selected = sessions.filter(
        (s) =>
          !c.params.agent_instance_id ||
          s.runtime_binding.agent_configuration.agent_instance_id ===
            c.params.agent_instance_id,
      );
      result = { sessions: selected.toReversed(), total: selected.length };
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
    } else return route.fallback();
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
  const firstSession = sessions.at(-1).id;
  await page.getByRole("button", {name:"新建 Session",exact:true}).click();
  await expect.poll(() => sessions.length).toBe(2);
  const latestSession = sessions.at(-1).id;
  expect(latestSession).not.toBe(firstSession);
  await expect(page.getByLabel("讨论会话",{exact:true})).toHaveValue(latestSession);
  await open.click();
  await expect(page.getByLabel("讨论会话",{exact:true})).toHaveValue(latestSession);
  expect(sessions).toHaveLength(2);
  await identity.selectOption(secondCreated.instance);
  await expect(page.getByLabel("讨论会话", { exact: true }).locator(`option[value="${latestSession}"]`)).toHaveCount(0);
  holdSecondList = false;
  releaseSecondList();
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

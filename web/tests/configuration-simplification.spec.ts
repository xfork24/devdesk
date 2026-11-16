import { expect, type Page } from "@playwright/test";
import { test } from "./support/kernel";
import { useMockCodexCatalog } from "./support/model-catalog";
useMockCodexCatalog();
async function rpc(page: Page, method: string, params: object = {}) {
  const value = await page.request
    .post("/rpc", { data: { jsonrpc: "2.0", id: 1, method, params } })
    .then((r) => r.json());
  expect(value.error, JSON.stringify(value)).toBeUndefined();
  return value.result;
}
test("配置归属与权限工具行，Workflow 内嵌调度", async ({ page }, info) => {
  await page.goto("/?tab=components&config_tab=permission");
  await expect(page.getByLabel("统一权限 JSON")).toBeVisible();
  for (const name of ["运行资源", "调度策略"])
    await expect(page.getByRole("tab", { name, exact: true })).toHaveCount(0);
  await expect(page.getByText("执行授权", { exact: true })).toHaveCount(0);
  const select = await page.getByLabel("版本化权限修订").boundingBox();
  const button = await page
    .getByRole("button", { name: "新建权限草稿", exact: true })
    .boundingBox();
  expect(
    Math.abs(select!.y + select!.height - button!.y - button!.height),
  ).toBeLessThan(2);
  await expect(page.getByLabel("ExecutionScope JSON")).toHaveCount(0);
  await page.screenshot({
    path: info.outputPath("permission-toolbar.png"),
    fullPage: true,
  });
  await page.goto(
    "/?tab=components&config_tab=harness&config_object=builtin-harness-codex-managed%3A1",
  );
  await expect(page.getByLabel("Harness 安装与登录组件")).toHaveValue(
    /@openai\/codex/,
  );
  await expect(page.getByLabel("运行资源精确修订")).toHaveCount(0);
  await page.goto(
    "/?tab=components&config_tab=sandbox&config_object=builtin-sandbox-docker%3A1",
  );
  await expect(page.getByLabel("Docker 基础环境与构建")).toHaveValue(
    /base_image/,
  );
  const defaults = await rpc(page, "query.configuration_defaults");
  await page.goto(
    `/?tab=workflows&config_tab=workflow&workflow_id=${encodeURIComponent(defaults.workflow_ref)}`,
  );
  await expect(page.getByLabel("调度配置")).toBeVisible();
  expect(
    JSON.parse(await page.getByLabel("调度配置").inputValue()).agent
      .max_turns_per_wakeup,
  ).toBe(8);
  await page.setViewportSize({ width: 760, height: 1000 });
  await page.screenshot({
    path: info.outputPath("workflow-scheduling.png"),
    fullPage: true,
  });
});
test("网页登录展示阶段并可在准备阶段取消", async ({ page }) => {
  let cancel = false;
  let finish: () => void = () => {};
  const done = new Promise<void>((resolve) => {
    finish = resolve;
  });
  await page.route("**/rpc", async (route) => {
    const call = route.request().postDataJSON();
    if (call.method === "codex.auth.start") {
      await done;
      await route.fulfill({
        json: {
          jsonrpc: "2.0",
          id: call.id,
          error: { message: "准备已取消", data: { code: "cancelled" } },
        },
      });
      return;
    }
    if (call.method === "codex.auth.preparation_status") {
      await route.fulfill({
        json: {
          jsonrpc: "2.0",
          id: call.id,
          result: {
            preparations: [
              {
                provider: "codex-auth",
                message: "校验本地缓存",
                started: Date.now() - 3000,
              },
            ],
          },
        },
      });
      return;
    }
    if (call.method === "codex.auth.preparation_cancel") {
      cancel = true;
      finish();
      await route.fulfill({
        json: {
          jsonrpc: "2.0",
          id: call.id,
          result: { cancel_requested: true },
        },
      });
      return;
    }
    await route.fallback();
  });
  await page.goto("/");
  await page.getByRole("button", { name: "设置", exact: true }).click();
  await page.getByRole("button", { name: "网页登录", exact: true }).click();
  await expect(page.getByText(/校验本地缓存 · 已用/)).toBeVisible();
  await page.getByRole("button", { name: "取消登录准备", exact: true }).click();
  await expect.poll(() => cancel).toBe(true);
  await expect(page.getByText(/准备已取消/)).toBeVisible();
});
test("授权地址返回后立即打开新页，工作区刷新不阻塞", async ({
  page,
  context,
}) => {
  let hold = false;
  let release: () => void = () => {};
  const wait = new Promise<void>((resolve) => {
    release = resolve;
  });
  await context.route("https://auth.openai.com/**", (route) =>
    route.fulfill({
      body: "Official authorization fixture",
      contentType: "text/html",
    }),
  );
  await page.route("**/rpc", async (route) => {
    const call = route.request().postDataJSON();
    if (call.method === "codex.auth.start") {
      hold = true;
      await route.fulfill({
        json: {
          jsonrpc: "2.0",
          id: call.id,
          result: {
            loginId: "login",
            authUrl: "https://auth.openai.com/authorize?fixture=1",
          },
        },
      });
      return;
    }
    if (hold && call.method === "query.workspace") await wait;
    await route.fallback();
  });
  await page.goto("/");
  await page.getByRole("button", { name: "设置", exact: true }).click();
  const opened = page.waitForEvent("popup");
  await page.getByRole("button", { name: "网页登录", exact: true }).click();
  const authorization = await opened;
  try {
    await expect(authorization).toHaveURL(/auth\.openai\.com\/authorize/);
    await expect(
      page.getByRole("button", { name: "检查登录状态", exact: true }),
    ).toBeEnabled();
  } finally {
    release();
    await authorization.close();
  }
});
test("流式回复只有一条消息，工具正文与技术回执在新标签页查看", async ({
  page,
  context,
}, info) => {
  await page.goto("/");
  const defaults = await rpc(page, "query.configuration_defaults");
  const agent = defaults.role_agent_bindings["builtin-role-analyst:1"];
  const instance = await rpc(page, "agent.instance.create", {
    command_id: crypto.randomUUID(),
    payload: { name: "展示验证", agent_revision_id: agent, scope: "global" },
  });
  const session = {
    id: "fixture-session",
    owner_id: "global",
    scope: "global",
    agent,
    revision: 1,
    lifecycle: "idle",
    agent_instance_id: instance.id,
    runtime_binding: {
      installation: { key: { provider: "codex" } },
      agent_configuration: { agent_instance_id: instance.id },
      work_type: "discussion",
    },
  };
  const events = [
    {
      id: "delta-1",
      sequence: 1,
      kind: "assistant",
      content: "临时",
      mode: "delta",
      turn_id: "turn",
      provider_item: "message",
    },
    {
      id: "delta-2",
      sequence: 2,
      kind: "assistant",
      content: "内容",
      mode: "delta",
      turn_id: "turn",
      provider_item: "message",
    },
    {
      id: "full",
      sequence: 3,
      kind: "assistant",
      content:
        "最终完整回答\n\n" +
        "这是一段较长的正文，用于确认前面的行可以使用内容框的完整宽度。".repeat(
          10,
        ) +
        "\n\n正文最后一行。",
      mode: "full",
      turn_id: "turn",
      provider_item: "message",
    },
    {
      id: "stop",
      sequence: 4,
      kind: "status",
      content: JSON.stringify({
        operation_id: "operation",
        writer_revoked: true,
        mode: "normal_stop",
        requires_successor: false,
      }),
      mode: "full",
      turn_id: "turn",
    },
    {
      id: "end",
      sequence: 5,
      kind: "status",
      content: "completed",
      mode: "full",
      turn_id: "turn",
    },
  ];
  const toolEvents = [
    {
      id: "search-start",
      provider_item: "search",
      content: { type: "webSearch", action: { query: "search-request-body" } },
    },
    {
      id: "search-complete",
      provider_item: "search",
      content: {
        type: "webSearch",
        status: "completed",
        action: { query: "search-request-body" },
        result: "search-response-body",
      },
    },
    {
      id: "command",
      provider_item: "command",
      content: {
        type: "commandExecution",
        command: "command-request-body",
        aggregatedOutput: "command-response-body",
        error: "command-error-body",
        status: "failed",
        exitCode: 1,
      },
    },
    {
      id: "tool-call",
      provider_item: "tool/call:read",
      content: {
        message: { name: "readFile", arguments: { path: "read-request-body" } },
      },
    },
    {
      id: "tool-result",
      provider_item: "tool/result:read",
      content: { message: { name: "readFile", output: "read-response-body" } },
    },
    {
      id: "plain-tool",
      provider_item: "plain-tool",
      content: "plain-tool-body",
    },
  ].map((event, index) => ({
    ...event,
    content:
      typeof event.content === "string"
        ? event.content
        : JSON.stringify(event.content),
    sequence: 6 + index,
    kind: "tool",
    mode: "full",
    turn_id: "turn",
  }));
  const layoutEvents = [
    {
      id: "list-reply",
      content: "列表回复\n\n1. 第一项\n2. **列表最后一项**",
    },
    {
      id: "code-reply",
      content: "代码回复\n\n```ts\nconst answer = 42;\n```",
    },
    {
      id: "table-reply",
      content: "| 名称 | 值 |\n| --- | --- |\n| 表格回复 | 42 |",
    },
    {
      id: "wrapping-reply",
      content: "自然换行验证：" + "连续正文内容".repeat(80),
    },
  ].map((event, index) => ({
    ...event,
    sequence: events.length + toolEvents.length + index + 1,
    kind: "assistant",
    mode: "full",
    turn_id: "turn",
    provider_item: event.id,
  }));
  const allEvents = [...events, ...toolEvents, ...layoutEvents];
  await context.route("**/rpc", async (route) => {
    const call = route.request().postDataJSON();
    let result;
    if (call.method === "session.open") result = { id: session.id };
    else if (call.method === "query.sessions")
      result = { sessions: [session], total: 1 };
    else if (call.method === "query.session")
      result = call.params.event_id
        ? {
            event: toolEvents.find(
              (event) => event.id === call.params.event_id,
            ),
          }
        : call.params.diagnostic_turn_id
          ? {
              turn: { id: "turn", status: "completed" },
              stop_receipts: [JSON.parse(events[3].content)],
              records: [events[2], events[3], events[4]],
            }
          : {
              session,
              current_runtime_binding: session.runtime_binding,
              events: call.params.cursor ? [] : allEvents,
              next_cursor: allEvents.length,
              turns: [],
              executions: [],
              interactions: [],
              proposals: [],
            };
    else return route.fallback();
    await route.fulfill({ json: { jsonrpc: "2.0", id: call.id, result } });
  });
  await page.getByRole("button", { name: "角色讨论", exact: true }).click();
  await page.getByLabel("讨论角色", { exact: true }).selectOption(agent);
  await page.getByLabel("Agent", { exact: true }).selectOption(instance.id);
  await page
    .getByRole("button", { name: "开始 / 续用讨论", exact: true })
    .click();
  await expect(page.getByText("最终完整回答", { exact: true })).toHaveCount(1);
  await expect(page.getByText(/合并展示|writer_revoked|运行状态/)).toHaveCount(
    0,
  );
  const log = page.getByRole("log", { name: "讨论记录", exact: true });
  const tools = log.locator(".conversation-message.tool");
  await expect(tools).toHaveCount(5);
  await expect(tools.getByLabel("工具名称", { exact: true })).toHaveText([
    "webSearch",
    "commandExecution",
    "readFile",
    "readFile",
    "工具事件",
  ]);
  await expect(log).not.toContainText(
    /request-body|response-body|error-body|plain-tool-body|退出码|状态：/,
  );
  await expect(tools.locator("details, pre")).toHaveCount(0);
  async function checkMessageRows() {
    for (const message of await log.locator("article").all()) {
      const body = message
        .getByLabel("内容预览", { exact: true })
        .or(message.getByLabel("工具类型", { exact: true }));
      const frame = message.getByRole("group", {
        name: "消息内容",
        exact: true,
      });
      const link = frame.getByRole("link", {
        name: "原始事件 ↗",
        exact: true,
      });
      const content = (await body.boundingBox())!;
      const bounds = (await frame.boundingBox())!;
      const eventLink = await link.boundingBox();
      expect(
        Math.abs(content.x + content.width - bounds.x - bounds.width + 15),
      ).toBeLessThan(2);
      expect(eventLink!.y).toBeGreaterThan(bounds.y);
      expect(eventLink!.y + eventLink!.height).toBeLessThanOrEqual(
        bounds.y + bounds.height,
      );
      const rightInset =
        bounds.x + bounds.width - eventLink!.x - eventLink!.width;
      expect(rightInset).toBeGreaterThanOrEqual(14);
      expect(rightInset).toBeLessThanOrEqual(16);
      const ending = await body.evaluate((node) => {
        const texts = document.createTreeWalker(node, NodeFilter.SHOW_TEXT);
        let last: Node | undefined;
        while (texts.nextNode()) {
          const current = texts.currentNode;
          if (
            current.textContent?.trim() &&
            !current.parentElement?.closest(".markdown-trailing")
          )
            last = current;
        }
        if (!last) throw new Error("正文缺少可读取的文本");
        const start = last.textContent!.search(/\S\s*$/);
        const range = document.createRange();
        range.setStart(last, start);
        range.setEnd(last, start + 1);
        const rect = range.getBoundingClientRect();
        return {
          y: rect.y,
          bottom: rect.bottom,
          x: rect.x,
          right: rect.right,
          separate: Boolean(last.parentElement?.closest("pre, table")),
        };
      });
      expect(eventLink!.y).toBeGreaterThanOrEqual(ending.y - 4);
      if (ending.separate) {
        expect(eventLink!.y).toBeGreaterThanOrEqual(ending.bottom);
      } else {
        const wraps = await body
          .textContent()
          .then((text) => text?.startsWith("自然换行验证："));
        if (!wraps) {
          expect(
            Math.abs(
              eventLink!.y +
                eventLink!.height / 2 -
                (ending.y + ending.bottom) / 2,
            ),
          ).toBeLessThan(4);
        } else if (eventLink!.y < ending.bottom) {
          expect(eventLink!.x).toBeGreaterThan(ending.right);
        }
      }
      await expect(frame).toHaveCSS("border-top-style", "solid");
      await expect(frame).toHaveCSS("border-top-width", "1px");
    }
    expect(
      await log.evaluate((node) => node.scrollWidth <= node.clientWidth),
    ).toBe(true);
  }
  await checkMessageRows();
  await log.evaluate((node) => node.scrollIntoView({ block: "center" }));
  const beforeResize = (await log.boundingBox())!;
  await page.mouse.move(
    beforeResize.x + beforeResize.width - 3,
    beforeResize.y + beforeResize.height - 3,
  );
  await page.mouse.down();
  await page.mouse.move(
    beforeResize.x + beforeResize.width + 40,
    beforeResize.y + beforeResize.height + 120,
    { steps: 8 },
  );
  await page.mouse.up();
  await expect
    .poll(async () => (await log.boundingBox())!.height)
    .toBeGreaterThan(beforeResize.height + 80);
  const expanded = (await log.boundingBox())!;
  expect(Math.abs(expanded.width - beforeResize.width)).toBeLessThan(2);
  await page.mouse.move(
    expanded.x + expanded.width - 3,
    expanded.y + expanded.height - 3,
  );
  await page.mouse.down();
  await page.mouse.move(
    expanded.x + expanded.width - 3,
    expanded.y + expanded.height - 83,
    { steps: 8 },
  );
  await page.mouse.up();
  await expect
    .poll(async () => (await log.boundingBox())!.height)
    .toBeLessThan(expanded.height - 50);
  await page.setViewportSize({ width: 720, height: 1000 });
  await checkMessageRows();
  await page.setViewportSize({ width: 1440, height: 1000 });
  for (const [index, event] of toolEvents.slice(1).entries()) {
    const link = tools
      .nth(index)
      .getByRole("link", { name: "原始事件 ↗", exact: true });
    await expect(link).toHaveAttribute("target", "_blank");
    await expect(link).toHaveAttribute(
      "href",
      new RegExp(`event_id=${event.id}`),
    );
    const opened = page.waitForEvent("popup");
    await link.click();
    const raw = await opened;
    await expect(
      raw.getByRole("heading", { name: "原始事件", exact: true }),
    ).toBeVisible();
    const expected = JSON.stringify({ event }, null, 2);
    await expect(raw.getByLabel("完整原始 JSON")).toHaveText(expected);
    await raw.reload();
    await expect(raw.getByLabel("完整原始 JSON")).toHaveText(expected);
    await raw.close();
  }
  const link = log
    .locator(".conversation-message.assistant")
    .first()
    .getByRole("link", { name: "原始事件 ↗", exact: true });
  await expect(link).toHaveAttribute("target", "_blank");
  await link.focus();
  await page.waitForResponse((response) => {
    if (!response.url().endsWith("/rpc")) return false;
    const call = response.request().postDataJSON();
    return call?.method === "query.session" && call.params.cursor > 0;
  });
  await page.evaluate(
    () =>
      new Promise<void>((resolve) =>
        requestAnimationFrame(() => requestAnimationFrame(() => resolve())),
      ),
  );
  await expect(link).toBeFocused();
  const opened = page.waitForEvent("popup");
  await link.click();
  const raw = await opened;
  await expect(
    raw.getByRole("heading", { name: "原始事件", exact: true }),
  ).toBeVisible();
  await expect(raw.locator("pre")).toContainText("writer_revoked");
  await expect(raw.locator("pre")).not.toContainText("delta-1");
  await raw.reload();
  await expect(raw.locator("pre")).toContainText("最终完整回答");
  await page.screenshot({
    path: info.outputPath("discussion-message.png"),
    fullPage: true,
  });
  await raw.close();
});

test("刷新设置恢复同一待授权请求，取消仍指向原登录", async ({ page }) => {
  let cancelled = "";
  let starts = 0;
  await page.route("**/rpc", async (route) => {
    const call = route.request().postDataJSON();
    if (call.method === "codex.auth.start") starts += 1;
    if (call.method === "codex.auth.status") {
      await route.fulfill({
        json: {
          jsonrpc: "2.0",
          id: call.id,
          result: {
            authenticated: false,
            pending: true,
            pending_login: {
              loginId: "pending-original",
              authUrl:
                "https://auth.openai.com/authorize?state=fixture-recovery",
            },
          },
        },
      });
      return;
    }
    if (call.method === "codex.auth.cancel") {
      cancelled = call.params.login_id;
      await route.fulfill({
        json: { jsonrpc: "2.0", id: call.id, result: {} },
      });
      return;
    }
    await route.fallback();
  });
  await page.goto("/?tab=settings");
  const link = page.getByRole("link", { name: "打开官方授权网页" });
  await expect(link).toHaveAttribute("href", /fixture-recovery/);
  await page.reload();
  await expect(link).toHaveAttribute("href", /fixture-recovery/);
  await page.getByRole("button", { name: "取消登录", exact: true }).click();
  await expect.poll(() => cancelled).toBe("pending-original");
  expect(starts).toBe(0);
});

test("项目默认调度与 Profile 保存恢复，工具接入空状态区分模型登录", async ({
  page,
}) => {
  await page.goto("/");
  const project = await rpc(page, "project.create", {
    command_id: crypto.randomUUID(),
    payload: { title: "工具接入说明", repository: "fixture/tool-bindings" },
  });
  await rpc(page, "project.open", {
    command_id: crypto.randomUUID(),
    target: project.id,
    payload: {},
  });
  await page.goto(
    `/?tab=projects&project_id=${encodeURIComponent(project.id)}`,
  );
  const panel = page.getByRole("region", { name: "项目配置", exact: true });
  const mode = panel.getByLabel("默认调度模式", { exact: true });
  const profile = panel.getByLabel("Harness Profile", { exact: true });
  const fullAccess = "builtin-permission-harness-full-access:1";
  await expect(mode).toHaveValue("agent");
  await expect(profile).toHaveValue(fullAccess);
  await expect(profile.locator("option:checked")).toContainText(
    "Harness 容器内全权限",
  );
  await expect(
    panel.getByLabel("Harness Profile（留空采用默认）", { exact: true }),
  ).toHaveCount(0);
  async function saveProject(expectedMode: string, expectedProfile: string) {
    const response = page.waitForResponse(
      (r) =>
        r.url().endsWith("/rpc") &&
        r.request().postDataJSON()?.method === "project.configuration.update",
    );
    await panel
      .getByRole("button", { name: "保存项目配置", exact: true })
      .click();
    expect((await (await response).json()).error).toBeUndefined();
    const saved = await rpc(page, "query.project_configuration", {
      project_id: project.id,
    });
    expect(saved.configuration.data.orchestration.mode).toBe(expectedMode);
    expect(saved.configuration.data.harness_permission_ref).toBe(
      expectedProfile,
    );
  }
  await saveProject("agent", fullAccess);
  await page.reload();
  await expect(mode).toHaveValue("agent");
  await expect(profile).toHaveValue(fullAccess);
  await mode.selectOption("manual");
  await profile.selectOption("builtin-permission-harness-read-only:1");
  await saveProject("manual", "builtin-permission-harness-read-only:1");
  await page.reload();
  await expect(mode).toHaveValue("manual");
  await expect(profile).toHaveValue("builtin-permission-harness-read-only:1");
  await page.getByRole("button", { name: "采用默认配置", exact: true }).click();
  await expect(mode).toHaveValue("agent");
  await expect(profile).toHaveValue(fullAccess);
  await expect(
    page.getByRole("heading", { name: "项目工具接入绑定", exact: true }),
  ).toBeVisible();
  await expect(
    page.getByText("当前所选 Agent 的工具无需配置项目级外部接入绑定。", {
      exact: true,
    }),
  ).toBeVisible();
  await expect(
    page.getByRole("button", { name: "保存项目接入绑定", exact: true }),
  ).toHaveCount(0);
  await expect(page.getByText(/模型服务商登录/)).toBeVisible();
});

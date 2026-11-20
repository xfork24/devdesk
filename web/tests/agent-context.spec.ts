import { expect, type Page } from "@playwright/test";
import { test } from "./support/kernel";

async function rpc(page: Page, method: string, params: object = {}) {
  const response = await page.request.post("/rpc", {
    data: { jsonrpc: "2.0", id: 1, method, params },
  });
  const value = await response.json();
  expect(value.error, JSON.stringify(value)).toBeUndefined();
  return value.result;
}

test("没有项目时可以选择 Agent 并查看完整能力和经验入口", async ({
  page,
}, info) => {
  await page.goto("/");
  const defaults = await rpc(page, "query.configuration_defaults");
  const agent = defaults.role_agent_bindings["builtin-role-analyst:1"];
  const instance = await rpc(page, "agent.instance.create", {
    command_id: crypto.randomUUID(),
    payload: {
      name: "独立需求分析师",
      agent_revision_id: agent,
      scope: "global",
    },
  });
  await page.reload();
  await page.getByRole("button", { name: "角色讨论", exact: true }).click();
  await expect(page.getByLabel("讨论角色", { exact: true })).toBeEnabled();
  await page.getByLabel("讨论角色", { exact: true }).selectOption(agent);
  await page.getByLabel("Agent", { exact: true }).selectOption(instance.id);
  await expect(page.getByLabel("讨论范围", { exact: true })).toHaveValue(
    "global",
  );
  await expect(
    page.getByRole("button", { name: "开始 / 续用讨论", exact: true }),
  ).toBeEnabled();
  const information = page.getByLabel("Agent信息", { exact: true });
  const summaries = information.locator(":scope > details > summary");
  await expect(summaries).toHaveText(["Agent定义", "Agent能力", "Agent经验"]);
  const positions = await summaries.evaluateAll((nodes) =>
    nodes.map((node) => node.getBoundingClientRect().top),
  );
  expect(Math.max(...positions) - Math.min(...positions)).toBeLessThan(2);
  await expect(
    page
      .locator(".conversation-panel > .actions > p")
      .filter({ hasText: /^Model / }),
  ).toHaveCount(0);
  await expect(information.locator(".experience-panel")).toHaveCSS(
    "border-top-width",
    "0px",
  );
  await information.getByText("Agent定义", { exact: true }).click();
  await expect(information.locator("pre").first()).toContainText(agent);
  await page.getByText("Agent能力", { exact: true }).click();
  await expect(page.getByText("Agent Specs", { exact: true })).toBeVisible();
  await expect(
    page.getByText("Agent 经验：检索与读取，可在当前讨论中按需使用。", {
      exact: true,
    }),
  ).toBeVisible();
  await page.getByText("Agent经验", { exact: true }).click();
  await expect(page.getByText(/0\s*条经验/)).toBeVisible();
  const workspace = await rpc(page, "query.workspace");
  expect(workspace.active_project).toBeNull();
  await page.screenshot({
    path: info.outputPath("detached-agent.png"),
    fullPage: true,
  });
  await page.setViewportSize({ width: 430, height: 932 });
  const mobilePositions = await summaries.evaluateAll((nodes) =>
    nodes.map((node) => node.getBoundingClientRect().top),
  );
  expect(
    Math.max(...mobilePositions) - Math.min(...mobilePositions),
  ).toBeLessThan(2);
  await page.screenshot({
    path: info.outputPath("detached-agent-mobile.png"),
    fullPage: true,
  });
});

test("全局讨论选择来源项目的原 Agent 实例时保留本页项目选择", async ({
  page,
}) => {
  await page.goto("/");
  const defaults = await rpc(page, "query.configuration_defaults");
  const template = defaults.records.find(
    (row: { id: string }) =>
      row.id === defaults.role_agent_bindings["builtin-role-analyst:1"],
  );
  const source = await rpc(page, "project.create", {
    command_id: crypto.randomUUID(),
    payload: { title: "经验来源项目" },
  });
  const agent = await rpc(page, "agent.create", {
    project_id: source.id,
    command_id: crypto.randomUUID(),
    payload: {
      name: "项目私有分析师",
      scope: `project:${source.id}`,
      role_ref: template.role_ref,
      capability_ref: template.capability_ref,
      data: {},
    },
  });
  const instance = await rpc(page, "agent.instance.create", {
    project_id: source.id,
    command_id: crypto.randomUUID(),
    payload: {
      name: "保留原经验的分析师",
      agent_revision_id: agent.id,
      scope: `project:${source.id}`,
    },
  });
  const current = await rpc(page, "project.create", {
    command_id: crypto.randomUUID(),
    payload: { title: "本页选择项目", repository: "fixture/current" },
  });
  await page.goto(
    `/?tab=discussion&project_id=${encodeURIComponent(current.id)}`,
  );
  await page.getByRole("button", { name: "角色讨论", exact: true }).click();
  await expect(page.getByLabel("讨论角色", { exact: true })).toBeEnabled();
  await page.getByLabel("讨论范围", { exact: true }).selectOption("global");
  await page.getByLabel("讨论角色", { exact: true }).selectOption(agent.id);
  await page.getByLabel("Agent", { exact: true }).selectOption(instance.id);
  await expect(page.getByLabel("Agent", { exact: true })).toHaveValue(
    instance.id,
  );
  await expect(
    page
      .getByLabel("Agent", { exact: true })
      .locator(`option[value="${instance.id}"]`),
  ).toContainText("经验来源项目");
  await expect(
    page.getByRole("button", { name: "开始 / 续用讨论", exact: true }),
  ).toBeEnabled();
  await expect(page.getByLabel("当前项目", { exact: true })).toHaveValue(
    current.id,
  );
  expect(
    (await rpc(page, "query.ui_bootstrap", { project_id: current.id }))
      .active_project.id,
  ).toBe(current.id);
  expect((await rpc(page, "query.ui_bootstrap")).active_project).toBeNull();
  const catalog = await page.request
    .post("/rpc", {
      data: {
        jsonrpc: "2.0",
        id: 1,
        method: "query.agent_experience",
        params: { scope: "global", agent_instance_id: instance.id },
      },
    })
    .then((r) => r.json());
  expect(catalog.result.total).toBe(0);
});

test("来源不可用时明确显示经验目录为历史摘要", async ({ page }) => {
  await page.goto("/");
  const defaults = await rpc(page, "query.configuration_defaults");
  const agent = defaults.role_agent_bindings["builtin-role-analyst:1"];
  const instance = await rpc(page, "agent.instance.create", {
    command_id: crypto.randomUUID(),
    payload: {
      name: "目录摘要分析师",
      agent_revision_id: agent,
      scope: "global",
    },
  });
  await page.route("**/rpc", async (route) => {
    const call = route.request().postDataJSON();
    if (call.method !== "query.agent_experience") return route.fallback();
    return route.fulfill({
      json: {
        jsonrpc: "2.0",
        id: call.id,
        result: {
          records: [
            {
              project_id: "unavailable-origin",
              project_name: "历史来源项目",
              feature_name: "历史工作",
              snapshot_id: "cached-snapshot",
              session_id: "cached-session",
              captured_at: 1000,
              kind: "conversation_completed",
              metadata_excerpt: true,
              history_more: true,
              snapshot_count: 46,
              availability: "unavailable",
            },
          ],
          total: 1,
          unavailable: [],
          scope: "global",
        },
      },
    });
  });
  await page.reload();
  await page.getByRole("button", { name: "角色讨论", exact: true }).click();
  await page.getByLabel("讨论角色", { exact: true }).selectOption(agent);
  await page.getByLabel("Agent", { exact: true }).selectOption(instance.id);
  await page.getByText("Agent能力", { exact: true }).click();
  await page.getByText("Agent经验", { exact: true }).click();
  await expect(
    page.getByText("来源暂不可用，当前显示缓存的历史摘要。", { exact: true }),
  ).toBeVisible();
});

test("经验分页、引用、展开、删除及来源会话", async ({ page }, info) => {
  await page.goto("/");
  const defaults = await rpc(page, "query.configuration_defaults");
  const agent = defaults.role_agent_bindings["builtin-role-analyst:1"];
  const [instance, source, current] = await Promise.all([
    rpc(page, "agent.instance.create", {
      command_id: crypto.randomUUID(),
      payload: {
        name: "经验回归分析师",
        agent_revision_id: agent,
        scope: "global",
      },
    }),
    ...["history", "current"].map((name) =>
      rpc(page, "project.create", {
        command_id: crypto.randomUUID(),
        payload: { title: name, repository: `fixture/${name}` },
      }),
    ),
  ]);
  const records = Array.from({ length: 11 }, (_, i) => ({
    project_id: source.id,
    project_name: "历史项目",
    feature_name: `工作 ${i + 1}`,
    session_id: `source-session-${i + 1}`,
    turn_id: `source-turn-${i + 1}`,
    snapshot_id: `snapshot-${i + 1}`,
    captured_at: 123 + i,
    kind: "conversation_completed",
  }));
  const session = (id: string, owner: string) => ({
    id,
    owner_id: owner,
    agent,
    agent_instance_id: instance.id,
    revision: 3,
    lifecycle: "active",
    runtime_binding: {
      model: "fixture",
      installation: { key: { provider: "codex" } },
      agent_configuration: { agent_instance_id: instance.id },
    },
  });
  const errors: string[] = [];
  page.on("pageerror", (e) => errors.push(String(e)));
  let sent: unknown;
  let anchored: unknown;
  await page.route("**/rpc", async (route) => {
    const { method, params, id } = route.request().postDataJSON();
    let result;
    switch (method) {
      case "query.agent_experience": {
        const entry = records.find(
          (row) => row.snapshot_id === params.snapshot_id,
        );
        result = entry
          ? {
              entry,
              snapshot: { id: entry.snapshot_id, content: "已核对的快照正文" },
              integrity: "verified",
            }
          : {
              records: records.slice(
                params.offset,
                params.offset + params.limit,
              ),
              total: records.length,
              unavailable: [],
            };
        break;
      }
      case "experience.delete": {
        expect(params.target).toBe("snapshot-11");
        expect(params.expected_revision).toBe(0);
        expect(params.payload.agent_instance_id).toBe(instance.id);
        records.pop();
        result = { snapshot_id: params.target };
        break;
      }
      case "query.sessions":
        if (params.project_id === source.id)
          anchored = params.workflow_session_id;
        result = {
          sessions: [
            session("newer-session", params.project_id),
            session(
              params.workflow_session_id || "discussion",
              params.project_id,
            ),
          ],
          total: 2,
        };
        break;
      case "query.session":
        result = {
          session: session(params.id, params.project_id),
          events: [],
          next_cursor: 0,
          executions: [],
          interactions: [],
          turns:
            params.id === "source-session-1"
              ? [{ id: "source-turn-1", status: "completed" }]
              : [],
          proposals: [],
          experience_reads:
            sent && params.id === "discussion"
              ? [{ id: "read", status: "succeeded", source: records[0] }]
              : [],
        };
        break;
      case "experience.restore":
        expect(params.target).toBe(current.id);
        expect(params.payload.snapshot_id).toBe("snapshot-1");
        result = { id: "restored-session" };
        break;
      case "conversation.send":
        sent = params.payload.experience_refs;
        result = { id: "completed-turn" };
        break;
      default:
        return route.fallback();
    }
    await route.fulfill({ json: { jsonrpc: "2.0", id, result } });
  });
  await page.goto(`/?tab=discussion&project_id=${current.id}`);
  await page.getByLabel("讨论角色", { exact: true }).selectOption(agent);
  await page.getByLabel("Agent", { exact: true }).selectOption(instance.id);
  await page.getByLabel("讨论会话", { exact: true }).selectOption("discussion");
  await page.getByText("Agent经验", { exact: true }).click();
  const entries = page.locator(".experience-entry");
  await expect(entries).toHaveCount(10);
  const first = entries.filter({
    has: page.getByText("历史项目 / 工作 1", { exact: true }),
  });
  await first.getByRole("button", { name: "展开摘要与快照" }).click();
  await expect(
    first.getByText("已核对的快照正文", { exact: true }),
  ).toBeVisible();
  await first.getByText("完整快照与证据引用").click();
  await expect(first.locator("pre")).toContainText("snapshot-1");
  await first.getByRole("button", { name: "引用到当前讨论" }).click();
  await expect(first.getByRole("button", { name: "已引用" })).toHaveAttribute(
    "aria-pressed",
    "true",
  );
  await page.getByRole("button", { name: "下一页" }).click();
  await expect(entries).toHaveCount(1);
  await entries.getByRole("button", { name: "引用到当前讨论" }).click();
  const references = page.getByRole("region", { name: "当前讨论引用的经验" });
  await expect(references.getByRole("listitem")).toHaveCount(2);
  await references
    .getByRole("button", { name: "清除引用 历史项目 / 工作 11" })
    .click();
  await expect(references.getByRole("listitem")).toHaveCount(1);
  const pins = await references.boundingBox();
  const log = await page.getByRole("log", { name: "讨论记录" }).boundingBox();
  expect(pins!.y + pins!.height).toBeLessThanOrEqual(log!.y);
  await entries.getByRole("button", { name: "引用到当前讨论" }).click();
  await expect(references.getByRole("listitem")).toHaveCount(2);
  page.once("dialog", (dialog) => dialog.accept());
  await entries.getByRole("button", { name: "删除经验" }).click();
  await expect(entries).toHaveCount(10);
  await expect(first).toHaveClass(/referenced/);
  await page.getByRole("button", { name: "讨论输入", exact: true }).click();
  await page.getByRole("textbox", { name: "讨论输入" }).fill("读取引用的经验");
  await page.getByRole("button", { name: "发送", exact: true }).click();
  await expect
    .poll(() => sent)
    .toEqual([{ source_project_id: source.id, snapshot_id: "snapshot-1" }]);
  await expect(references.getByRole("listitem")).toHaveCount(1);
  await page.getByText("Agent 在本次讨论中读取的经验").click();
  await expect(
    page.getByText("历史项目 / 工作 1 · 已读取", { exact: true }),
  ).toBeVisible();
  await page.screenshot({
    path: info.outputPath("discussion-experience.png"),
    fullPage: true,
  });
  await first.getByRole("button", { name: "基于此快照开启新讨论" }).click();
  await expect(page.getByLabel("讨论会话", { exact: true })).toHaveValue(
    "restored-session",
  );
  await expect(references).toHaveCount(0);
  await first.getByRole("button", { name: "定位来源工作" }).click();
  await expect(page.getByLabel("当前项目", { exact: true })).toHaveValue(
    source.id,
  );
  await expect(page.getByLabel("讨论会话", { exact: true })).toHaveValue(
    "source-session-1",
  );
  await expect(page.getByLabel("讨论轮次", { exact: true })).toHaveValue(
    "source-turn-1",
  );
  await expect(page).toHaveURL(/session_id=source-session-1/);
  await expect.poll(() => anchored).toBe("source-session-1");
  await page.reload();
  await expect(page.getByLabel("讨论会话", { exact: true })).toHaveValue(
    "source-session-1",
  );
  expect(errors).toEqual([]);
});

test("已结束的会话可继承续聊并可删除历史轮次或会话", async ({ page }, info) => {
  await page.goto("/");
  const defaults = await rpc(page, "query.configuration_defaults");
  const agent = defaults.role_agent_bindings["builtin-role-analyst:1"];
  const project = await rpc(page, "project.create", {
    command_id: crypto.randomUUID(),
    payload: { title: "讨论续聊", repository: "fixture/discussion" },
  });
  const instance = await rpc(page, "agent.instance.create", {
    command_id: crypto.randomUUID(),
    payload: { name: "续聊分析师", agent_revision_id: agent, scope: "global" },
  });
  const original = {
    id: "original-session",
    owner_id: project.id,
    agent,
    agent_instance_id: instance.id,
    revision: 7,
    lifecycle: "requires_successor",
    runtime_binding: {
      model: "fixture",
      installation: { key: { provider: "codex" } },
      agent_configuration: { agent_instance_id: instance.id },
    },
  };
  const sessions = [{ ...original }];
  let turns = [
    { id: "earlier-turn", status: "completed", revision: 3 },
    { id: "cancelled-turn", status: "cancelled", revision: 5 },
  ];
  const messages = turns.map((turn, index) => ({
    id: turn.id,
    turn_id: turn.id,
    sequence: index + 1,
    kind: "assistant",
    type: "assistant",
    mode: "full",
    content: index === 0 ? "保留上一轮讨论" : "待删除的轮次正文",
  }));
  const continued: object[] = [];
  const writes: object[] = [];
  let sent: object | undefined;
  await page.route("**/rpc", async (route) => {
    const { method, params, id } = route.request().postDataJSON();
    let result;
    switch (method) {
      case "query.sessions":
        result = { sessions, total: sessions.length };
        break;
      case "query.session": {
        const session = sessions.find((row) => row.id === params.id);
        result = {
          session,
          turns: params.id === original.id ? turns : [],
          events:
            params.id === original.id
              ? messages.filter(
                  (event) =>
                    turns.some((turn) => turn.id === event.turn_id) &&
                    event.sequence > params.cursor,
                )
              : [],
          next_cursor: params.id === original.id ? 2 : 0,
          executions: [],
          interactions: [],
          proposals: [],
        };
        break;
      }
      case "session.open": {
        continued.push(params);
        const next = {
          ...original,
          id: `continued-session-${continued.length}`,
          predecessor: original.id,
          lifecycle: "prepared",
          revision: 1,
        };
        sessions.unshift(next);
        result = { id: next.id, revision: 1 };
        break;
      }
      case "conversation.send":
        sent = params;
        result = { id: "new-turn" };
        break;
      case "conversation.delete_turn": {
        writes.push(params);
        turns = turns.filter((turn) => turn.id !== params.payload.turn_id);
        const session = sessions.find((row) => row.id === params.target)!;
        session.revision += 1;
        result = { id: session.id, revision: session.revision };
        break;
      }
      case "session.delete": {
        writes.push(params);
        const index = sessions.findIndex((row) => row.id === params.target);
        sessions.splice(index, 1);
        result = { id: params.target };
        break;
      }
      default:
        return route.fallback();
    }
    await route.fulfill({ json: { jsonrpc: "2.0", id, result } });
  });

  await page.goto(
    `/?tab=discussion&project_id=${project.id}&session_id=${original.id}&turn_id=cancelled-turn`,
  );
  const log = page.getByRole("log", { name: "讨论记录" });
  await expect(log).toContainText("待删除的轮次正文");
  await expect(
    page.getByRole("button", { name: "发送", exact: true }),
  ).toBeDisabled();
  await page.getByRole("button", { name: "讨论输入", exact: true }).click();
  await page
    .getByRole("textbox", { name: "讨论输入", exact: true })
    .fill("续聊草稿");
  await page.getByRole("button", { name: "从此轮次继续" }).click();
  await expect(page.getByLabel("讨论会话", { exact: true })).toHaveValue(
    "continued-session-1",
  );
  expect(continued[0]).toMatchObject({
    target: project.id,
    expected_revision: 7,
    payload: {
      agent_instance_id: instance.id,
      continue_from: {
        session_id: original.id,
        turn_id: "cancelled-turn",
        turn_revision: 5,
      },
    },
  });
  await expect(
    page.getByRole("button", { name: "讨论输入", exact: true }),
  ).toContainText("续聊草稿");
  await page.getByRole("button", { name: "发送", exact: true }).click();
  await expect
    .poll(() => sent)
    .toMatchObject({
      target: "continued-session-1",
      expected_revision: 1,
      payload: { input: "续聊草稿" },
    });

  await page.getByLabel("讨论会话", { exact: true }).selectOption(original.id);
  await expect(log).toContainText("待删除的轮次正文");
  await page
    .getByLabel("讨论轮次", { exact: true })
    .selectOption("cancelled-turn");
  await page.screenshot({
    path: info.outputPath("discussion-history-actions.png"),
    fullPage: true,
  });
  page.once("dialog", (dialog) => dialog.accept());
  await page.getByRole("button", { name: "删除此轮次" }).click();
  await expect(
    page
      .getByLabel("讨论轮次", { exact: true })
      .locator('option[value="cancelled-turn"]'),
  ).toHaveCount(0);
  await expect(log).not.toContainText("待删除的轮次正文");
  expect(writes[0]).toMatchObject({
    target: original.id,
    expected_revision: 7,
    payload: { turn_id: "cancelled-turn", turn_revision: 5 },
  });
  await page.getByRole("button", { name: "继续此会话" }).click();
  await expect(page.getByLabel("讨论会话", { exact: true })).toHaveValue(
    "continued-session-2",
  );
  expect(continued[1]).toMatchObject({
    target: project.id,
    expected_revision: 8,
    payload: { continue_from: { session_id: original.id } },
  });
  await page.getByLabel("讨论会话", { exact: true }).selectOption(original.id);
  await expect(log).toContainText("保留上一轮讨论");
  page.once("dialog", (dialog) => dialog.accept());
  await page.getByRole("button", { name: "删除会话", exact: true }).click();
  await expect(
    page
      .getByLabel("讨论会话", { exact: true })
      .locator(`option[value="${original.id}"]`),
  ).toHaveCount(0);
  expect(writes[1]).toMatchObject({
    target: original.id,
    expected_revision: 8,
  });
});

import { expect } from "@playwright/test";
import { test } from "./support/kernel";

test("UUID 统一显示八位，同前缀轮次仍使用完整 ID 选择、请求、跳转和复制", async ({
  page,
}) => {
  await page.goto("/");
  const rpc = async (method: string, params: object = {}) => {
    const value = await page.request
      .post("/rpc", {
        data: { jsonrpc: "2.0", id: 1, method, params },
      })
      .then((response) => response.json());
    expect(value.error).toBeUndefined();
    return value.result;
  };
  const defaults = await rpc("query.configuration_defaults");
  const agent = defaults.role_agent_bindings["builtin-role-analyst:1"];
  const instance = await rpc("agent.instance.create", {
    command_id: crypto.randomUUID(),
    payload: {
      name: "UUID 显示验证",
      agent_revision_id: agent,
      scope: "global",
    },
  });
  const sessionId = "12345678-1234-4234-8234-123456789abc";
  const firstTurn = "abcdef01-1111-4111-8111-111111111111";
  const secondTurn = "abcdef01-2222-4222-8222-222222222222";
  const snapshot = "fedcba98-3333-4333-8333-333333333333";
  const session = {
    id: sessionId,
    owner_id: "global",
    agent,
    revision: 1,
    lifecycle: "idle",
    snapshot_id: snapshot,
    runtime_binding: {
      installation: { key: { provider: "codex" } },
      agent_configuration: { agent_instance_id: instance.id },
    },
  };
  const events = [
    {
      id: "first",
      sequence: 1,
      kind: "assistant",
      turn_id: firstTurn,
      content: `第一轮 · ${firstTurn}\n\n[引用 ${snapshot}](https://example.com/snapshot/${snapshot})\n\n实例 agent-instance.${sessionId}:1`,
    },
    {
      id: "second",
      sequence: 2,
      kind: "assistant",
      turn_id: secondTurn,
      content: `第二轮 · ${secondTurn}`,
    },
  ];
  const reads: string[] = [];
  const rawValue = {
    session_id: sessionId,
    turn_id: firstTurn,
    snapshot_id: snapshot,
    resource: `custom-${snapshot}:2`,
  };
  await page.context().route("**/rpc", async (route) => {
    const call = route.request().postDataJSON();
    if (call.method === "query.sessions")
      return route.fulfill({
        json: { jsonrpc: "2.0", id: call.id, result: { sessions: [session] } },
      });
    if (call.method === "query.session") {
      reads.push(call.params.id);
      return route.fulfill({
        json: {
          jsonrpc: "2.0",
          id: call.id,
          result: call.params.diagnostic_turn_id
            ? rawValue
            : {
                session,
                turns: [
                  { id: firstTurn, status: "completed" },
                  { id: secondTurn, status: "completed" },
                ],
                events: call.params.cursor ? [] : events,
                next_cursor: 2,
                interactions: [],
                proposals: [],
                executions: [],
              },
        },
      });
    }
    return route.fallback();
  });
  await page.goto("/?tab=discussion");
  await page.getByLabel("Agent", { exact: true }).selectOption(agent);
  const role = page.getByLabel("讨论轮次", { exact: true });
  await expect(role).toBeVisible();
  await expect(role.locator("option")).toHaveText([
    "全部轮次",
    "abcdef01 · completed",
    "abcdef01 · completed",
  ]);
  await expect(
    page.getByRole("region", { name: "会话与流程关联" }),
  ).toContainText("会话 ID：12345678");
  await expect(
    page.getByRole("region", { name: "会话与流程关联" }),
  ).not.toContainText(sessionId);
  await role.selectOption(secondTurn);
  await expect(role).toHaveValue(secondTurn);
  const log = page.getByRole("log", { name: "讨论记录" });
  await expect(log).toContainText("第二轮 · abcdef01");
  await expect(log).not.toContainText("第一轮");
  await role.selectOption(firstTurn);
  await expect(log).toContainText("第一轮 · abcdef01");
  await expect(log).toContainText("实例 12345678");
  await expect(
    log.getByRole("link", { name: "引用 fedcba98", exact: true }),
  ).toHaveAttribute("href", `https://example.com/snapshot/${snapshot}`);
  expect(await page.locator("body").innerText()).not.toMatch(
    /[0-9a-f]{8}-[0-9a-f]{4}-/i,
  );
  const event = log.getByRole("link", { name: "原始事件 ↗", exact: true });
  await expect(event).toHaveAttribute(
    "href",
    new RegExp(`diagnostic_turn_id=${firstTurn}`),
  );
  const popup = page.waitForEvent("popup");
  await event.click();
  const raw = await popup;
  await expect(raw.getByLabel("完整原始 JSON")).toContainText(
    '"session_id": "12345678"',
  );
  await expect(raw.getByLabel("完整原始 JSON")).toContainText(
    '"resource": "fedcba98"',
  );
  expect(await raw.locator("body").innerText()).not.toMatch(
    /[0-9a-f]{8}-[0-9a-f]{4}-/i,
  );
  await raw.evaluate(() => {
    navigator.clipboard.writeText = async (text: string) => {
      sessionStorage.setItem("uuid-copied-json", text);
    };
  });
  await raw.getByRole("button", { name: "复制 JSON", exact: true }).click();
  await expect
    .poll(() => raw.evaluate(() => sessionStorage.getItem("uuid-copied-json")))
    .toBe(JSON.stringify(rawValue, null, 2));
  expect(reads.length).toBeGreaterThan(0);
  expect(reads.every((id) => id === sessionId)).toBe(true);
  await raw.close();
});

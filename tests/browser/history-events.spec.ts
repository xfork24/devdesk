import { expect, type Page } from "@playwright/test";
import { test } from "./support/kernel";
import { useMockCodexCatalog } from "./support/model-catalog";
import { displayText } from "../../web/src/sdk/display-text";

useMockCodexCatalog();

async function rpc(page: Page, method: string, params: object = {}) {
  const value = await page.request
    .post("/rpc", { data: { jsonrpc: "2.0", id: 1, method, params } })
    .then((response) => response.json());
  expect(value.error, JSON.stringify(value)).toBeUndefined();
  return value.result;
}

test("真实历史短事件不提供展开，长事件可展开收起且隔离项目与 Global", async ({
  page,
}, info) => {
  await page.goto("/");
  const project = await rpc(page, "project.create", {
    command_id: crypto.randomUUID(),
    payload: { title: "事件展开测试项目" },
  });
  await page.goto(`/?tab=history&project_id=${project.id}`);
  const history = page.getByRole("region", { name: "历史记录", exact: true });
  const detail = history.locator(".history-event-detail");
  for (const scope of ["project", "global"] as const) {
    await page.getByLabel("历史范围", { exact: true }).selectOption(scope);
    const params = {
      scope,
      ...(scope === "project" ? { project_id: project.id } : {}),
    };
    const list = await rpc(page, "query.history", {
      ...params,
      include_tool_calls: true,
      view: "summary",
      limit: 50,
    });
    await expect(history.locator(".history-row")).toHaveCount(
      list.records.length,
    );
    const shortIndex = list.records.findIndex(
      (row: any) => row.type === "event" && row.event.metadata_only === false,
    );
    const longIndex = list.records.findIndex(
      (row: any) => row.type === "event" && row.event.metadata_only === true,
    );
    expect(shortIndex).toBeGreaterThanOrEqual(0);
    expect(longIndex).toBeGreaterThanOrEqual(0);
    const short = history.locator(".history-row").nth(shortIndex);
    await expect(short.locator("code")).toHaveCount(0);
    await expect(short.getByRole("button")).toHaveCount(0);
    await expect(short).toContainText(
      displayText(list.records[shortIndex].event.detail),
    );
    const long = history.locator(".history-row").nth(longIndex);
    const event = list.records[longIndex].event;
    await expect(detail).toHaveCount(0);
    const expandedResponse = page.waitForResponse((response) => {
      const call = response.request().postDataJSON();
      return (
        response.url().endsWith("/rpc") &&
        call?.method === "query.history" &&
        call.params.event_id === event.id
      );
    });
    await long.getByRole("button", { name: "展开事件", exact: true }).click();
    const response = await expandedResponse;
    expect(response.request().postDataJSON().params).toMatchObject(params);
    if (scope === "global")
      expect(
        response.request().postDataJSON().params.project_id,
      ).toBeUndefined();
    const exact = (await response.json()).result.events[0];
    expect(exact.id).toBe(event.id);
    const toggle = long.getByRole("button", {
      name: "收起事件",
      exact: true,
    });
    await expect(toggle).toHaveAttribute("aria-expanded", "true");
    await expect(detail).toBeVisible();
    await expect(toggle).toHaveAttribute(
      "aria-controls",
      (await detail.getAttribute("id"))!,
    );
    expect([...exact.detail].length).toBeGreaterThan(512);
    await expect(detail).toHaveText(
      `持久化事件 · ${displayText(exact.detail)}`,
    );
    await history
      .getByRole("button", { name: "刷新历史记录", exact: true })
      .click();
    await expect(detail).toBeVisible();
    await page.screenshot({
      path: info.outputPath(`history-event-${scope}-expanded.png`),
      fullPage: true,
    });
    await toggle.click();
    await expect(detail).toHaveCount(0);
    await expect(long.locator("p")).toHaveText(
      `持久化事件 · ${displayText(event.detail)}`,
    );
    await long.getByRole("button", { name: "展开事件", exact: true }).click();
    await expect(detail).toBeVisible();
  }
});

test("历史事件读取失败可重试，展开显示超出摘要的完整多行正文", async ({
  page,
}, info) => {
  let attempts = 0;
  const event = {
    id: 27,
    target: "fixture-target",
    kind: "fixture.multiline",
    detail: `${"完整日志内容".repeat(150)}\n最后一行记录`,
    time: 1000,
  };
  await page.route("**/rpc", async (route) => {
    const call = route.request().postDataJSON();
    if (call.method !== "query.history") return route.fallback();
    if (call.params.event_id !== undefined) {
      expect(call.params).toMatchObject({
        scope: "global",
        event_id: event.id,
        limit: 1,
      });
      expect(call.params.view).toBeUndefined();
      attempts += 1;
      await route.fulfill({
        json: {
          jsonrpc: "2.0",
          id: call.id,
          ...(attempts === 1
            ? { error: { message: "事件读取暂时失败" } }
            : { result: { events: [event], total: 1 } }),
        },
      });
      return;
    }
    await route.fulfill({
      json: {
        jsonrpc: "2.0",
        id: call.id,
        result: {
          records: [
            {
              type: "event",
              time: event.time,
              event: {
                ...event,
                detail: event.detail.slice(0, 512),
                metadata_only: true,
              },
            },
          ],
          total: 1,
        },
      },
    });
  });
  await page.goto("/?tab=history&history_scope=global");
  const history = page.getByRole("region", { name: "历史记录", exact: true });
  const expand = history.getByRole("button", { name: "展开事件", exact: true });
  const detail = history.locator(".history-event-detail");
  await expect(history).not.toContainText("最后一行记录");
  await expand.click();
  await expect(history.getByRole("alert")).toContainText("事件读取暂时失败");
  await expect(detail).toHaveCount(0);
  await expect(expand).toBeEnabled();
  await expand.click();
  await expect(history.getByRole("alert")).toHaveCount(0);
  await expect(detail).toHaveText(`持久化事件 · ${event.detail}`);
  expect(attempts).toBe(2);
  await page.setViewportSize({ width: 760, height: 1000 });
  await expect(detail).toBeVisible();
  expect(
    await page.evaluate(
      () => document.documentElement.scrollWidth <= innerWidth,
    ),
  ).toBe(true);
  await page.screenshot({
    path: info.outputPath("history-event-multiline-expanded.png"),
    fullPage: true,
  });
  await history.getByRole("button", { name: "收起事件", exact: true }).click();
  await expect(history).not.toContainText("最后一行记录");
});

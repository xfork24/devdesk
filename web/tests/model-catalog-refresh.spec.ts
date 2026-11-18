import { expect } from "@playwright/test";
import { test } from "./support/kernel";

test("模型目录刷新在配置版本更新后仍发出真实重试并保留草稿", async ({
  page,
}) => {
  let identities = 0;
  let catalogs = 0;
  let holdRefresh = false;
  let refreshHeld = false;
  let release!: () => void;
  const held = new Promise<void>((resolve) => {
    release = resolve;
  });
  await page.route("**/rpc", async (route) => {
    const call = route.request().postDataJSON();
    if (call.method === "query.model_catalog_identity") {
      identities += 1;
      if (holdRefresh && !refreshHeld) {
        refreshHeld = true;
        await held;
      }
      return route.fulfill({
        json: {
          jsonrpc: "2.0",
          id: call.id,
          result: { identity: "refresh-account", available: true },
        },
      });
    }
    if (call.method !== "codex.auth.models") return route.fallback();
    catalogs += 1;
    return route.fulfill({
      json: {
        jsonrpc: "2.0",
        id: call.id,
        ...(catalogs === 1
          ? { error: { message: "controlled catalog failure" } }
          : { result: { models: [{ model: "private-refreshed" }] } }),
      },
    });
  });
  try {
    await page.goto(
      "/?tab=components&config_tab=model&config_object=builtin-model-gpt-6-1-sol-high%3A1",
    );
    const panel = page.getByRole("region", { name: "Model 管理" });
    await expect(
      panel
        .getByRole("alert")
        .filter({ hasText: "controlled catalog failure" }),
    ).toBeVisible();
    await panel.getByLabel("名称", { exact: true }).fill("刷新期间保留草稿");
    const identitiesBeforeRefresh = identities;
    holdRefresh = true;
    await panel
      .getByRole("button", { name: "刷新模型目录", exact: true })
      .click();
    await expect.poll(() => refreshHeld).toBe(true);
    const result = await page.request
      .post("/rpc", {
        data: {
          jsonrpc: "2.0",
          id: 1,
          method: "role.create",
          params: {
            command_id: crypto.randomUUID(),
            payload: {
              name: "Invalidate configuration during model refresh",
              scope: "global",
              data: {
                instructions: "Trigger a committed configuration version",
              },
            },
          },
        },
      })
      .then((response) => response.json());
    expect(result.error).toBeUndefined();
    // The committed SSE version replaces the request prop while the explicit
    // refresh is still waiting for account identity, before discovery starts.
    await expect
      .poll(() => identities)
      .toBeGreaterThan(identitiesBeforeRefresh + 1);
    await expect(
      panel
        .getByLabel("模型标识", { exact: true })
        .locator('option[value="private-refreshed"]'),
    ).toHaveCount(1);
    expect(catalogs).toBe(2);
    await expect(panel.getByLabel("名称", { exact: true })).toHaveValue(
      "刷新期间保留草稿",
    );
    release();
    await expect(
      panel
        .getByRole("alert")
        .filter({ hasText: "controlled catalog failure" }),
    ).toHaveCount(0);
  } finally {
    release();
  }
});

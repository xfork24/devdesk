import { test, type Page } from "@playwright/test";

export function useMockCodexCatalog() {
  test.beforeEach(async ({ page }: { page: Page }) => {
    await page.route("**/rpc", async (route) => {
      const call = route.request().postDataJSON();
      if (call.method !== "codex.auth.models") return route.fallback();
      return route.fulfill({
        json: {
          jsonrpc: "2.0",
          id: call.id,
          result: {
            models: [{ model: "gpt-6.1-sol" }, { model: "fixture-model" }],
          },
        },
      });
    });
  });
}

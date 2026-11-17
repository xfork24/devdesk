import { expect } from "@playwright/test";
import { test } from "./support/kernel";
import { useMockCodexCatalog } from "./support/model-catalog";

useMockCodexCatalog();

for (const [group, kind, label] of [
  ["workflows", "spec", "Spec"],
  ["workflows", "delivery_type", "Delivery"],
  ["actors", "role", "Role"],
  ["actors", "capability", "Capability"],
  ["actors", "agent", "Agent"],
  ["components", "model", "Model"],
  ["components", "harness", "Harness"],
  ["components", "sandbox", "Sandbox"],
  ["components", "skills", "Skills"],
] as const) {
  test(`${label} 切换选择、补载详情和重载后保持列表顺序与选中状态`, async ({
    page,
  }) => {
    await page.goto(`/?tab=${group}&config_tab=${kind}`);
    const list = page.locator(".configuration-list");
    const entries = list.locator("button:has(strong)");
    await expect(entries.first()).toBeVisible();
    const names = await entries.locator("strong").allTextContents();
    expect(names.length).toBeGreaterThan(0);

    for (const index of [
      0,
      Math.floor(names.length / 2),
      names.length - 1,
      0,
    ]) {
      const entry = entries.filter({ hasText: names[index] });
      await entry.click();
      await expect(entry).toHaveClass(/selected/);
      await expect(list.locator("button.selected:has(strong)")).toHaveCount(1);
      await expect(entries.locator("strong")).toHaveText(names);
    }

    // Restoring a saved selection also loads the full record over the index.
    await page.reload();
    await expect(entries.filter({ hasText: names[0] })).toHaveClass(/selected/);
    await expect(entries.locator("strong")).toHaveText(names);
  });
}

test("Workflow 查看后返回列表保持原顺序和选中状态", async ({ page }) => {
  await page.goto("/?tab=workflows&config_tab=workflow");
  const list = page.getByRole("region", { name: "工作流程管理" });
  const entries = list.locator(".configuration-list button:has(strong)");
  await expect(entries.first()).toBeVisible();
  const names = await entries.locator("strong").allTextContents();
  expect(names.length).toBeGreaterThan(1);

  for (const name of [names[0], names.at(-1)!, names[0]]) {
    await entries.filter({ hasText: name }).click();
    await list
      .getByRole("button", { name: "返回流程列表", exact: true })
      .click();
    await expect(entries.locator("strong")).toHaveText(names);
    await expect(entries.filter({ hasText: name })).toHaveClass(/selected/);
  }
});

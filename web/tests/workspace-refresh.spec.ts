import { test, expect } from "@playwright/test";
import { mkdtempSync } from "node:fs";
import { tmpdir } from "node:os";
import { join } from "node:path";

test("frequent workflow events cannot starve a slower workspace response", async ({ page }) => {
  await page.goto("/");
  const capabilities = await page.request.get("/capabilities").then(r => r.json());
  if (capabilities.setup_required) {
    await page.getByLabel("数据目录").fill(mkdtempSync(join(tmpdir(), "devdesk-refresh-")));
    await page.getByRole("button", { name: /初始化/ }).click();
    await expect(page.getByLabel("数据目录")).toHaveCount(0);
  }
  const response = await page.request.post("/rpc", {
    data: { jsonrpc: "2.0", id: 1, method: "query.workspace", params: {} },
  }).then(r => r.json());
  const project = { id: "slow-workspace", title: "持续事件中的慢工作区", revision: 1,
    execution_mode: "live", concurrency: 2,
    workspace_definition: { repository: "fixture/slow-workspace", mainline: "main" } };
  let active = 0;
  let maximum = 0;
  await page.route("**/capabilities", route => route.fulfill({ json: {
    setup_required: false, active_project_id: project.id,
  }}));
  await page.route("**/rpc", async route => {
    const call = route.request().postDataJSON();
    if (call.method !== "query.workspace") return route.fallback();
    active++;
    maximum = Math.max(maximum, active);
    try {
      await new Promise(resolve => setTimeout(resolve, 800));
      await route.fulfill({ json: {jsonrpc: "2.0", id: call.id, result: {
        ...response.result, active_project: project, projects: [project], total_projects: 1,
        repository_ready: true, configuration_ready: true, features: [], total_features: 0,
        configurations: [], detail: null,
      }}});
    } finally { active--; }
  });
  // UI scheduling regression only: SSE is faster than the controlled response delay.
  // Real project execution, Git and model behavior are verified separately.
  await page.addInitScript(() => {
    Object.defineProperty(window, "EventSource", { value: class {
      onopen: (() => void) | null = null;
      onmessage: (() => void) | null = null;
      onerror: (() => void) | null = null;
      timer: ReturnType<typeof setInterval>;
      constructor() {
        setTimeout(() => this.onopen?.(), 0);
        this.timer = setInterval(() => this.onmessage?.(), 200);
      }
      close() { clearInterval(this.timer); }
    }});
  });
  await page.goto("/?tab=execution");
  await expect(page.getByRole("banner").getByText(project.title, {exact: true})).toBeVisible({timeout: 5000});
  expect(maximum).toBe(1);
});

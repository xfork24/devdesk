import { test, expect } from "@playwright/test";
import { mkdtempSync } from "node:fs";
import { tmpdir } from "node:os";
import { join } from "node:path";

test("frequent workflow events cannot starve a slower workspace response", async ({
  page,
}) => {
  await page.goto("/");
  const capabilities = await page.request
    .get("/capabilities")
    .then((r) => r.json());
  if (capabilities.setup_required) {
    await page
      .getByLabel("数据目录")
      .fill(mkdtempSync(join(tmpdir(), "devdesk-refresh-")));
    await page.getByRole("button", { name: /初始化/ }).click();
    await expect(page.getByLabel("数据目录")).toHaveCount(0);
  }
  const response = await page.request
    .post("/rpc", {
      data: { jsonrpc: "2.0", id: 1, method: "query.workspace", params: {} },
    })
    .then((r) => r.json());
  const project = {
    id: "slow-workspace",
    title: "持续事件中的慢工作区",
    revision: 1,
    execution_mode: "live",
    concurrency: 2,
    workspace_definition: {
      repository: "fixture/slow-workspace",
      mainline: "main",
    },
  };
  let active = 0;
  let maximum = 0;
  await page.route("**/capabilities", (route) =>
    route.fulfill({
      json: {
        setup_required: false,
        active_project_id: project.id,
      },
    }),
  );
  await page.route("**/rpc", async (route) => {
    const call = route.request().postDataJSON();
    if (call.method !== "query.workspace") return route.fallback();
    active++;
    maximum = Math.max(maximum, active);
    try {
      await new Promise((resolve) => setTimeout(resolve, 800));
      await route.fulfill({
        json: {
          jsonrpc: "2.0",
          id: call.id,
          result: {
            ...response.result,
            active_project: project,
            projects: [project],
            total_projects: 1,
            repository_ready: true,
            configuration_ready: true,
            features: [],
            total_features: 0,
            configurations: [],
            detail: null,
          },
        },
      });
    } finally {
      active--;
    }
  });
  // UI scheduling regression only: SSE is faster than the controlled response delay.
  // Real project execution, Git and model behavior are verified separately.
  await page.addInitScript(() => {
    Object.defineProperty(window, "EventSource", {
      value: class {
        onopen: (() => void) | null = null;
        onmessage: (() => void) | null = null;
        onerror: (() => void) | null = null;
        timer: ReturnType<typeof setInterval>;
        constructor() {
          setTimeout(() => this.onopen?.(), 0);
          this.timer = setInterval(() => this.onmessage?.(), 200);
        }
        close() {
          clearInterval(this.timer);
        }
      },
    });
  });
  await page.goto("/?tab=execution");
  await expect(
    page.getByRole("banner").getByText(project.title, { exact: true }),
  ).toBeVisible({ timeout: 5000 });
  expect(maximum).toBe(1);
});

test("resource preference events refresh the configuration shell without changing its source clock", async ({
  page,
}) => {
  await page.goto("/");
  if (
    (
      await page.request
        .get("/capabilities")
        .then((response) => response.json())
    ).setup_required
  ) {
    await page
      .getByLabel("数据目录")
      .fill(mkdtempSync(join(tmpdir(), "devdesk-resource-refresh-")));
    await page.getByRole("button", { name: /初始化/ }).click();
    await expect(page.getByLabel("数据目录")).toHaveCount(0);
  }
  // Only the notification is controlled. Preference publication and both shell
  // queries use the real isolated HTTP/SQLite host.
  await page.addInitScript(() => {
    Object.defineProperty(window, "EventSource", {
      value: class {
        onopen: (() => void) | null = null;
        onmessage: ((event: { data: string }) => void) | null = null;
        onerror: (() => void) | null = null;
        listener = (event: Event) =>
          this.onmessage?.({
            data: JSON.stringify((event as CustomEvent).detail),
          });
        constructor() {
          window.addEventListener("test:kernel-event", this.listener);
          setTimeout(() => this.onopen?.(), 0);
        }
        close() {
          window.removeEventListener("test:kernel-event", this.listener);
        }
      },
    });
  });
  let reads = 0;
  let harness = "";
  let catalogVersion = "";
  await page.route("**/rpc", async (route) => {
    if (route.request().postDataJSON().method !== "query.ui_bootstrap")
      return route.fallback();
    const response = await route.fetch();
    const body = await response.json();
    reads++;
    harness = body.result.default_harness;
    catalogVersion = JSON.stringify(body.result.catalog_version);
    await route.fulfill({ response });
  });
  await page.goto("/?tab=actors");
  await expect.poll(() => reads).toBeGreaterThan(0);
  const before = reads;
  const clock = catalogVersion;
  const next = harness === "codex" ? "dsh" : "codex";
  const snapshot = await page.request
    .post("/rpc", {
      data: { jsonrpc: "2.0", id: 1, method: "query.settings", params: {} },
    })
    .then((response) => response.json());
  const guard = snapshot.result.versions.find(
    (version: any) => version.resource.id === "default_harness",
  );
  const response = await page.request
    .post("/rpc", {
      data: {
        jsonrpc: "2.0",
        id: 2,
        method: "settings.update",
        params: {
          command_id: crypto.randomUUID(),
          preconditions: [guard],
          payload: { default_harness: next },
        },
      },
    })
    .then((response) => response.json());
  expect(response.error).toBeUndefined();
  await page.evaluate(
    (detail) =>
      window.dispatchEvent(new CustomEvent("test:kernel-event", { detail })),
    {
      kind: "resource.changed",
      detail: JSON.stringify(response.meta.commit.versions),
    },
  );
  await expect.poll(() => reads).toBeGreaterThan(before);
  await expect.poll(() => harness).toBe(next);
  expect(catalogVersion).toBe(clock);
});

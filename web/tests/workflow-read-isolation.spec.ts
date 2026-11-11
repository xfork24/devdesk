import { test, expect, type Page } from "@playwright/test";
import { mkdtempSync } from "node:fs";
import { tmpdir } from "node:os";
import { join } from "node:path";

// Controlled UI responses; actual execution and publication are verified separately.
async function fixture(page: Page, failure?: "catalog" | "defaults") {
  await page.goto("/");
  if (
    (await page.request.get("/capabilities").then((r) => r.json()))
      .setup_required
  ) {
    await page
      .getByLabel("数据目录")
      .fill(mkdtempSync(join(tmpdir(), "devdesk-read-ui-")));
    await page.getByRole("button", { name: "初始化 / 打开数据集" }).click();
    await expect(page.getByLabel("数据目录")).toHaveCount(0);
  }
  const original = await page.request
    .post("/rpc", {
      data: { jsonrpc: "2.0", id: 1, method: "query.workspace", params: {} },
    })
    .then((r) => r.json());
  const project = {
    id: "read-project",
    title: "读取隔离项目",
    revision: 1,
    workspace_definition: { repository: "fixture/reads", mainline: "main" },
  };
  const feature = {
    id: "read-feature",
    title: "读取隔离 Feature",
    status: "running",
    revision: 1,
  };
  const document = { stages: [], steps: [] };
  const instance = {
    id: "read-instance",
    kind: "workflow_instance",
    name: "读取隔离流程",
    revision: 1,
    data: {
      owner_id: feature.id,
      parent_execution_id: null,
      status: "running",
      paused: false,
      steps: [],
      plan: { document, standard_ref: "standard-current:1" },
      active_plan_revision_id: "plan-current",
      role_agent_bindings: {},
    },
  };
  const history = ["standard-old:1", "standard-current:1"].map(
    (ref, index) => ({
      id: `plan-${index}`,
      data: { standard_ref: ref, reason: `采用记录 ${index + 1}` },
    }),
  );
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
    let result: any;
    switch (call.method) {
      case "query.workspace":
        result = {
          ...original.result,
          active_project: project,
          projects: [project],
          total_projects: 1,
          repository_ready: true,
          configuration_ready: true,
          features: [feature],
          total_features: 1,
          configurations: [instance],
          detail: {
            feature,
            tasks: [],
            documents: [],
            snapshots: [],
            contexts: [],
            totals: {},
          },
        };
        break;
      case "query.workflow_instance":
        result = {
          record: instance,
          runtime: instance.data,
          plan: document,
          history: [],
          outputs: {},
        };
        break;
      case "query.work_configuration":
        result = { configuration: null, history };
        break;
      case "query.project_configuration":
        result = { configuration: null };
        break;
      case "query.configurations":
        result = { records: [], total: 0 };
        break;
      case "query.standard_catalog":
        result = { records: [] };
        break;
      case "query.workflow_requirements":
        result = { roles: [] };
        break;
      case "query.orchestration":
        result = { record: null };
        break;
      default:
        return route.fallback();
    }
    if (
      (failure === "catalog" && call.method === "query.configurations") ||
      (failure === "defaults" && call.method === "query.project_configuration")
    ) {
      await new Promise((resolve) => setTimeout(resolve, 200));
      return route.fulfill({
        json: {
          jsonrpc: "2.0",
          id: call.id,
          error: {
            code: -32000,
            message: "controlled read failure",
            data: { code: "read_failed" },
          },
        },
      });
    }
    await route.fulfill({ json: { jsonrpc: "2.0", id: call.id, result } });
  });
  return "/?tab=execution&project_id=read-project&feature_id=read-feature";
}

test("slow controller polling renders its result without overlapping requests and stops on unmount", async ({
  page,
}) => {
  const url = await fixture(page);
  let active = 0,
    maximum = 0,
    calls = 0;
  await page.route("**/rpc", async (route) => {
    const call = route.request().postDataJSON();
    if (call.method !== "query.orchestration") return route.fallback();
    calls++;
    active++;
    maximum = Math.max(maximum, active);
    try {
      await new Promise((resolve) => setTimeout(resolve, 4000));
      await route.fulfill({
        json: {
          jsonrpc: "2.0",
          id: call.id,
          result: {
            record: {
              id: "controller",
              revision: 1,
              data: { status: "active", reason: "slow controller observed" },
            },
            calls: [],
            messages: [],
            decisions: [],
          },
        },
      });
    } finally {
      active--;
    }
  });
  await page.goto(url);
  await expect(
    page
      .getByRole("region", { name: "项目经理协作" })
      .getByText("slow controller observed"),
  ).toBeVisible({ timeout: 6500 });
  expect(maximum).toBe(1);
  await page.getByRole("button", { name: "工作总览", exact: true }).click();
  const before = calls;
  await page.waitForTimeout(3000);
  expect(calls).toBe(before);
});

for (const failure of ["catalog", "defaults"] as const) {
  test(`Standard history remains visible when ${failure} loading fails`, async ({
    page,
  }) => {
    const url = await fixture(page, failure);
    await page.goto(url);
    const standard = page
      .getByRole("region", { name: "当前工作流", exact: true })
      .locator(".work-standard");
    await standard.locator(":scope > summary").click();
    await standard.getByText("规范采用历史（2）", { exact: true }).click();
    await expect(
      standard.getByText("standard-old:1 · 采用记录 1", { exact: false }),
    ).toBeVisible();
    await expect(standard.getByRole("alert")).toContainText(
      "controlled read failure",
    );
    await expect(
      standard.getByText("standard-old:1 · 采用记录 1", { exact: false }),
    ).toBeVisible();
  });
}

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
    type: "workflow_instance",
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
      case "query.workflow_driver":
        result = { driver: null };
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

test("paused root manager adopts exact successor sources through the Human resume entry", async ({
  page,
}) => {
  const url = await fixture(page);
  let adopted: Record<string, unknown> | undefined;
  await page.route("**/rpc", async (route) => {
    const call = route.request().postDataJSON();
    let result: any;
    if (call.method === "query.orchestration") {
      result = {
        record: {
          id: "controller",
          revision: adopted ? 2 : 1,
          data: {
            status: adopted ? "active" : "paused",
            epoch: adopted ? 4 : 3,
            role_ref: "manager-role:1",
            agent_revision_id: "manager-old:1",
            permission_context_ref: "context-old:1",
          },
        },
        calls: [],
        messages: [],
        decisions: [],
      };
    } else if (call.method === "query.configurations") {
      result = {
        records: [
          {
            id: "manager-new:1",
            type: "agent",
            definition_id: "manager-new",
            name: "Selected manager",
            version: 1,
            scope: "global",
            role_ref: "manager-role:1",
            data: {},
          },
          {
            id: "context-new:1",
            type: "permission_context",
            definition_id: "context-new",
            name: "Selected budget",
            version: 1,
            scope: "global",
            data: {},
          },
        ],
        total: 2,
      };
    } else if (call.method === "orchestration.resume") {
      adopted = call.params.payload;
      result = { id: "controller" };
    } else return route.fallback();
    await route.fulfill({ json: { jsonrpc: "2.0", id: call.id, result } });
  });
  await page.goto(url);
  const panel = page.getByRole("region", { name: "项目经理协作" });
  await panel
    .getByLabel("经理 Agent 精确修订", { exact: true })
    .selectOption("manager-new:1");
  await panel
    .getByLabel("经理授权上下文", { exact: true })
    .selectOption("context-new:1");
  await panel
    .getByRole("button", { name: "采用并恢复经理", exact: true })
    .click();
  await expect
    .poll(() => adopted)
    .toEqual({
      epoch: 3,
      agent_ref: "manager-new:1",
      permission_context_ref: "context-new:1",
    });
  await expect(
    panel.getByRole("button", { name: "采用并恢复经理", exact: true }),
  ).toBeDisabled();
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

test("a late defaults read preserves an already displayed Standard comparison", async ({
  page,
}) => {
  const url = await fixture(page);
  let releaseDefaults!: () => void;
  const defaults = new Promise<void>((resolve) => {
    releaseDefaults = resolve;
  });
  const selection = {
    standard_ref: "standard-current:1",
    children: [],
    exceptions: [],
  };
  const diff = {
    equal: false,
    changes: [{ kind: "definition_changed", source_kind: "workflow" }],
  };
  await page.route("**/rpc", async (route) => {
    const call = route.request().postDataJSON();
    let result: any;
    if (call.method === "query.project_configuration") {
      await defaults;
      result = { configuration: null };
    } else if (call.method === "query.work_standard_choices") {
      result = {
        records: [
          {
            id: "other-work",
            project_title: "Fixture",
            title: "Other work",
            work: { kind: "task" },
          },
        ],
        total: 1,
      };
    } else if (call.method === "query.work_standard_diff") {
      result = { entry_diff: diff, diff, before: selection, after: selection };
    } else return route.fallback();
    await route.fulfill({ json: { jsonrpc: "2.0", id: call.id, result } });
  });
  await page.goto(url);
  const standard = page
    .getByRole("region", { name: "当前工作流", exact: true })
    .locator(".work-standard");
  await standard.locator(":scope > summary").click();
  await standard.getByText("与其他工作比较规范", { exact: true }).click();
  await standard
    .getByLabel("比较工作", { exact: true })
    .selectOption("other-work");
  await standard
    .getByRole("button", { name: "比较工作规范", exact: true })
    .click();
  await expect(standard.locator(".standard-diff").first()).toContainText(
    "1 项规范变化",
  );
  releaseDefaults();
  // This selection is set by the late read's callback, so the assertion runs after it.
  await expect(
    standard.getByLabel("采用 Standard", { exact: true }),
  ).toHaveValue("standard-current:1");
  await expect(standard.locator(".standard-diff").first()).toContainText(
    "1 项规范变化",
  );
});

for (const catalogState of ["pending", "failed"] as const) {
  test(`manager runtime remains visible when configuration catalog is ${catalogState}`, async ({
    page,
  }) => {
    const url = await fixture(page);
    let release!: () => void;
    const blocked = new Promise<void>((resolve) => {
      release = resolve;
    });
    await page.route("**/rpc", async (route) => {
      const call = route.request().postDataJSON();
      let result: any;
      if (call.method === "query.configurations") {
        if (catalogState === "pending") await blocked;
        return route.fulfill({
          json: {
            jsonrpc: "2.0",
            id: call.id,
            error: {
              code: -32000,
              message: "catalog unavailable",
              data: { code: "read_failed" },
            },
          },
        });
      } else if (call.method === "query.orchestration") {
        result = {
          record: {
            id: "controller",
            revision: 1,
            data: {
              status: "active",
              epoch: 5,
              reason: "actual manager is active",
            },
          },
          calls: [],
          messages: [],
          decisions: [],
        };
      } else if (call.method === "query.workflow_driver") {
        result = { driver: { data: { mode: "agent", epoch: 7 } } };
      } else return route.fallback();
      await route.fulfill({ json: { jsonrpc: "2.0", id: call.id, result } });
    });
    try {
      await page.goto(url);
      const panel = page.getByRole("region", { name: "项目经理协作" });
      await expect(
        panel.getByText("当前调度：agent · 代次 7", { exact: true }),
      ).toBeVisible({ timeout: 6500 });
      await expect(
        panel.getByText("actual manager is active", { exact: true }),
      ).toBeVisible();
      await expect(
        panel.getByRole("button", { name: "人工接管", exact: true }),
      ).toBeEnabled();
      await expect(panel).not.toContainText("当前调度：manual");
      await expect(panel).not.toContainText("发送指令后启动项目经理");
      if (catalogState === "failed")
        await expect(panel.getByRole("alert")).toContainText(
          "配置选项暂不可用",
        );
    } finally {
      release();
    }
  });
}

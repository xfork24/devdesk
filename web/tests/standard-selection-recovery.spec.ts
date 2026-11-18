import { expect, type Page } from "@playwright/test";
import { test } from "./support/kernel";

async function rpc(page: Page, method: string, params: any = {}) {
  const response = await page.request.post("/rpc", {
    data: { jsonrpc: "2.0", id: 1, method, params },
  });
  const value = await response.json();
  expect(value.error, JSON.stringify(value)).toBeUndefined();
  return value.result;
}

async function projectWithoutConfiguration(page: Page) {
  await page.goto("/");
  return rpc(page, "project.create", {
    command_id: crypto.randomUUID(),
    payload: { title: "尚未配置规范的导入项目", repository: "fixture/import" },
  });
}

test("未配置项目打开已失效的规范链接时清理旧引用并显示可用快照", async ({
  page,
}) => {
  const project = await projectWithoutConfiguration(page);
  const reads: string[] = [];
  page.on("request", (request) => {
    if (!request.url().endsWith("/rpc")) return;
    const call = request.postDataJSON();
    if (["query.standard", "query.standard_usage"].includes(call.method))
      reads.push(call.params.id);
  });
  await page.goto(
    `/?tab=workflows&config_tab=standard&project_id=${project.id}&standard_id=builtin-standard-engineering%3A8`,
  );
  const panel = page.getByRole("region", { name: "Standard 规范快照" });
  const select = panel.getByLabel("查看 Standard", { exact: true });
  await expect(select).not.toHaveValue("");
  await expect(select).not.toHaveValue("builtin-standard-engineering:8");
  await expect(panel.getByText(/不可变规范快照 ·/)).toBeVisible();
  await expect(panel.getByRole("alert")).toHaveCount(0);
  await expect(select.locator("option")).not.toContainText(["固定历史修订"]);
  await expect(page).not.toHaveURL(/builtin-standard-engineering%3A8/i);
  expect(reads).not.toContain("builtin-standard-engineering:8");
  const selection = await rpc(page, "query.project_configuration", {
    project_id: project.id,
  });
  expect(selection.configuration).toBeNull();
});

test("规范目录为空时移除失效链接并显示空状态", async ({ page }) => {
  const project = await projectWithoutConfiguration(page);
  await page.route("**/rpc", async (route) => {
    const call = route.request().postDataJSON();
    if (call.method !== "query.configuration_index") return route.continue();
    const response = await route.fetch();
    const value = await response.json();
    value.result.records = value.result.records.filter(
      (row: any) => row.type !== "standard",
    );
    value.result.total = value.result.records.length;
    await route.fulfill({ response, json: value });
  });
  await page.goto(
    `/?tab=workflows&config_tab=standard&project_id=${project.id}&standard_id=builtin-standard-engineering%3A8`,
  );
  const panel = page.getByRole("region", { name: "Standard 规范快照" });
  await expect(panel.getByLabel("查看 Standard", { exact: true })).toHaveValue(
    "",
  );
  await expect(panel.getByRole("alert")).toHaveCount(0);
  await expect(page).not.toHaveURL(/standard_id=/);
  await expect(panel.getByText("选择快照查看确切规范与关系。")).toBeVisible();
});

test("未配置项目的默认规范已失效时等待选择可用规范", async ({ page }) => {
  const project = await projectWithoutConfiguration(page);
  const defaults = await rpc(page, "query.configuration_defaults", {
    project_id: project.id,
    view: "selection",
  });
  await page.route("**/rpc", async (route) => {
    const call = route.request().postDataJSON();
    if (call.method !== "query.configuration_defaults") return route.continue();
    const response = await route.fetch();
    const value = await response.json();
    value.result.standard_ref = "builtin-standard-engineering:8";
    await route.fulfill({ response, json: value });
  });
  await page.goto(`/?tab=projects&project_id=${project.id}`);
  const panel = page.getByRole("region", { name: "项目配置" });
  const select = panel.getByLabel("基础 Standard", { exact: true });
  await expect(
    select.locator(`option[value="${defaults.standard_ref}"]`),
  ).toHaveCount(1);
  await expect(select).toHaveValue("");
  await expect(select.locator("option")).not.toContainText(["固定历史修订"]);
  await expect(page.getByRole("alert")).toHaveCount(0);
  await expect(
    panel.getByRole("button", { name: "保存项目配置", exact: true }),
  ).toBeDisabled();
});

test("清理未保存项目草稿中的失效规范，保留独立的测试配置", async ({ page }) => {
  const project = await projectWithoutConfiguration(page);
  const defaults = await rpc(page, "query.configuration_defaults", {
    project_id: project.id,
    view: "selection",
  });
  await page.goto(`/?tab=projects&project_id=${project.id}`);
  const panel = page.getByRole("region", { name: "项目配置" });
  await expect(panel.getByLabel("基础 Standard", { exact: true })).toHaveValue(
    defaults.standard_ref,
  );
  const testConfiguration = '{"language":"JavaScript"}';
  await panel
    .getByLabel("项目测试配置", { exact: true })
    .fill(testConfiguration);
  await expect
    .poll(() =>
      page.evaluate((id) => {
        const key = Object.keys(localStorage).find(
          (key) =>
            key.startsWith(`devdesk:standard-project-v2:${id}`) &&
            !key.endsWith(":dataset-owner"),
        );
        return (
          key && JSON.parse(localStorage.getItem(key)!).value.test_configuration
        );
      }, project.id),
    )
    .toBe(testConfiguration);
  await page.evaluate((id) => {
    const key = Object.keys(localStorage).find(
      (key) =>
        key.startsWith(`devdesk:standard-project-v2:${id}`) &&
        !key.endsWith(":dataset-owner"),
    )!;
    const saved = JSON.parse(localStorage.getItem(key)!);
    saved.value.standard_ref = "builtin-standard-engineering:8";
    saved.value.harness_permission_ref = "";
    saved.value.standard_overrides = [
      { source_ref: "missing-spec:1", replacement_ref: "missing-spec:2" },
    ];
    localStorage.setItem(key, JSON.stringify(saved));
  }, project.id);
  const previews: string[] = [];
  page.on("request", (request) => {
    if (!request.url().endsWith("/rpc")) return;
    const call = request.postDataJSON();
    if (call.method === "query.project_standard_preview")
      previews.push(call.params.standard_ref);
  });
  await page.reload();
  await expect(panel.getByLabel("基础 Standard", { exact: true })).toHaveValue(
    defaults.standard_ref,
  );
  await expect(panel.getByLabel("项目测试配置", { exact: true })).toHaveValue(
    testConfiguration,
  );
  await expect(
    panel.getByLabel("Harness Profile", { exact: true }),
  ).toHaveValue(defaults.harness_permission_ref);
  await expect(
    panel.getByText("项目覆盖（0 项）", { exact: true }),
  ).toBeVisible();
  await expect(
    panel.getByRole("button", { name: "保存项目配置", exact: true }),
  ).toBeEnabled();
  await expect(page.getByRole("alert")).toHaveCount(0);
  expect(previews).not.toContain("builtin-standard-engineering:8");
  await page.reload();
  await expect(panel.getByLabel("基础 Standard", { exact: true })).toHaveValue(
    defaults.standard_ref,
  );
  expect(
    (await rpc(page, "query.project_configuration", { project_id: project.id }))
      .configuration,
  ).toBeNull();
});

test("已有的历史规范修订可以通过链接查看且项目草稿继续使用确切版本", async ({
  page,
}) => {
  const project = await projectWithoutConfiguration(page);
  const defaults = await rpc(page, "query.configuration_defaults", {
    project_id: project.id,
    view: "selection",
  });
  const payload = {
    name: "历史规范回归",
    root_workflow_refs: [defaults.workflow_ref],
    default_workflow_ref: defaults.workflow_ref,
    scope: `project:${project.id}`,
  };
  const first = await rpc(page, "standard.capture", {
    project_id: project.id,
    command_id: crypto.randomUUID(),
    payload,
  });
  await rpc(page, "standard.capture", {
    project_id: project.id,
    command_id: crypto.randomUUID(),
    target: first.id,
    expected_revision: first.revision,
    payload,
  });
  await page.goto(
    `/?tab=workflows&config_tab=standard&project_id=${project.id}&standard_id=${encodeURIComponent(first.id)}`,
  );
  const standard = page.getByRole("region", { name: "Standard 规范快照" });
  await expect(
    standard.getByLabel("查看 Standard", { exact: true }),
  ).toHaveValue(first.id);
  await expect(
    standard
      .getByLabel("查看 Standard", { exact: true })
      .locator("option:checked"),
  ).toHaveText("历史规范回归 · v1 · 项目");
  await expect(standard.getByRole("alert")).toHaveCount(0);
  await page.goto(`/?tab=projects&project_id=${project.id}`);
  const panel = page.getByRole("region", { name: "项目配置" });
  const select = panel.getByLabel("基础 Standard", { exact: true });
  await expect(select).toHaveValue(defaults.standard_ref);
  await select.focus();
  await expect(select.locator(`option[value="${first.id}"]`)).toHaveCount(1);
  await select.selectOption(first.id);
  await expect(
    panel.getByRole("button", { name: "保存项目配置", exact: true }),
  ).toBeEnabled();
  await page.reload();
  await expect(select).toHaveValue(first.id);
  await expect(
    panel.getByRole("button", { name: "保存项目配置", exact: true }),
  ).toBeEnabled();
  await expect(page.getByRole("alert")).toHaveCount(0);
});

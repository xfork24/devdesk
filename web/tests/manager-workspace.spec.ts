import { test, expect } from "@playwright/test";
import { mkdtempSync } from "node:fs";
import { tmpdir } from "node:os";
import { join } from "node:path";

test("项目管理只保存配置并统一设置项目经理", async ({ page }) => {
  await page.goto("/");
  const capabilities = await page.request
    .get("/capabilities")
    .then((r) => r.json());
  if (capabilities.setup_required) {
    await page
      .getByLabel("数据目录")
      .fill(mkdtempSync(join(tmpdir(), "devdesk-manager-ui-")));
    await page.getByRole("button", { name: "初始化 / 打开数据集" }).click();
    await expect(page.getByLabel("数据目录")).toHaveCount(0);
  }
  const created = await page.request
    .post("/rpc", {
      data: {
        jsonrpc: "2.0",
        id: 1,
        method: "project.create",
        params: {
          command_id: crypto.randomUUID(),
          payload: {
            title: "Manager without Feature",
            repository: "https://github.com/fixture/manager.git",
          },
        },
      },
    })
    .then((r) => r.json());
  expect(created.error, JSON.stringify(created)).toBeUndefined();
  await page.goto(
    `/?tab=projects&project_id=${encodeURIComponent(created.result.id)}`,
  );
  await page.getByRole("button", { name: "项目管理", exact: true }).click();
  await expect(
    page.getByRole("region", { name: "项目经理工作台" }),
  ).toHaveCount(0);
  await expect(page.getByLabel("工作台 Agent 精确修订")).toHaveCount(0);
  await expect(page.getByLabel("给项目经理的指令")).toHaveCount(0);
  await expect(page.getByText("发送指令后启动项目经理")).toHaveCount(0);
  await expect(
    page.getByRole("button", { name: "新 Feature", exact: true }),
  ).toHaveCount(0);
  const configuration = page.getByRole("region", { name: "项目配置" });
  await expect(
    configuration.getByLabel("编排 Agent", { exact: true }),
  ).toBeVisible();
  await expect(configuration.getByLabel("编排角色")).toHaveCount(0);
  await expect(
    configuration.getByText(
      "Feature Development / Task Development · 项目经理",
    ),
  ).toBeVisible();
  await expect(configuration.getByLabel("流程所需 Role → Agent")).toBeVisible();
  const historyRequested = (kind: string) =>
    page.waitForResponse((response) => {
      const request = response.request();
      if (!request.url().endsWith("/rpc") || request.method() !== "POST")
        return false;
      const call = request.postDataJSON();
      return (
        call.method === "query.configuration_index" &&
        call.params.kind === kind &&
        call.params.include_history === true
      );
    });
  const profileHistory = historyRequested("permission_profile");
  const harness = configuration.getByLabel("Harness Profile", { exact: true });
  await harness.focus();
  await profileHistory;
  const sandbox = configuration.getByLabel("Sandbox Profile", { exact: true });
  await sandbox.focus();
  await expect(harness.locator("option")).not.toContainText(["Sandbox"]);
  await expect(sandbox.locator("option")).not.toContainText(["Harness"]);

  const agentHistory = historyRequested("agent");
  const controller = configuration.getByLabel("编排 Agent", { exact: true });
  await controller.focus();
  await agentHistory;
  await expect(controller.locator("option")).not.toContainText(["Architect"]);
  const stepRoles = configuration.getByRole("group", {
    name: "流程所需 Role → Agent",
  });
  const stepAgents = stepRoles.getByRole("combobox");
  await expect(stepAgents).toHaveCount(6);
  for (const [index, name] of [
    "Requirements Analyst",
    "Software Architect",
    "Software Developer",
    "Project Manager",
    "Independent Reviewer",
    "Test Engineer",
  ].entries()) {
    const choices = stepAgents.nth(index);
    await choices.focus();
    const labels = await choices
      .locator("option[value]:not([value=''])")
      .allTextContents();
    expect(labels.length).toBeGreaterThan(0);
    expect(labels.every((label) => label.startsWith(name))).toBe(true);
  }
  const starts: string[] = [];
  page.on("request", (request) => {
    if (!request.url().endsWith("/rpc") || request.method() !== "POST") return;
    const method = request.postDataJSON().method;
    if (
      [
        "manager.open",
        "orchestration.open",
        "orchestration.input",
        "workflow.start",
        "feature.create",
      ].includes(method)
    )
      starts.push(method);
  });
  await configuration.getByRole("button", { name: "保存项目配置" }).click();
  await expect(
    page.getByText(
      "项目配置已保存，直接使用所选 Standard；已有 Feature 的冻结版本保持不变。",
      { exact: true },
    ),
  ).toBeVisible();
  expect(starts).toEqual([]);
  const result = await page.request
    .post("/rpc", {
      data: {
        jsonrpc: "2.0",
        id: 2,
        method: "query.project_knowledge",
        params: { project_id: created.result.id },
      },
    })
    .then((r) => r.json());
  expect(result.error, JSON.stringify(result)).toBeUndefined();
  expect(result.result.counts.features).toBe(0);
  expect(result.result.counts.tasks).toBe(0);
  await page.getByRole("button", { name: "工作总览", exact: true }).click();
  await expect(
    page.getByRole("button", { name: "新 Feature", exact: true }),
  ).toBeVisible();
  await page.getByRole("button", { name: "执行与交付", exact: true }).click();
  await expect(
    page.getByRole("button", { name: "新 Feature", exact: true }),
  ).toHaveCount(0);
});

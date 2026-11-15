import { expect, type Page } from "@playwright/test";
import { test } from "./support/kernel";

async function rpc(page: Page, method: string, params: object = {}) {
  const response = await page.request.post("/rpc", {
    data: { jsonrpc: "2.0", id: 1, method, params },
  });
  const value = await response.json();
  expect(value.error, JSON.stringify(value)).toBeUndefined();
  return value.result;
}

test("没有项目时可以选择 Agent 并查看完整能力和经验入口", async ({
  page,
}, info) => {
  await page.goto("/");
  const defaults = await rpc(page, "query.configuration_defaults");
  const agent = defaults.role_agent_bindings["builtin-role-analyst:1"];
  const instance = await rpc(page, "agent.instance.create", {
    command_id: crypto.randomUUID(),
    payload: {
      name: "独立需求分析师",
      agent_revision_id: agent,
      scope: "global",
    },
  });
  await page.reload();
  await page.getByRole("button", { name: "角色讨论", exact: true }).click();
  await page.getByLabel("讨论角色", { exact: true }).selectOption(agent);
  await page.getByLabel("Agent", { exact: true }).selectOption(instance.id);
  await expect(page.getByLabel("讨论范围", { exact: true })).toHaveValue(
    "global",
  );
  await expect(
    page.getByRole("button", { name: "开始 / 续用讨论", exact: true }),
  ).toBeEnabled();
  await page.getByText("Agent 完整能力", { exact: true }).click();
  await expect(page.getByText("Agent Specs", { exact: true })).toBeVisible();
  await expect(
    page.getByText("Agent 经验：检索与读取，可在当前讨论中按需使用。", {
      exact: true,
    }),
  ).toBeVisible();
  await page
    .getByText("Agent 经验 · 项目 / Feature / 时间", { exact: true })
    .click();
  await expect(page.getByText(/0\s*条快照/)).toBeVisible();
  const workspace = await rpc(page, "query.workspace");
  expect(workspace.active_project).toBeNull();
  await page.screenshot({
    path: info.outputPath("detached-agent.png"),
    fullPage: true,
  });
});

test("无项目讨论可以选择来源项目的原 Agent 实例并保持活动项目", async ({
  page,
}) => {
  await page.goto("/");
  const defaults = await rpc(page, "query.configuration_defaults");
  const template = defaults.records.find(
    (row: { id: string }) =>
      row.id === defaults.role_agent_bindings["builtin-role-analyst:1"],
  );
  const source = await rpc(page, "project.create", {
    command_id: crypto.randomUUID(),
    payload: { title: "经验来源项目" },
  });
  const agent = await rpc(page, "agent.create", {
    project_id: source.id,
    command_id: crypto.randomUUID(),
    payload: {
      name: "项目私有分析师",
      scope: `project:${source.id}`,
      role_ref: template.role_ref,
      capability_ref: template.capability_ref,
      data: {},
    },
  });
  const instance = await rpc(page, "agent.instance.create", {
    project_id: source.id,
    command_id: crypto.randomUUID(),
    payload: {
      name: "保留原经验的分析师",
      agent_revision_id: agent.id,
      scope: `project:${source.id}`,
    },
  });
  const current = await rpc(page, "project.create", {
    command_id: crypto.randomUUID(),
    payload: { title: "当前活动项目" },
  });
  await page.reload();
  await page.getByRole("button", { name: "角色讨论", exact: true }).click();
  await page.getByLabel("讨论范围", { exact: true }).selectOption("global");
  await page.getByLabel("讨论角色", { exact: true }).selectOption(agent.id);
  await page.getByLabel("Agent", { exact: true }).selectOption(instance.id);
  await expect(page.getByLabel("Agent", { exact: true })).toHaveValue(
    instance.id,
  );
  await expect(
    page
      .getByLabel("Agent", { exact: true })
      .locator(`option[value="${instance.id}"]`),
  ).toContainText("经验来源项目");
  await expect(
    page.getByRole("button", { name: "开始 / 续用讨论", exact: true }),
  ).toBeEnabled();
  expect((await rpc(page, "query.workspace")).active_project.id).toBe(
    current.id,
  );
  expect(
    (
      await rpc(page, "query.agent_experience", {
        scope: "global",
        agent_instance_id: instance.id,
      })
    ).total,
  ).toBe(0);
});

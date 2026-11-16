import { test, expect, type Page } from "@playwright/test";
import { mkdtempSync } from "node:fs";
import { tmpdir } from "node:os";
import { join } from "node:path";

async function rpc(page: Page, method: string, params: any = {}) {
  const result = await page.request
    .post("/rpc", { data: { jsonrpc: "2.0", id: 1, method, params } })
    .then((r) => r.json());
  expect(result.error, JSON.stringify(result)).toBeUndefined();
  return result.result;
}

test("Standard 决定入口，项目覆盖保存来源与差异并在重载后恢复", async ({
  page,
}, info) => {
  await page.goto("/");
  if (
    (await page.request.get("/capabilities").then((r) => r.json()))
      .setup_required
  ) {
    await page
      .getByLabel("数据目录")
      .fill(mkdtempSync(join(tmpdir(), "devdesk-standard-overrides-")));
    await page.getByRole("button", { name: "初始化 / 打开数据集" }).click();
  }
  const project = await rpc(page, "project.create", {
    command_id: crypto.randomUUID(),
    payload: {
      title: "项目规范覆盖",
      repository: "fixture/standard-overrides",
    },
  });
  const defaults = await rpc(page, "query.configuration_defaults", {
    project_id: project.id,
  });
  const catalog = await rpc(page, "query.configurations", {
    kind: "spec",
    include_history: true,
    limit: 100,
    project_id: project.id,
  });
  const spec = catalog.records.find(
    (r: any) => r.id === "builtin-spec-stage-prd:2",
  );
  const generation = await rpc(page, "query.template_catalog");
  const replacement = await rpc(page, "spec.revise", {
    command_id: crypto.randomUUID(),
    target: spec.id,
    expected_revision: spec.revision,
    expected_catalog_generation: generation.generation,
    payload: {
      name: "项目 PRD 定制规范",
      data: { ...spec.data, body: spec.data.body + "\n确认本项目的新增要求。" },
    },
  });
  await page.reload();
  await page.getByRole("button", { name: "项目管理", exact: true }).click();
  const panel = page.getByRole("region", { name: "项目配置" });
  await expect(panel.getByLabel("项目 Workflow", { exact: true })).toHaveCount(
    0,
  );
  await expect(panel.getByLabel("基础 Standard", { exact: true })).toHaveValue(
    defaults.standard_ref,
  );
  await expect(panel.getByLabel("默认启动入口")).toContainText(
    "默认 Feature 开发流程",
  );
  const save = panel.getByRole("button", { name: "保存项目配置", exact: true });
  await expect(save).toBeEnabled();
  await save.click();
  await expect(panel.getByText(/^当前 Standard：/)).toBeVisible();
  const initial = await rpc(page, "query.project_configuration", {
    project_id: project.id,
  });
  expect(initial.configuration.data.standard_ref).toBe(defaults.standard_ref);

  const orchestration = panel.locator(".project-orchestration-fields select");
  const orchestrationBoxes = await orchestration.evaluateAll((nodes) =>
    nodes.map((node) => {
      const { x, y, width } = node.getBoundingClientRect();
      return { x, y, width };
    }),
  );
  expect(orchestrationBoxes).toHaveLength(6);
  expect(new Set(orchestrationBoxes.slice(0, 4).map((box) => box.y)).size).toBe(
    1,
  );
  expect(new Set(orchestrationBoxes.slice(4).map((box) => box.y)).size).toBe(1);
  expect(orchestrationBoxes[4].y).toBeGreaterThan(orchestrationBoxes[0].y);
  const roles = panel.getByRole("group", { name: "流程所需 Role → Agent" });
  const roleBoxes = await roles.locator("select").evaluateAll((nodes) =>
    nodes.map((node) => {
      const { x, y } = node.getBoundingClientRect();
      return { x, y };
    }),
  );
  expect(roleBoxes.length).toBeGreaterThan(1);
  expect(roleBoxes[0].y).toBe(roleBoxes[1].y);
  expect(roleBoxes[1].x).toBeGreaterThan(roleBoxes[0].x);
  const agentWidths = await roles.locator("select").evaluateAll((nodes) =>
    nodes.map((node) => {
      const style = getComputedStyle(node);
      const context = document.createElement("canvas").getContext("2d")!;
      context.font = style.font;
      const longest = Math.max(
        ...Array.from(
          node.querySelectorAll("option"),
          (option) => context.measureText(option.text).width,
        ),
      );
      return { width: node.getBoundingClientRect().width, longest };
    }),
  );
  for (const agent of agentWidths) {
    expect(agent.width).toBeGreaterThan(agent.longest + 40);
    expect(agent.width).toBeLessThan(agent.longest + 80);
  }
  await panel.getByText("项目覆盖（0 项）", { exact: true }).click();
  const kind = panel.getByLabel("规范类型", { exact: true });
  const source = panel.getByLabel("要覆盖的规范项", { exact: true });
  const target = panel.getByLabel("替换为", { exact: true });
  await expect(source).toBeDisabled();
  await expect(source.locator("option")).toHaveCount(1);
  await expect(target.locator("option")).toHaveCount(1);
  const baseline = await rpc(page, "query.standard", {
    id: defaults.standard_ref,
    project_id: project.id,
    limit: 1000,
  });
  for (const type of ["workflow", "delivery_type", "spec"]) {
    await kind.selectOption(type);
    const available = await rpc(page, "query.configurations", {
      kind: type,
      local: true,
      include_history: true,
      project_id: project.id,
    });
    await expect(source.locator("option")).toHaveCount(
      baseline.nodes.filter((node: any) => node.kind === type).length + 1,
    );
    await expect(target.locator("option")).toHaveCount(available.total + 1);
  }
  const overrideBoxes = await panel
    .locator("details .project-configuration-row select")
    .evaluateAll((nodes) =>
      nodes.map((node) => node.getBoundingClientRect().y),
    );
  expect(overrideBoxes).toHaveLength(3);
  expect(new Set(overrideBoxes).size).toBe(1);
  await panel
    .getByLabel("要覆盖的规范项", { exact: true })
    .selectOption(spec.id);
  await panel
    .getByLabel("替换为", { exact: true })
    .selectOption(replacement.id);
  await panel.getByRole("button", { name: "添加覆盖", exact: true }).click();
  await expect(save).toBeEnabled();
  await panel.getByText("保存前查看来源与差异", { exact: true }).click();
  await expect(panel.locator(".standard-diff")).toContainText("规范变化");
  await save.click();
  await expect(
    page.getByText(
      "项目专属规范快照已保存；已有 Feature 的冻结版本保持不变。",
      { exact: true },
    ),
  ).toBeVisible();
  const selected = await rpc(page, "query.project_configuration", {
    project_id: project.id,
  });
  const effective = await rpc(page, "query.standard", {
    id: selected.configuration.data.standard_ref,
    project_id: project.id,
    limit: 1000,
  });
  expect(effective.record.scope).toBe(`project:${project.id}`);
  expect(effective.provenance.base_standard_ref).toBe(defaults.standard_ref);
  expect(effective.provenance.overrides).toEqual([
    { source_ref: spec.id, replacement_ref: replacement.id },
  ]);
  expect(effective.provenance.diff.equal).toBe(false);
  expect(effective.nodes.some((n: any) => n.reference === replacement.id)).toBe(
    true,
  );
  expect(effective.nodes.some((n: any) => n.reference === spec.id)).toBe(false);
  await page.reload();
  await expect(panel.getByLabel("基础 Standard", { exact: true })).toHaveValue(
    defaults.standard_ref,
  );
  await panel.getByText("项目覆盖（1 项）", { exact: true }).click();
  await expect(
    panel.getByText(`${spec.id} → ${replacement.id}`, { exact: true }),
  ).toBeVisible();
  await expect(save).toBeEnabled();
  await expect(panel.getByText(/^当前项目快照：/)).toBeVisible();
  await expect(panel.getByLabel("默认启动入口")).toContainText(
    "默认 Feature 开发流程",
  );
  await page.screenshot({
    path: info.outputPath("project-standard-overrides.png"),
    fullPage: true,
  });
  await panel
    .getByRole("button", { name: `移除覆盖 ${spec.id}`, exact: true })
    .click();
  await expect(save).toBeEnabled();
  await save.click();
  await expect(
    page.getByText(
      "项目配置已保存，直接使用所选 Standard；已有 Feature 的冻结版本保持不变。",
      { exact: true },
    ),
  ).toBeVisible();
  const reverted = await rpc(page, "query.project_configuration", {
    project_id: project.id,
  });
  const restored = await rpc(page, "query.standard", {
    id: reverted.configuration.data.standard_ref,
    project_id: project.id,
  });
  expect(reverted.configuration.data.standard_ref).toBe(defaults.standard_ref);
  expect(restored.provenance).toBeNull();
  await expect(panel.getByText(/^当前 Standard：/)).toBeVisible();
  await kind.selectOption("workflow");
  const workflow = baseline.nodes.find((node: any) => node.kind === "workflow");
  await source.selectOption(workflow.reference);
  await kind.selectOption("");
  await expect(source).toHaveValue("");
  await expect(source).toBeDisabled();
  await expect(source.locator("option")).toHaveCount(1);
  await expect(target).toHaveValue("");
  await expect(target.locator("option")).toHaveCount(1);

  await page.setViewportSize({ width: 600, height: 900 });
  const narrowRoles = await roles.locator("select").evaluateAll((nodes) =>
    nodes.map((node) => {
      const { x, y, right } = node.getBoundingClientRect();
      return { x, y, right };
    }),
  );
  expect(new Set(narrowRoles.map((box) => box.y)).size).toBeGreaterThan(1);
  expect(narrowRoles.every((box) => box.x >= 0 && box.right <= 600)).toBe(true);
  await page.screenshot({
    path: info.outputPath("project-standard-overrides-mobile.png"),
    fullPage: true,
  });
});

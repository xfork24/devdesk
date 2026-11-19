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

test("项目中替换共享 Standard 的 Spec 后生成命名项目快照，采用时保留新快照身份", async ({
  page,
}) => {
  await page.goto("/");
  const project = await rpc(page, "project.create", {
    command_id: crypto.randomUUID(),
    payload: {
      title: "Standard 替换",
      repository: "fixture/standard-overrides",
    },
  });
  const defaults = await rpc(page, "query.configuration_defaults", {
    project_id: project.id,
  });
  const original = await rpc(page, "query.standard", {
    id: defaults.standard_ref,
    limit: 1000,
  });
  const spec = original.nodes.find((node: any) => node.kind === "spec");
  const source = (
    await rpc(page, "query.configuration_records", { refs: [spec.reference] })
  ).records[0];
  const catalog = await rpc(page, "query.template_catalog");
  const replacement = await rpc(page, "spec.revise", {
    command_id: crypto.randomUUID(),
    target: source.id,
    expected_revision: source.revision,
    expected_catalog_generation: catalog.generation,
    payload: {
      name: "新的验收要求",
      data: { ...source.data, body: source.data.body + "\n项目新增验收要求。" },
    },
  });
  await page.goto(
    `/?tab=workflows&config_tab=standard&project_id=${project.id}&standard_id=${encodeURIComponent(original.record.id)}`,
  );
  const panel = page.getByRole("region", {
    name: "Standard 规范快照",
    exact: true,
  });
  const item = panel.getByRole("region", {
    name: `${spec.name} · ${spec.reference}`,
    exact: true,
  });
  await expect(item.getByLabel("替换为", { exact: true })).not.toBeVisible();
  await item.locator(":scope > details > summary").click();
  const sizing = await item.getByRole("combobox").evaluate((select) => {
    const style = getComputedStyle(select);
    const context = document.createElement("canvas").getContext("2d")!;
    context.font = style.font;
    const longest = Math.max(
      ...Array.from(
        (select as HTMLSelectElement).options,
        (option) => context.measureText(option.text).width,
      ),
    );
    return {
      width: select.getBoundingClientRect().width,
      content:
        longest +
        parseFloat(style.paddingLeft) +
        parseFloat(style.paddingRight),
    };
  });
  expect(sizing.width).toBeGreaterThanOrEqual(sizing.content - 2);
  expect(sizing.width).toBeLessThan(sizing.content + 32);
  await item.getByRole("combobox").selectOption(replacement.id);
  await panel.getByLabel("快照名称", { exact: true }).fill("项目验收规范");
  await panel.getByRole("button", { name: "预览替换", exact: true }).click();
  const save = panel.getByRole("button", { name: "生成新版本", exact: true });
  await expect(save).toBeEnabled();
  await save.click();
  const selected = panel.getByLabel("查看 Standard", { exact: true });
  await expect(selected).not.toHaveValue(original.record.id);
  const id = await selected.inputValue();
  const next = await rpc(page, "query.standard", {
    project_id: project.id,
    id,
    limit: 1000,
  });
  expect(next.record.name).toBe("项目验收规范");
  expect(next.record.scope).toBe(`project:${project.id}`);
  expect(next.record.version).toBe(1);
  expect(
    next.nodes.some((node: any) => node.reference === spec.reference),
  ).toBe(false);
  expect(
    next.nodes.some((node: any) => node.reference === replacement.id),
  ).toBe(true);
  expect(next.provenance.overrides).toEqual([
    { source_ref: spec.reference, replacement_ref: replacement.id },
  ]);
  expect(
    (await rpc(page, "query.standard", { id: original.record.id }))
      .snapshot_digest,
  ).toBe(original.snapshot_digest);
  const currentProject = (
    await rpc(page, "query.project_directory")
  ).projects.find((row: any) => row.id === project.id);
  await rpc(page, "project.configuration.update", {
    project_id: project.id,
    target: project.id,
    expected_revision: currentProject.revision,
    command_id: crypto.randomUUID(),
    expected_catalog_generation: catalog.generation,
    payload: {
      standard_ref: id,
      role_agent_bindings: defaults.role_agent_bindings,
      harness_permission_ref: defaults.harness_permission_ref,
    },
  });
  const selection = await rpc(page, "query.project_configuration", {
    project_id: project.id,
    view: "selection",
  });
  expect(selection.configuration.data.standard_ref).toBe(id);
  expect(selection.standard.provenance).toBeNull();
  await page.goto(`/?tab=projects&project_id=${project.id}`);
  await expect(
    page
      .getByRole("region", { name: "项目配置" })
      .getByLabel("基础 Standard", { exact: true }),
  ).toHaveValue(id);
});

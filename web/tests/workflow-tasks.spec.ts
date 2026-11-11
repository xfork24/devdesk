import { test, expect, Page } from "@playwright/test";
import { mkdtempSync } from "node:fs";
import { join } from "node:path";
import { tmpdir } from "node:os";

async function rpc(page: Page, method: string, params: any = {}) {
  const result = await page.request
    .post("/rpc", { data: { jsonrpc: "2.0", id: 1, method, params } })
    .then((r) => r.json());
  expect(result.error, JSON.stringify(result)).toBeUndefined();
  return result.result;
}
async function setup(page: Page) {
  await page.goto("/");
  if (
    (await page.request.get("/capabilities").then((r) => r.json()))
      .setup_required
  ) {
    await page
      .getByLabel("数据目录")
      .fill(mkdtempSync(join(tmpdir(), "devdesk-task-web-")));
    await page.getByRole("button", { name: "初始化 / 打开数据集" }).click();
  }
}
test("Agent Step 的任务原文在 UI 编辑保存发布，并与 Role Skill Spec 一起预览", async ({
  page,
}, info) => {
  const errors: string[] = [];
  page.on("pageerror", (e) => errors.push(e.message));
  await setup(page);
  const name = `任务指令 ${crypto.randomUUID().slice(0, 6)}`;
  const original = "## 原任务\n\n保留原不可变版本。";
  const body =
    "## 可编辑任务正文\n\n新的目标, 失败路径与恢复。\n\n```js\nconsole.log('{{literal}}')\n```\n🙂";
  const draft = await rpc(page, "workflow.create", {
    command_id: crypto.randomUUID(),
    payload: {
      name,
      document: {
        schema_version: 5,
        stages: [{ key: "work", name: "任务阶段" }],
        steps: [
          {
            key: "discuss",
            name: "执行可编辑任务",
            stage: "work",
            executor: { type: "agent", role_ref: "builtin-role-analyst" },
            required_skill_refs: ["builtin-skill-prd-v2"],
            spec_refs: ["builtin-spec-stage-prd-v1"],
            task_instructions: original,
          },
        ],
      },
    },
  });
  const published = await rpc(page, "workflow.publish", {
    command_id: crypto.randomUUID(),
    target: draft.id,
    expected_revision: draft.revision,
    payload: {},
  });
  await page.goto(`/?tab=workflows&workflow_id=${published.id}`);
  const editor = page.getByRole("region", { name: "工作流程管理" });
  await editor.getByRole("button", { name: /执行可编辑任务/ }).click();
  await expect(
    editor.getByRole("heading", { name: "原任务", exact: true }),
  ).toBeVisible();
  await editor.getByRole("button", { name: "编辑流程", exact: true }).click();
  await expect(
    editor.getByRole("button", { name: "保存草稿", exact: true }),
  ).toBeEnabled();
  await editor.getByRole("button", { name: /执行可编辑任务/ }).click();
  await editor.getByRole("button", { name: "任务指令", exact: true }).click();
  await editor
    .getByRole("textbox", { name: "任务指令", exact: true })
    .fill(body);
  await editor.getByRole("button", { name: "保存草稿", exact: true }).click();
  await expect(
    page.getByRole("status").filter({ hasText: "流程草稿已保存。" }),
  ).toHaveText(/流程草稿已保存/);
  const newId = new URL(page.url()).searchParams.get("workflow_id");
  expect(
    (await rpc(page, "query.workflow", { id: newId })).data.document.steps[0]
      .task_instructions,
  ).toBe(body);
  expect(
    (await rpc(page, "query.workflow", { id: published.id })).data.steps[0]
      .task_instructions,
  ).toBe(original);
  await page.reload();
  await expect(
    editor.getByRole("heading", { name: "可编辑任务正文", exact: true }),
  ).toBeVisible();
  await editor.getByText("预览任务与执行上下文", { exact: true }).click();
  await editor
    .getByLabel("预览 Agent（可选）", { exact: true })
    .selectOption("builtin-agent-sol-high-analyst-v3");
  await editor
    .getByRole("button", { name: "预览当前任务", exact: true })
    .click();
  const preview = editor.getByRole("region", { name: "Agent 任务与上下文" });
  await expect(
    preview.getByRole("heading", { name: "可编辑任务正文", exact: true }),
  ).toBeVisible();
  await expect(preview.getByRole("group", { name: /Role 职责/ })).toBeVisible();
  await expect(preview.getByRole("group", { name: /Skill ·/ })).toHaveCount(1);
  await expect(preview.getByRole("group", { name: /Spec ·/ })).toHaveCount(1);
  await editor.getByRole("button", { name: "发布流程", exact: true }).click();
  await expect(
    page.getByRole("status").filter({ hasText: "流程已发布。" }),
  ).toHaveText(/流程已发布/);
  const newPublished = new URL(page.url()).searchParams.get("workflow_id");
  expect(
    (await rpc(page, "query.workflow", { id: newPublished })).data.steps[0]
      .task_instructions,
  ).toBe(body);
  await page.screenshot({
    path: info.outputPath("agent-task-preview.png"),
    fullPage: true,
  });
  expect(errors).toEqual([]);
});

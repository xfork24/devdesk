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
  if (await page.getByLabel("数据目录").count()) {
    await page
      .getByLabel("数据目录")
      .fill(mkdtempSync(join(tmpdir(), "devdesk-task-web-")));
    await page.getByRole("button", { name: "初始化 / 打开数据集" }).click();
  }
}
test("Agent Step 的任务原文在 UI 编辑保存发布，并与 Role Skill Spec 一起预览", async ({
  page,
}) => {
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
        schema_version: 3,
        process_kind: "subworkflow",
        stages: [{ key: "work", name: "任务阶段" }],
        steps: [
          {
            key: "discuss",
            name: "执行可编辑任务",
            stage: "work",
            kind: "agent.interact",
            contract_version: 2,
            role_ref: "builtin-role-analyst",
            spec_refs: ["builtin-spec-stage-prd-v1"],
            task_instructions: original,
            interaction: {
              required_topics: ["具体结果"],
              output_kind: "requirement",
              max_turns: 8,
            },
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
  const editor = page.getByRole("region", { name: "Workflow 编辑器" });
  await editor.getByRole("button", { name: /执行可编辑任务/ }).click();
  await expect(
    editor.getByRole("heading", { name: "原任务", exact: true }),
  ).toBeVisible();
  await editor.getByRole("button", { name: "编辑流程", exact: true }).click();
  await editor
    .getByRole("button", { name: "编辑任务指令（Markdown）源码", exact: true })
    .click();
  await editor
    .getByRole("textbox", { name: "任务指令（Markdown）", exact: true })
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
    .getByLabel("预览 Agent", { exact: true })
    .selectOption("builtin-agent-sol-high-analyst-v3");
  await editor
    .getByRole("button", { name: "生成任务预览", exact: true })
    .click();
  const preview = editor.getByRole("region", { name: "Agent 任务与上下文" });
  await expect(
    preview.getByRole("heading", { name: "可编辑任务正文", exact: true }),
  ).toBeVisible();
  await expect(
    preview.getByRole("group", { name: /Role 与 Agent 有效指令/ }),
  ).toBeVisible();
  await expect(preview.getByRole("group", { name: /Skill ·/ })).toHaveCount(2);
  await expect(preview.getByRole("group", { name: /Spec ·/ })).toHaveCount(1);
  await editor
    .getByRole("button", { name: "保存可用版本", exact: true })
    .click();
  await expect(
    page.getByRole("status").filter({ hasText: "流程可用版本已保存。" }),
  ).toHaveText(/流程可用版本已保存/);
  const newPublished = new URL(page.url()).searchParams.get("workflow_id");
  expect(
    (await rpc(page, "query.workflow", { id: newPublished })).data.steps[0]
      .task_instructions,
  ).toBe(body);
  expect(errors).toEqual([]);
});

test("正式执行分别编辑公共和 Agent 节点任务，系统测试节点不显示任务编辑器", async ({
  page,
}) => {
  await setup(page);
  await page.goto(
    "/?tab=workflows&workflow_id=builtin-workflow-task-sol-high-v3",
  );
  const editor = page.getByRole("region", { name: "Workflow 编辑器" });
  await editor.getByRole("button", { name: /TDD 与实现/ }).click();
  await editor.getByRole("button", { name: "编辑流程", exact: true }).click();
  await editor
    .getByRole("button", {
      name: "编辑公共任务指令（Markdown）源码",
      exact: true,
    })
    .click();
  await editor
    .getByRole("textbox", { name: "公共任务指令（Markdown）", exact: true })
    .fill("## 公共目标\n\n仅实现当前 Task。");
  await editor
    .getByRole("button", {
      name: "编辑实现 节点任务指令（Markdown）源码",
      exact: true,
    })
    .click();
  await editor
    .getByRole("textbox", {
      name: "实现 节点任务指令（Markdown）",
      exact: true,
    })
    .fill("## 具体实现\n\n按已采纳接口完成当前 Task, 保持测试不变。");
  await expect(
    editor.getByRole("group", { name: /节点任务指令（Markdown）/ }),
  ).toHaveCount(3);
  await expect(
    editor.getByText("系统测试执行器按冻结 TestRecipe 执行，不调用 Agent。", {
      exact: true,
    }),
  ).toHaveCount(3);
  await editor.getByRole("button", { name: "保存草稿", exact: true }).click();
  await expect(
    page.getByRole("status").filter({ hasText: "流程草稿已保存。" }),
  ).toHaveText(/流程草稿已保存/);
  const id = new URL(page.url()).searchParams.get("workflow_id");
  const doc = (await rpc(page, "query.workflow", { id })).data.document;
  const step = doc.steps.find((s: any) => s.kind === "attempt.execute");
  expect(step.task_instructions).toBe("## 公共目标\n\n仅实现当前 Task。");
  expect(
    step.execution_recipe.find((n: any) => n.key === "implement")
      .task_instructions,
  ).toContain("## 具体实现");
  expect(
    step.execution_recipe
      .filter((n: any) => ["red", "green", "verify"].includes(n.key))
      .every((n: any) => n.task_instructions === undefined),
  ).toBe(true);
});

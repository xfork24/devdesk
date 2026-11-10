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
for (const allowOverride of [false, true])
  test(`未满足的完成条件：配置${allowOverride ? "允许说明理由后覆盖" : "禁止人工覆盖"}`, async ({
    page,
  }) => {
    await page.goto("/");
    if (
      (await page.request.get("/capabilities").then((r) => r.json()))
        .setup_required
    ) {
      await page
        .getByLabel("数据目录")
        .fill(mkdtempSync(join(tmpdir(), "devdesk-confirm-")));
      await page.getByRole("button", { name: "初始化 / 打开数据集" }).click();
    }
    const draft = await rpc(page, "workflow.create", {
      command_id: crypto.randomUUID(),
      payload: {
        name: "确切版本确认",
        document: {
          schema_version: 4,
          stages: [{ key: "decision", name: "决定" }],
          steps: [
            {
              key: "check",
              name: "核对未满足条件",
              stage: "decision",
              executor: { type: "control" },
              confirmation: "human",
              allow_completion_override: allowOverride,
              completion: {
                op: "equals",
                source: { step: "$input", port: "accepted" },
                value: true,
              },
            },
          ],
          inputs: { accepted: { delivery_ref: "builtin-delivery-object-v1" } },
        },
      },
    });
    const workflow = await rpc(page, "workflow.publish", {
      command_id: crypto.randomUUID(),
      target: draft.id,
      expected_revision: draft.revision,
      payload: {},
    });
    const project = await rpc(page, "project.create", {
      command_id: crypto.randomUUID(),
      payload: { title: "确认条件项目" },
    });
    const instance = await rpc(page, "workflow.start", {
      project_id: project.id,
      command_id: crypto.randomUUID(),
      target: project.id,
      expected_revision: (
        await rpc(page, "query.workspace", { project_id: project.id })
      ).active_project.revision,
      payload: {
        workflow_ref: workflow.id,
        role_agent_bindings: {},
        inputs: { accepted: { value: false } },
      },
    });
    await page.goto(`/?tab=execution&project_id=${project.id}`);
    const runtime = page.getByRole("region", {
      name: "当前工作流",
      exact: true,
    });
    await expect(
      runtime.getByRole("button", { name: "确认此版本结果", exact: true }),
    ).toBeVisible();
    if (allowOverride) {
      await runtime.getByLabel("明确覆盖未满足的完成条件").check();
      await expect(
        runtime.getByRole("button", { name: "确认此版本结果", exact: true }),
      ).toBeDisabled();
      await runtime
        .getByLabel("确认说明")
        .fill("已核实当前材料，记录人工例外。");
      await runtime
        .getByRole("button", { name: "确认此版本结果", exact: true })
        .click();
      await expect
        .poll(
          async () =>
            (
              await rpc(page, "query.workspace", { project_id: project.id })
            ).configurations.find((r: any) => r.id === instance.id)?.data
              .status,
        )
        .toBe("completed");
    } else {
      await expect(runtime.getByLabel("明确覆盖未满足的完成条件")).toHaveCount(
        0,
      );
      await runtime
        .getByRole("button", { name: "确认此版本结果", exact: true })
        .click();
      await expect(runtime.getByRole("alert")).toContainText("不允许直接通过");
      const row = (
        await rpc(page, "query.workspace", { project_id: project.id })
      ).configurations.find((r: any) => r.id === instance.id);
      expect(row.data.steps[0].status).toBe("waiting_confirmation");
    }
  });

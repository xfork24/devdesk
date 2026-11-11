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
  test(`校验未通过时可填写理由手动 Approve：${allowOverride ? "人工确认步骤" : "自动确认步骤"}`, async ({
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
          schema_version: 5,
          stages: [{ key: "decision", name: "决定" }],
          steps: [
            {
              key: "check",
              name: "核对未满足条件",
              stage: "decision",
              executor: { type: "control" },
              confirmation: allowOverride ? "human" : "automatic",
              allow_completion_override: allowOverride,
              completion: {
                op: "equals",
                source: { step: "$input", port: "accepted" },
                value: true,
              },
            },
          ],
          inputs: { accepted: { delivery_ref: "builtin-delivery-object:1" } },
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
    await runtime
      .getByRole("button", { name: "执行步骤", exact: true })
      .click();
    const approve = runtime.getByRole("button", {
      name: "手动 Approve 此版本",
      exact: true,
    });
    await expect(
      runtime.getByText(/校验未通过，完成条件尚未满足/),
    ).toBeVisible();
    await expect(
      runtime.getByRole("button", { name: "从此步骤返工", exact: true }),
    ).toBeVisible();
    await expect(approve).toBeVisible();
    await expect(approve).toBeDisabled();
    await runtime.getByLabel("手动批准理由（必填）").fill("  ");
    await expect(approve).toBeDisabled();
    const reason = "已核实当前材料，接受本次校验缺口。";
    await runtime.getByLabel("手动批准理由（必填）").fill(reason);
    await expect(approve).toBeEnabled();
    await approve.click();
    await runtime
      .getByRole("button", { name: "接纳结果", exact: true })
      .click();
    await runtime
      .getByRole("button", { name: "完成流程", exact: true })
      .click();
    await expect
      .poll(
        async () =>
          (
            await rpc(page, "query.workflow_instance", {
              project_id: project.id,
              id: instance.id,
            })
          ).runtime.status,
      )
      .toBe("completed");
    await expect(
      runtime.getByText(`已手动批准，原完成条件未满足：${reason}`, {
        exact: true,
      }),
    ).toBeVisible();
    const view = await rpc(page, "query.workflow_instance", {
      project_id: project.id,
      id: instance.id,
    });
    const confirmation = view.completion_status[view.runtime.steps[0].id];
    expect(confirmation.satisfied).toBe(false);
    expect(confirmation.confirmation.reason).toBe(reason);
    expect(confirmation.confirmation.manual_approval).toBe(true);
    expect(view.plan.steps[0].allow_completion_override).toBe(allowOverride);
    await page.reload();
    await expect(
      runtime.getByText(`已手动批准，原完成条件未满足：${reason}`, {
        exact: true,
      }),
    ).toBeVisible();
  });

test("并行等待审批的步骤分别保存理由", async ({ page }) => {
  await page.goto("/");
  if (
    (await page.request.get("/capabilities").then((r) => r.json()))
      .setup_required
  ) {
    await page
      .getByLabel("数据目录")
      .fill(mkdtempSync(join(tmpdir(), "devdesk-multi-confirm-")));
    await page.getByRole("button", { name: "初始化 / 打开数据集" }).click();
  }
  const draft = await rpc(page, "workflow.create", {
    command_id: crypto.randomUUID(),
    payload: {
      name: "并行审批",
      document: {
        schema_version: 5,
        stages: [{ key: "decision", name: "决定" }],
        inputs: { accepted: { delivery_ref: "builtin-delivery-object:1" } },
        steps: ["first", "second"].map((key) => ({
          key,
          name: key,
          stage: "decision",
          executor: { type: "control" },
          completion: {
            op: "equals",
            source: { step: "$input", port: "accepted" },
            value: true,
          },
        })),
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
    payload: { title: "并行审批项目" },
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
  const runtime = page.getByRole("region", { name: "当前工作流", exact: true });
  await runtime
    .getByRole("button", { name: "执行步骤", exact: true })
    .first()
    .click();
  await runtime
    .getByRole("article", { name: "步骤 · second", exact: true })
    .getByRole("button", { name: "执行步骤", exact: true })
    .click();
  const first = page.getByRole("article", {
    name: "步骤 · first",
    exact: true,
  });
  const second = page.getByRole("article", {
    name: "步骤 · second",
    exact: true,
  });
  await first.getByLabel("手动批准理由（必填）").fill("第一步的人工决定");
  await second.getByLabel("手动批准理由（必填）").focus();
  await expect(second.getByLabel("手动批准理由（必填）")).toHaveValue("");
  await expect(
    second.getByRole("button", { name: "手动 Approve 此版本" }),
  ).toBeDisabled();
  await second.getByLabel("手动批准理由（必填）").fill("第二步的人工决定");
  await first.getByRole("button", { name: "手动 Approve 此版本" }).click();
  await expect(
    first.getByText("已手动批准，原完成条件未满足：第一步的人工决定", {
      exact: true,
    }),
  ).toBeVisible();
  await expect(second.getByLabel("手动批准理由（必填）")).toHaveValue(
    "第二步的人工决定",
  );
  await second.getByRole("button", { name: "手动 Approve 此版本" }).click();
  await first.getByRole("button", { name: "接纳结果", exact: true }).click();
  await second.getByRole("button", { name: "接纳结果", exact: true }).click();
  await runtime.getByRole("button", { name: "完成流程", exact: true }).click();
  await expect
    .poll(
      async () =>
        (
          await rpc(page, "query.workflow_instance", {
            project_id: project.id,
            id: instance.id,
          })
        ).runtime.status,
    )
    .toBe("completed");
});

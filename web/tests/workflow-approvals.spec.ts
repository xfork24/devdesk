import { expect, type Page } from "@playwright/test";
import { test } from "./support/kernel";

async function rpc(page: Page, method: string, params: object = {}) {
  const response = await page.request.post("/rpc", {
    data: { jsonrpc: "2.0", id: 1, method, params },
  });
  const result = await response.json();
  expect(result.error, JSON.stringify(result)).toBeUndefined();
  return result.result;
}

for (const nested of [false, true]) {
  test(
    nested
      ? "nested human gates are confirmed inside their manager subflow card and resume the full tree"
      : "manager and workflow approvals both persist human decisions and continue the real driver",
    async ({ page }) => {
      await page.goto("/");
      const project = await rpc(page, "project.create", {
        command_id: crypto.randomUUID(),
        payload: { title: "审批链路验收" },
      });
      const draft = await rpc(page, "workflow.create", {
        command_id: crypto.randomUUID(),
        payload: {
          name: "两个人工确认点",
          document: {
            schema_version: 6,
            stages: [{ key: "work", name: "确认阶段" }],
            steps: [
              {
                key: "one",
                name: "需求",
                stage: "work",
                executor: { type: "control" },
                confirmation: "human",
              },
              {
                key: "two",
                name: "设计",
                stage: "work",
                dependencies: ["one"],
                executor: { type: "control" },
                confirmation: "human",
              },
            ],
          },
        },
      });
      const workflow = await rpc(page, "workflow.publish", {
        command_id: crypto.randomUUID(),
        target: draft.id,
        expected_revision: draft.revision,
        payload: {},
      });
      let rootWorkflow = workflow;
      if (nested) {
        const wrapper = await rpc(page, "workflow.create", {
          command_id: crypto.randomUUID(),
          payload: {
            name: "默认 Feature 开发流程",
            document: {
              schema_version: 6,
              stages: [{ key: "work", name: "父阶段" }],
              steps: [
                {
                  key: "task",
                  name: "执行各 Task",
                  stage: "work",
                  executor: { type: "workflow", workflow_ref: workflow.id },
                },
              ],
            },
          },
        });
        rootWorkflow = await rpc(page, "workflow.publish", {
          command_id: crypto.randomUUID(),
          target: wrapper.id,
          expected_revision: wrapper.revision,
          payload: {},
        });
      }
      const root = await rpc(page, "workflow.start", {
        project_id: project.id,
        command_id: crypto.randomUUID(),
        target: project.id,
        expected_revision: (
          await rpc(page, "query.workspace", { project_id: project.id })
        ).active_project.revision,
        payload: {
          workflow_ref: rootWorkflow.id,
          role_agent_bindings: {},
          orchestration: { mode: "rust" },
        },
      });
      let executionInstanceId = root.id;
      if (nested) {
        await expect
          .poll(
            async () =>
              (
                await rpc(page, "query.workflow_instance", {
                  project_id: project.id,
                  id: root.id,
                })
              ).children.length,
          )
          .toBe(1);
        executionInstanceId = (
          await rpc(page, "query.workflow_instance", {
            project_id: project.id,
            id: root.id,
          })
        ).children[0].id;
      }
      const inspect = () =>
        rpc(page, "query.workflow_instance", {
          project_id: project.id,
          id: executionInstanceId,
        });
      await expect
        .poll(async () => (await inspect()).runtime.steps[0].status)
        .toBe("waiting_confirmation");
      const before = await inspect();
      const step = before.runtime.steps[0];
      const pending = await rpc(page, "workflow.decision.request", {
        project_id: project.id,
        command_id: crypto.randomUUID(),
        target: executionInstanceId,
        expected_revision: before.record.revision,
        payload: {
          plan_revision_id: before.runtime.active_plan_revision_id,
          control_version: before.runtime.control_version,
          operation: "workflow.step.accept",
          reason: "请人类核对需求",
          arguments: {
            instance_id: executionInstanceId,
            plan_revision_id: before.runtime.active_plan_revision_id,
            step: step.step,
            execution_id: step.id,
            input_digest: step.input_digest,
            outputs: step.outputs,
          },
        },
      });
      await page.goto(
        `/?tab=execution&detail=manager&project_id=${project.id}&instance_id=${root.id}`,
      );
      const card = page.getByRole("article", { name: "确认需求", exact: true });
      await expect(card).toBeVisible();
      const featureFrame = page.locator(".manager-workflow-summary");
      await expect(
        featureFrame.getByRole("region", { name: "项目经理协作", exact: true }),
      ).toBeVisible();
      await expect(
        featureFrame.locator(".manager-collaboration.panel"),
      ).toHaveCount(0);
      if (!nested)
        await expect(
          featureFrame.getByRole("article", { name: "确认需求", exact: true }),
        ).toBeVisible();
      await page.screenshot({
        path: test
          .info()
          .outputPath(
            nested ? "nested-manager-pending.png" : "manager-pending.png",
          ),
        fullPage: true,
      });
      if (nested) {
        await expect(
          featureFrame
            .getByRole("region", { name: "项目经理协作", exact: true })
            .getByRole("article", { name: "确认需求", exact: true }),
        ).toHaveCount(1);
        await expect(
          featureFrame.getByRole("heading", {
            name: "主工作流 · Feature开发",
            exact: true,
          }),
        ).toBeVisible();
        await expect(
          page
            .locator(".manager-child-flow")
            .getByRole("heading", { name: /^子流程 · Task开发/ }),
        ).toBeVisible();
        await expect(page.locator(".manager-child-flow > summary")).toHaveCount(
          0,
        );
      }
      await expect(
        page.getByLabel("回复项目经理", { exact: true }),
      ).toHaveCount(0);
      await card
        .getByRole("button", { name: "确认：确认需求", exact: true })
        .click();
      await expect
        .poll(async () => (await inspect()).runtime.steps[0].status)
        .toBe("completed");
      await expect
        .poll(async () => (await inspect()).runtime.steps[1].status)
        .toBe("waiting_confirmation");
      const inbox = await rpc(page, "query.orchestration", {
        project_id: project.id,
        root_instance_id: root.id,
      });
      expect(
        inbox.decisions.find((d: { id: string }) => d.id === pending.id).data
          .status,
      ).toBe("approved");
      if (nested) {
        const designCard = featureFrame.getByRole("article", {
          name: "确认设计",
          exact: true,
        });
        await designCard
          .getByRole("button", { name: "确认：确认设计", exact: true })
          .click();
      } else {
        await page.goto(
          `/?tab=execution&detail=overview&project_id=${project.id}&instance_id=${root.id}`,
        );
        const design = page.getByRole("article", {
          name: "步骤 · 设计",
          exact: true,
        });
        await design.locator("button.workflow-step-toggle").click();
        await design
          .getByRole("button", { name: "确认此版本结果", exact: true })
          .click();
      }
      await expect
        .poll(async () => (await inspect()).runtime.status)
        .toBe("completed");
      const final = await inspect();
      expect(
        final.runtime.steps.every(
          (s: { status: string }) => s.status === "completed",
        ),
      ).toBe(true);
      await expect
        .poll(
          async () =>
            (
              await rpc(page, "query.workflow_instance", {
                project_id: project.id,
                id: root.id,
              })
            ).runtime.status,
        )
        .toBe("completed");
    },
  );
}

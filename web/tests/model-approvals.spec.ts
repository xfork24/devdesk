import { test, expect } from "@playwright/test";
import { mkdtempSync } from "node:fs";
import { join } from "node:path";
import { tmpdir } from "node:os";

for (const modelApprove of [true, false]) {
  test(`内容校验显示模型结论，人类${modelApprove ? "Approve" : "覆盖"}后继续`, async ({
    page,
  }) => {
    const errors: string[] = [];
    page.on("pageerror", (error) => errors.push(error.message));
    await page.goto("/");
    if (
      (await page.request.get("/capabilities").then((r) => r.json()))
        .setup_required
    ) {
      await page
        .getByLabel("数据目录")
        .fill(mkdtempSync(join(tmpdir(), "devdesk-approve-ui-")));
      await page.getByRole("button", { name: "初始化 / 打开数据集" }).click();
    }
    const base = (
      await (
        await page.request.post("/rpc", {
          data: {
            jsonrpc: "2.0",
            id: 1,
            method: "query.workspace",
            params: {},
          },
        })
      ).json()
    ).result;
    const project = {
      id: "approval-project",
      title: "Approve 测试项目",
      revision: 1,
      execution_mode: "live",
      binding: {},
      workspace_definition: {
        repository: "fixture/approval",
        mainline: "main",
      },
    };
    const feature = {
      id: "approval-feature",
      project_id: project.id,
      title: "内容校验测试",
      status: "planning",
      revision: 1,
      input_version: 1,
      binding: {},
    };
    const step: any = {
      id: "approval-step",
      key: "validate",
      round: 1,
      revision: 1,
      status: "waiting_confirmation",
      outputs: { attempt: "attempt-fixture", session: "formal-session" },
      input_hash: "current-report-hash",
      reason: "等待人类 Approve",
      confirmation: null,
    };
    const record = {
      id: "approval-runtime",
      kind: "workflow_instance",
      definition_id: feature.id,
      revision: 1,
      data: {
        owner_id: feature.id,
        status: "waiting_confirmation",
        paused: false,
        task_members: [],
        steps: [step],
      },
    };
    const definition = {
      key: "validate",
      name: "需求内容校验",
      kind: "attempt.execute",
      stage: "requirements",
      advance_mode: "automatic",
      interaction: {},
      parameters: { phase: "requirement_definition" },
    };
    const report = {
      run_id: "approval-model-run",
      node: "requirement_validate",
      approve: modelApprove,
      summary: "范围明确，失败恢复说明需要人工核对。",
      skills: ["selected-skill-v2"],
      checks: [
        {
          key: "failure_paths",
          passed: modelApprove,
          evidence: "PRD 第 4 节的上限恢复行为需要补充。",
        },
      ],
    };
    const commands: any[] = [];
    const session = {
      id: "formal-session",
      owner_id: feature.id,
      project_id: project.id,
      feature_id: feature.id,
      agent: "approval-agent",
      lifecycle: "idle",
      revision: 1,
      runtime_binding: {
        model: "fixture-model",
        work_type: "design",
        writable_paths: [],
        read_only_paths: [],
        installation: { key: { provider: "codex" } },
      },
    };
    const newestSession = {...session, id:"latest-formal-session"};
    await page.route("**/capabilities", (route) =>
      route.fulfill({
        json: {
          setup_required: false,
          active_project_id: project.id,
          protocol_version: 1,
        },
      }),
    );
    await page.route("**/rpc", async (route) => {
      const command = route.request().postDataJSON();
      let result: any;
      if (command.method === "query.workspace")
        result = {
          ...base,
          active_project: project,
          active_project_id: project.id,
          projects: [project],
          features: [feature],
          total_features: 1,
          repository_ready: true,
          configuration_ready: true,
          configurations: [record],
          detail: {
            feature,
            allowed_actions: {},
            totals: {},
            tasks: [],
            attempts: [],
            runs: [],
            documents: [],
            snapshots: [],
            contexts: [],
            deliveries: [],
            reviews: [],
            merges: [],
            resources: [],
            effects: [],
          },
        };
      else if (command.method === "query.workflow_instance")
        result = {
          record,
          plan: {
            stages: [{ key: "requirements", name: "需求阶段" }],
            steps: [definition],
          },
          configuration: {},
          children: [],
          actions: [
            {
              id: step.id,
              allowed:
                step.status === "waiting_confirmation"
                  ? ["workflow.step.confirm"]
                  : [],
              model_approvals: [report],
            },
          ],
        };
      else if (command.method === "query.configurations")
        result = { records: [], total: 0 };
      else if (command.method === "query.agent_revision")
        result = {
          id: session.agent,
          kind: "agent",
          name: "正式校验 Agent",
          version: 1,
          scope: { kind: "global" },
          data: {
            role: "analyst",
            skills: [],
            tools: [],
            model_ref: "model",
            harness_ref: "harness",
          },
        };
      else if (command.method === "query.sessions")
        result = { sessions: [newestSession, session] };
      else if (command.method === "query.session")
        result = {
          session: command.params.id === newestSession.id ? newestSession : session,
          formal_execution: true,
          output_contract: null,
          turns: [],
          events: [],
          next_cursor: 0,
          interactions: [],
          executions: [],
          proposals: [
            {
              id: "stray-proposal",
              status: "draft",
              revision: 1,
              dependencies: [],
              adopted_objects: [],
              turn_id: "standalone-turn",
              session_id: session.id,
              content: {
                kind: "requirement",
                title: "不属于正式校验的提案",
                body: "旧会话回复",
                acceptance: [],
              },
            },
          ],
        };
      else if (command.method === "workflow.step.confirm") {
        commands.push(command);
        step.status = "completed";
        step.confirmation = {
          human_approve: true,
          overwrite_model: command.params.payload.overwrite_model,
          reason: command.params.payload.reason,
        };
        record.revision++;
        record.data.status = "completed";
        result = { id: record.id };
      } else return route.continue();
      return route.fulfill({
        json: { jsonrpc: "2.0", id: command.id, result },
      });
    });
    await page.goto(
      `/?tab=execution&project_id=${project.id}&feature_id=${feature.id}`,
    );
    await expect(
      page.getByRole("button", { name: "继续讨论 / 查看提案", exact: true }),
    ).toHaveCount(0);
    await page
      .getByRole("button", { name: "查看执行会话", exact: true })
      .click();
    await expect.poll(async () => ({
      errors,
      returnLinks: await page.getByRole("link", { name: "返回当前工作流" }).count(),
    })).toEqual({ errors: [], returnLinks: 1 });
    await expect(page.getByLabel("讨论会话", {exact:true})).toHaveValue(newestSession.id);
    await expect(
      page.getByText("该会话用于正式执行或校验。", { exact: false }),
    ).toBeVisible();
    await expect(
      page.getByRole("button", { name: "采纳", exact: true }),
    ).toHaveCount(0);
    await expect(
      page.getByRole("button", { name: "发送", exact: true }),
    ).toBeDisabled();
    await page.getByRole("link", { name: "返回当前工作流" }).click();
    const panel = page.getByRole("region", { name: "内容校验与 Approve" });
    await expect(
      panel.getByRole("heading", {
        name: modelApprove ? "模型 Approve" : "模型 Reject",
      }),
    ).toBeVisible();
    await expect(panel).toContainText("PRD 第 4 节");
    const approve = page.getByRole("button", {
      name: modelApprove ? "人类 Approve" : "覆盖模型结论并 Approve",
      exact: true,
    });
    if (!modelApprove) {
      await expect(approve).toBeDisabled();
      await page
        .getByLabel("覆盖模型结论的理由")
        .fill("测试阶段接受该范围，保留恢复行为的后续改进项。");
    }
    await approve.click();
    await expect.poll(() => commands.length).toBe(1);
    expect(commands[0].params.payload.input_hash).toBe("current-report-hash");
    if (!modelApprove) {
      expect(commands[0].params.payload.overwrite_model).toBe(true);
      expect(commands[0].params.payload.reason).toContain("测试阶段接受");
    }
    await expect(panel).toContainText("人类已 Approve");
    expect(errors).toEqual([]);
  });
}

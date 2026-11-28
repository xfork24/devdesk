import { expect, type Page } from "@playwright/test";
import { test } from "./support/kernel";
import { useMockCodexCatalog } from "./support/model-catalog";
import type { Row } from "../../web/src/sdk/rpc-types";

useMockCodexCatalog();

async function rpc(page: Page, method: string, params: Row = {}) {
  const response = await page.request.post("/rpc", {
    data: { jsonrpc: "2.0", id: 1, method, params },
  });
  const body = await response.json();
  expect(body.error, JSON.stringify(body)).toBeUndefined();
  return body.result;
}
const command = (page: Page, method: string, payload: Row, target?: Row) =>
  rpc(page, method, {
    command_id: crypto.randomUUID(),
    payload,
    target: target?.id,
    expected_revision: target?.revision,
  });

test("Spec 正文之前展示确切 Delivery 与 Capability，Delivery 展示生产和消费步骤", async ({
  page,
}, info) => {
  await page.goto(
    "/?tab=workflows&config_tab=spec&config_object=builtin-spec-delivery-review%3A3",
  );
  const usage = page.getByLabel("被引用情况", { exact: true });
  await expect(
    usage.getByRole("link", { name: "Review Report", exact: true }),
  ).toBeVisible();
  await expect(
    usage.getByRole("link", { name: "Review Report", exact: true }),
  ).toHaveText("↗");
  const referenceGap = await usage
    .getByRole("link", { name: "Review Report", exact: true })
    .evaluate(
      (link) =>
        link.getBoundingClientRect().left -
        link.previousElementSibling!.getBoundingClientRect().right,
    );
  expect(Math.abs(referenceGap - 4)).toBeLessThan(1);
  await expect(
    usage.getByRole("link", {
      name: "Independent Review Capability · GLM 5.3",
      exact: true,
    }),
  ).toBeVisible();
  await expect(
    usage.getByRole("link", {
      name: "Independent Review Capability · GPT-6.2 Sol High",
      exact: true,
    }),
  ).toBeVisible();
  const body = page.getByRole("group", { name: "正文", exact: true });
  expect(
    await usage.evaluate(
      (node, body) =>
        !!(
          node.compareDocumentPosition(body!) & Node.DOCUMENT_POSITION_FOLLOWING
        ),
      await body.elementHandle(),
    ),
  ).toBe(true);
  const builtin = await rpc(page, "query.configuration_usage", {
    id: "builtin-spec-delivery-review:3",
    view: "builtin",
  });
  expect(
    builtin.users.some(
      (user: Row) => user.reference === "builtin-delivery-review:4",
    ),
  ).toBe(true);
  await usage
    .getByRole("link", {
      name: "Independent Review Capability · GLM 5.3",
      exact: true,
    })
    .click();
  await expect(page.getByLabel("名称", { exact: true })).toHaveValue(
    "Independent Review Capability · GLM 5.3",
  );
  const specLink = page.getByRole("link", {
    name: "查看 Review Report Specification · v3",
    exact: true,
  });
  await expect(specLink).toHaveText("↗");
  await expect(
    specLink.locator(
      "xpath=ancestor::span[contains(@class, 'configuration-field-title')]",
    ),
  ).toContainText("Spec 精确版本");
  for (const header of await page
    .locator(".configuration-field-title:has(a)")
    .all()) {
    const gap = await header.evaluate((node) => {
      const title = node.firstElementChild!.getBoundingClientRect();
      const icon = node.querySelector("a")!.getBoundingClientRect();
      return icon.left - title.right;
    });
    expect(Math.abs(gap - referenceGap)).toBeLessThan(1);
  }
  await page
    .getByRole("link", {
      name: "查看 Review Report Specification · v3",
      exact: true,
    })
    .click();
  await expect(page.getByLabel("名称", { exact: true })).toHaveValue(
    "Review Report Specification",
  );
  await usage.getByRole("link", { name: "Review Report", exact: true }).click();
  await expect(page.getByLabel("名称", { exact: true })).toHaveValue(
    "Review Report",
  );
  await page
    .getByRole("link", {
      name: "查看 Review Report Specification · v3",
      exact: true,
    })
    .click();
  await expect(page.getByLabel("名称", { exact: true })).toHaveValue(
    "Review Report Specification",
  );
  await page.goto(
    "/?tab=workflows&config_tab=delivery_type&config_object=builtin-delivery-prd%3A5",
  );
  const featureUsage = usage.locator(":scope > ul > li").filter({
    has: page.getByRole("link", {
      name: "Feature Development",
      exact: true,
    }),
  });
  await expect(
    featureUsage.getByRole("link", {
      name: "Feature Development",
      exact: true,
    }),
  ).toBeVisible();
  await expect(featureUsage).toContainText(
    "阶段：需求 · 步骤：Draft Product Requirements · 输出：result",
  );
  await expect(featureUsage).toContainText(
    "阶段：需求 · 步骤：Review and Approve PRD · 输入：work",
  );
  await page.screenshot({
    path: info.outputPath("delivery-reference-positions.png"),
    fullPage: true,
  });
  await page.setViewportSize({ width: 430, height: 932 });
  await page.screenshot({
    path: info.outputPath("delivery-reference-positions-mobile.png"),
    fullPage: true,
  });
  await featureUsage
    .getByRole("link", {
      name: "阶段：需求 · 步骤：Draft Product Requirements · 输出：result",
      exact: true,
    })
    .click();
  await expect(page.getByLabel("步骤名称", { exact: true })).toHaveValue(
    "Draft Product Requirements",
  );
  await expect(page.getByLabel("所属阶段", { exact: true })).toHaveValue(
    "requirements",
  );
  await expect(
    page.locator('[id="workflow-port:output:result"]'),
  ).toHaveAttribute("open", "");
  await expect(
    page.locator('[id="workflow-port:output:result"] > summary > span'),
  ).toHaveText(["输出：PRD", "类型：PRD v5"]);
  await page.getByRole("link", { name: "查看 PRD · v5", exact: true }).click();
  await expect(page.getByLabel("名称", { exact: true })).toHaveValue("PRD");
  await featureUsage
    .getByRole("link", {
      name: "阶段：需求 · 步骤：Review and Approve PRD · 输入：work",
      exact: true,
    })
    .click();
  await expect(page.getByLabel("步骤名称", { exact: true })).toHaveValue(
    "Review and Approve PRD",
  );
  await expect(page.locator('[id="workflow-port:input:work"]')).toHaveAttribute(
    "open",
    "",
  );
  await expect(
    page.locator('[id="workflow-port:input:work"] > summary > span'),
  ).toHaveText(["输入：PRD", "类型：PRD v5"]);
  await page.getByRole("link", { name: "查看 PRD", exact: true }).click();
  await expect(page.getByLabel("名称", { exact: true })).toHaveValue("PRD");
});

test("Spec 引用可定位到默认评审步骤和 GPT 需求分析能力", async ({ page }) => {
  const scenarios = [
    [
      "builtin-spec-feature-code-review:2",
      "Feature Development",
      "Review Feature Changes",
    ],
    [
      "builtin-spec-task-code-review:2",
      "Task Development",
      "Review Task Changes Independently",
    ],
  ];
  const usage = page.getByLabel("被引用情况", { exact: true });
  for (const [spec, workflow, step] of scenarios) {
    await page.goto(
      `/?tab=workflows&config_tab=spec&config_object=${encodeURIComponent(spec)}`,
    );
    await expect(
      usage.getByRole("link", { name: workflow, exact: true }),
    ).toBeVisible();
    await usage
      .getByRole("link", { name: new RegExp(`步骤：${step} · 适用规范`) })
      .click();
    await expect(page.getByLabel("步骤名称", { exact: true })).toHaveValue(
      step,
    );
    await expect(page.getByLabel("验收 Spec", { exact: true })).toHaveValues([
      spec,
    ]);
  }

  await page.goto(
    "/?tab=workflows&config_tab=spec&config_object=builtin-spec-stage-prd%3A4",
  );
  await expect(
    usage.getByRole("link", {
      name: "Requirements Analysis Capability · GPT-6.2 Sol High",
      exact: true,
    }),
  ).toBeVisible();
  const agents = await rpc(page, "query.configurations", {
    kind: "agent",
    limit: 100,
  });
  const reviewer = agents.records.filter(
    (row: Row) => row.role_ref === "builtin-role-reviewer:1",
  );
  expect(reviewer.map((row: Row) => row.name).sort()).toEqual([
    "Independent Reviewer · GLM 5.3",
    "Independent Reviewer · GPT-6.2 Sol High",
    "Independent Reviewer · MiniMax M3.1 Flash",
  ]);
});

test("Spec 检查项默认折叠，编辑与删除保持对应行", async ({ page }) => {
  await page.goto("/");
  const spec = await command(page, "spec.create", {
    name: "检查项列表验收",
    data: {
      schema_version: 3,
      body: "检查交付物的范围和证据。",
      checks: [
        { key: "scope", name: "范围", instructions: "检查范围。" },
        { key: "evidence", name: "证据", instructions: "检查证据。" },
      ],
    },
  });
  await page.goto(
    `/?tab=workflows&config_tab=spec&config_object=${encodeURIComponent(spec.id)}`,
  );
  const rows = page.locator(".configuration-disclosure-list > details");
  const scope = rows.nth(0);
  const evidence = rows.nth(1);
  await expect(scope.locator("summary > span")).toHaveText(["范围"]);
  await expect(evidence.locator("summary > span")).toHaveText(["证据"]);
  await expect(
    page.getByLabel("检查标识", { exact: true }).first(),
  ).toBeHidden();
  await scope.locator("summary").press("Space");
  await scope
    .getByLabel("检查标识", { exact: true })
    .fill("check-scope-and-version");
  await scope.getByLabel("检查名称", { exact: true }).fill("范围与版本");
  await expect(scope.locator("summary > span")).toHaveText(["范围与版本"]);
  await scope.getByRole("button", { name: "删除检查项" }).click();
  await expect(rows).toHaveCount(1);
  await expect(rows.first().locator("summary > span")).toHaveText(["证据"]);
  await expect(
    rows.first().getByLabel("检查名称", { exact: true }),
  ).toBeHidden();
  await page.getByRole("button", { name: "添加检查项" }).click();
  const added = rows.last();
  await expect(added.getByLabel("检查名称", { exact: true })).toBeVisible();
  await added.locator("summary").click();
  await page.getByRole("button", { name: "保存 Spec 新版本" }).click();
  await expect(added.getByLabel("检查标识", { exact: true })).toBeFocused();
  await added
    .getByLabel("检查标识", { exact: true })
    .fill("check-acceptance-criteria");
  await added.getByLabel("检查名称", { exact: true }).fill("验收");
  await added.getByRole("button", { name: "检查要求 2", exact: true }).click();
  await added.getByRole("textbox", { name: "检查要求 2" }).fill("检查验收。");
  await page.getByRole("button", { name: "保存 Spec 新版本" }).click();
  await expect(page.getByRole("status")).toHaveText(/新修订已保存/);
});

test("工具参数和子流程集合的 Delivery 引用可以双向定位", async ({ page }) => {
  await page.goto(
    "/?tab=workflows&config_tab=delivery_type&config_object=builtin-delivery-test-design%3A2",
  );
  const usage = page.getByLabel("被引用情况", { exact: true });
  await usage
    .getByRole("link", {
      name: "阶段：独立测试 · 步骤：Run Red Tests · 工具参数：recipe",
      exact: true,
    })
    .click();
  await expect(page.getByLabel("步骤名称", { exact: true })).toHaveValue(
    "Run Red Tests",
  );
  const argument = page.locator('[id="workflow-port:argument:recipe"]');
  await expect(argument).toBeInViewport();
  await argument
    .getByRole("link", { name: "查看 Test Design", exact: true })
    .click();
  await expect(page.getByLabel("名称", { exact: true })).toHaveValue(
    "Test Design",
  );
  await page.goto(
    "/?tab=workflows&config_tab=delivery_type&config_object=builtin-delivery-task-creation-report%3A3",
  );
  await usage
    .locator(":scope > ul > li")
    .filter({
      has: page.getByRole("link", {
        name: "Feature Development",
        exact: true,
      }),
    })
    .getByRole("link", {
      name: "阶段：实施 · 步骤：Execute Task Dependency Graph · 遍历来源",
      exact: true,
    })
    .click();
  await expect(page.getByLabel("步骤名称", { exact: true })).toHaveValue(
    "Execute Task Dependency Graph",
  );
  const collection = page.locator('[id="workflow-port:collection"]');
  await expect(collection).toBeInViewport();
  await collection
    .getByRole("link", { name: "查看 Task Creation Report", exact: true })
    .click();
  await expect(page.getByLabel("名称", { exact: true })).toHaveValue(
    "Task Creation Report",
  );
});

test("Skill、HTTP 工具和原生工具显示 Capability 引用，提交后同步刷新", async ({
  page,
}, info) => {
  await page.goto("/");
  const source = await command(page, "integration.create", {
    name: "Usage API",
    data: { source: { kind: "http_api", base_url: "https://example.test" } },
  });
  const tool = await command(page, "integration.tool.create", {
    name: "Usage tool",
    data: {
      schema_version: 2,
      qualified_name: "usage_lookup",
      description: "Usage fixture",
      integration_ref: source.id,
      implementation: {
        kind: "http_api",
        method: "GET",
        path: "/items",
        parameters: [],
      },
      input_schema: { type: "object" },
      effect: "read_only",
    },
  });
  const payload = {
    name: "引用演示能力",
    model_ref: "builtin-model-gpt-6-2-sol-high:1",
    harness_ref: "builtin-harness-codex-managed:2",
    skills: ["builtin-skill-software-delivery:1"],
    tools: ["builtin-tool-codex-native:2", tool.id],
    data: {},
  };
  const capability = await command(page, "capability.create", payload);
  await page.goto(
    "/?tab=components&config_tab=skills&config_object=builtin-skill-software-delivery%3A1",
  );
  await expect(
    page
      .getByLabel("被引用情况")
      .getByRole("link", { name: payload.name, exact: true }),
  ).toBeVisible();
  await page
    .getByLabel("被引用情况")
    .getByRole("link", { name: payload.name, exact: true })
    .click();
  await page
    .getByRole("link", { name: "查看 Software Delivery · v1", exact: true })
    .click();
  await expect(page.getByLabel("名称", { exact: true })).toHaveValue(
    "Software Delivery",
  );
  await page
    .getByLabel("被引用情况")
    .getByRole("link", { name: payload.name, exact: true })
    .click();
  await page
    .getByRole("link", { name: "查看 Codex Native Tools · v2", exact: true })
    .click();
  await expect(
    page.getByRole("tab", { name: "Harness Tools", exact: true }),
  ).toHaveAttribute("aria-selected", "true");
  const native = page.locator(".native-tool-group").filter({
    has: page.getByRole("heading", { name: "Codex Native Tools", exact: true }),
  });
  await expect(
    native
      .getByLabel("被引用情况")
      .getByRole("link", { name: payload.name, exact: true }),
  ).toBeVisible();
  await page.screenshot({
    path: info.outputPath("native-capability-references.png"),
    fullPage: true,
  });
  await native
    .getByLabel("被引用情况")
    .getByRole("link", { name: payload.name, exact: true })
    .click();
  await page
    .getByRole("link", { name: "查看 Usage tool · v1", exact: true })
    .click();
  await expect(
    page
      .getByRole("region", { name: "HTTP 动作管理" })
      .getByLabel("工具调用名称", { exact: true }),
  ).toHaveValue("usage_lookup");
  const usage = page
    .getByRole("region", { name: "HTTP 动作管理" })
    .getByLabel("被引用情况");
  await expect(
    usage.getByRole("link", { name: payload.name, exact: true }),
  ).toBeVisible();
  await command(
    page,
    "capability.revise",
    { ...payload, tools: ["builtin-tool-codex-native:2"] },
    capability,
  );
  await expect(
    usage.getByRole("link", { name: payload.name, exact: true }),
  ).toHaveCount(0);
  await expect(usage).toContainText("当前没有配置引用此修订。");
  await page.goto(
    "/?tab=actors&config_tab=capability&config_object=builtin-capability-reviewer-gpt-6-2-sol-codex%3A4",
  );
  await page
    .getByRole("link", {
      name: "查看 Validate Specification · v2",
      exact: true,
    })
    .click();
  const internal = page.getByRole("region", {
    name: "Internal Tools",
    exact: true,
  });
  await expect(
    page.getByRole("tab", { name: "Internal Tools", exact: true }),
  ).toHaveAttribute("aria-selected", "true");
  const toolList = internal.locator(".configuration-list");
  await expect(toolList.getByRole("button")).toHaveCount(49);
  const detail = internal.getByRole("article", { name: "内部工具详情" });
  await expect(detail.locator("code").first()).toHaveText("spec.lint");
  const [listBox, detailBox] = await Promise.all([
    toolList.boundingBox(),
    detail.boundingBox(),
  ]);
  expect(listBox!.x + listBox!.width).toBeLessThan(detailBox!.x);
  await expect(
    detail.getByLabel("被引用情况").getByRole("link", {
      name: "Independent Review Capability · GPT-6.2 Sol High",
      exact: true,
    }),
  ).toBeVisible();
  for (const model of ["GLM 5.3", "GPT-6.2 Sol High", "MiniMax M3.1 Flash"]) {
    await expect(
      detail.getByLabel("被引用情况").getByRole("link", {
        name: `Project Management Capability · ${model}`,
        exact: true,
      }),
    ).toBeVisible();
  }
  await internal.getByRole("button", { name: /Run Independent Tests/ }).click();
  await expect(detail.locator("code").first()).toHaveText("tests.run");
  await expect(detail).toContainText("builtin-sandbox-docker:2");
  const testUsage = detail.getByLabel("被引用情况");
  await expect(
    testUsage.getByRole("link", { name: "Task Development", exact: true }),
  ).toBeVisible();
  const redStep = testUsage.getByRole("link", {
    name: "阶段：独立测试 · 步骤：Run Red Tests · 执行工具",
    exact: true,
  });
  await expect(redStep).toHaveAttribute("href", /workflow_node=step%3Ared/);
  await expect(
    testUsage.getByRole("link", { name: /步骤：Run Independent Green Tests/ }),
  ).toBeVisible();
  await detail.getByText("工具输入与输出合同", { exact: true }).click();
  await expect(detail.locator("pre")).toContainText(
    '"expect_assertion_failure"',
  );
  await page.screenshot({
    path: info.outputPath("internal-tools.png"),
    fullPage: true,
  });
  await page.reload();
  await expect(detail.locator("code").first()).toHaveText("tests.run");
  await redStep.click();
  await expect(page.getByLabel("步骤名称", { exact: true })).toHaveValue(
    "Run Red Tests",
  );
  await page
    .getByRole("link", { name: "查看 Run Independent Tests · v3", exact: true })
    .click();
  await expect(detail.locator("code").first()).toHaveText("tests.run");
  await toolList
    .getByRole("button", { name: /Run Project Regression Tests/ })
    .click();
  await expect(
    testUsage.getByRole("link", { name: "Feature Development", exact: true }),
  ).toBeVisible();
  await expect(
    testUsage.getByRole("link", { name: "Task Development", exact: true }),
  ).toBeVisible();
  for (const skill of [
    "builtin-skill-code-review:1",
    "builtin-skill-independent-review:3",
  ]) {
    await page.goto(
      `/?tab=components&config_tab=skills&config_object=${encodeURIComponent(skill)}`,
    );
    const skillUsage = page.getByLabel("被引用情况", { exact: true });
    for (const model of ["GLM 5.3", "GPT-6.2 Sol High", "MiniMax M3.1 Flash"]) {
      await expect(
        skillUsage.getByRole("link", {
          name: `Independent Review Capability · ${model}`,
          exact: true,
        }),
      ).toBeVisible();
    }
  }
});

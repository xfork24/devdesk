import { expect, type Page } from "@playwright/test";
import { test } from "./support/kernel";
import { useMockCodexCatalog } from "./support/model-catalog";
import type { Row } from "../src/components/rpc-types";

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
    "/?tab=workflows&config_tab=spec&config_object=builtin-spec-delivery-review%3A1",
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
      name: "评审能力 · GLM 5.3 Flash / DSH",
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
    id: "builtin-spec-delivery-review:1",
    view: "builtin",
  });
  expect(
    builtin.users.some(
      (user: Row) => user.reference === "builtin-delivery-review:2",
    ),
  ).toBe(true);
  await usage
    .getByRole("link", { name: "评审能力 · GLM 5.3 Flash / DSH", exact: true })
    .click();
  await expect(page.getByLabel("名称", { exact: true })).toHaveValue(
    "评审能力 · GLM 5.3 Flash / DSH",
  );
  const specLink = page.getByRole("link", {
    name: "查看 Review Report 内容规范 · v1",
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
      name: "查看 Review Report 内容规范 · v1",
      exact: true,
    })
    .click();
  await expect(page.getByLabel("名称", { exact: true })).toHaveValue(
    "Review Report 内容规范",
  );
  await usage.getByRole("link", { name: "Review Report", exact: true }).click();
  await expect(page.getByLabel("名称", { exact: true })).toHaveValue(
    "Review Report",
  );
  await page
    .getByRole("link", {
      name: "查看 Review Report 内容规范 · v1",
      exact: true,
    })
    .click();
  await expect(page.getByLabel("名称", { exact: true })).toHaveValue(
    "Review Report 内容规范",
  );
  await page.goto(
    "/?tab=workflows&config_tab=delivery_type&config_object=builtin-delivery-prd%3A3",
  );
  const featureUsage = usage.locator(":scope > ul > li").filter({
    has: page.getByRole("link", {
      name: "精简版Feature开发流程",
      exact: true,
    }),
  });
  await expect(
    featureUsage.getByRole("link", {
      name: "精简版Feature开发流程",
      exact: true,
    }),
  ).toBeVisible();
  await expect(featureUsage).toContainText(
    "阶段：需求 · 步骤：F01 澄清需求与 PRD · 输出：result",
  );
  await expect(featureUsage).toContainText(
    "阶段：需求 · 步骤：F02 PRD 验收与人工确认 · 输入：work",
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
      name: "阶段：需求 · 步骤：F01 澄清需求与 PRD · 输出：result",
      exact: true,
    })
    .click();
  await expect(page.getByLabel("步骤名称", { exact: true })).toHaveValue(
    "F01 澄清需求与 PRD",
  );
  await expect(page.getByLabel("所属阶段", { exact: true })).toHaveValue(
    "requirements",
  );
  await page.getByRole("link", { name: "查看 PRD · v3", exact: true }).click();
  await expect(page.getByLabel("名称", { exact: true })).toHaveValue("PRD");
  await featureUsage
    .getByRole("link", {
      name: "阶段：需求 · 步骤：F02 PRD 验收与人工确认 · 输入：work",
      exact: true,
    })
    .click();
  await expect(page.getByLabel("步骤名称", { exact: true })).toHaveValue(
    "F02 PRD 验收与人工确认",
  );
  await page.getByRole("link", { name: "查看 PRD", exact: true }).click();
  await expect(page.getByLabel("名称", { exact: true })).toHaveValue("PRD");
});

test("工具参数和子流程集合的 Delivery 引用可以双向定位", async ({ page }) => {
  await page.goto(
    "/?tab=workflows&config_tab=delivery_type&config_object=simplified-delivery-test-recipe%3A1",
  );
  const usage = page.getByLabel("被引用情况", { exact: true });
  await usage
    .getByRole("link", {
      name: "阶段：独立测试 · 步骤：T02 执行真实 Red · 工具参数：recipe",
      exact: true,
    })
    .click();
  await expect(page.getByLabel("步骤名称", { exact: true })).toHaveValue(
    "T02 执行真实 Red",
  );
  const argument = page.locator('[id="workflow-port:argument:recipe"]');
  await expect(argument).toBeInViewport();
  await argument
    .getByRole("link", { name: "查看 精简版独立测试配方", exact: true })
    .click();
  await expect(page.getByLabel("名称", { exact: true })).toHaveValue(
    "精简版独立测试配方",
  );
  await page.goto(
    "/?tab=workflows&config_tab=delivery_type&config_object=builtin-delivery-task-creation-report%3A1",
  );
  await usage
    .locator(":scope > ul > li")
    .filter({
      has: page.getByRole("link", {
        name: "精简版Feature开发流程",
        exact: true,
      }),
    })
    .getByRole("link", {
      name: "阶段：实施 · 步骤：F07 按 DAG 并发开发 Tasks · 遍历来源",
      exact: true,
    })
    .click();
  await expect(page.getByLabel("步骤名称", { exact: true })).toHaveValue(
    "F07 按 DAG 并发开发 Tasks",
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
    model_ref: "builtin-model-gpt-6-1-sol-high:1",
    harness_ref: "builtin-harness-codex-managed:1",
    skills: ["builtin-skill-engineering-method:1"],
    tools: ["builtin-tool-codex-native:1", tool.id],
    data: {},
  };
  const capability = await command(page, "capability.create", payload);
  await page.goto(
    "/?tab=components&config_tab=skills&config_object=builtin-skill-engineering-method%3A1",
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
    .getByRole("link", { name: "查看 默认分阶段开发方法 · v1", exact: true })
    .click();
  await expect(page.getByLabel("名称", { exact: true })).toHaveValue(
    "默认分阶段开发方法",
  );
  await page
    .getByLabel("被引用情况")
    .getByRole("link", { name: payload.name, exact: true })
    .click();
  await page
    .getByRole("link", { name: "查看 Codex 原生工具组 · v1", exact: true })
    .click();
  await expect(
    page.getByRole("tab", { name: "Harness原生工具", exact: true }),
  ).toHaveAttribute("aria-selected", "true");
  const native = page.locator(".native-tool-group").filter({
    has: page.getByRole("heading", { name: "Codex 原生工具组", exact: true }),
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
    { ...payload, tools: ["builtin-tool-codex-native:1"] },
    capability,
  );
  await expect(
    usage.getByRole("link", { name: payload.name, exact: true }),
  ).toHaveCount(0);
  await expect(usage).toContainText("当前没有配置引用此修订。");
  await page.goto(
    "/?tab=actors&config_tab=capability&config_object=simplified-capability-reviewer%3A4",
  );
  await page
    .getByRole("link", {
      name: "查看 检查规范结构与内容样本 · v1",
      exact: true,
    })
    .click();
  await expect(
    page
      .getByRole("region", { name: "Tool 定义", exact: true })
      .locator("code")
      .first(),
  ).toHaveText("spec.lint");
  await expect(
    page
      .getByRole("region", { name: "Tool 定义", exact: true })
      .getByLabel("被引用情况")
      .getByRole("link", {
        name: "精简版reviewer能力",
        exact: true,
      }),
  ).toBeVisible();
});

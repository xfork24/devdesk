import { test, expect, Page } from "@playwright/test";
import { mkdtempSync } from "node:fs";
import { tmpdir } from "node:os";
import { join } from "node:path";
async function setup(page: Page) {
  await page.goto("/");
  if (await page.getByLabel("数据目录").count()) {
    await page
      .getByLabel("数据目录")
      .fill(mkdtempSync(join(tmpdir(), "devdesk-workflow-web-")));
    await page.getByRole("button", { name: "初始化 / 打开数据集" }).click();
  }
  await page.getByRole("button", { name: "工作流程", exact: true }).click();
}
test("Workflow 编辑、版本发布和刷新恢复使用真实 RPC 与 SQLite", async ({
  page,
}, info) => {
  await setup(page);
  await page
    .getByLabel("流程名称", { exact: true })
    .fill("浏览器工作流 " + crypto.randomUUID().slice(0, 6));
  await page
    .getByRole("button", { name: "新建 Workflow", exact: true })
    .click();
  await expect(
    page.getByText("草稿 revision 1", { exact: false }),
  ).toBeVisible();
  await page
    .getByRole("button", { name: "需求头脑风暴", exact: true })
    .first()
    .click();
  await page
    .getByLabel("讨论目标", { exact: true })
    .fill("明确需求、验收和错误路径，输出可采纳需求。");
  await page
    .getByLabel("推进策略", { exact: true })
    .selectOption("human_confirmation");
  await page.getByRole("button", { name: "保存草稿", exact: true }).click();
  await expect(
    page.getByRole("status").filter({ hasText: "已保存" }),
  ).toBeVisible();
  await page.getByRole("button", { name: "校验流程", exact: true }).click();
  await expect(page.getByText("校验通过", { exact: true })).toBeVisible();
  await page.getByRole("button", { name: "预演", exact: true }).click();
  await expect(
    page.getByText("演练 · 不产生工程完成事实", { exact: true }),
  ).toBeVisible();
  await expect(
    page.locator("li").filter({ hasText: "等待前置" }).first(),
  ).toBeVisible();
  await page
    .getByLabel("样本已产生输出的步骤")
    .selectOption(["initial_snapshot"]);
  await page
    .getByRole("button", { name: "按样本重新演练", exact: true })
    .click();
  await expect(
    page.locator("li").filter({ hasText: "等待交互" }).first(),
  ).toBeVisible();
  await page.getByRole("button", { name: "发布版本", exact: true }).click();
  await expect(page.getByText("发布版本 v1", { exact: false })).toBeVisible();
  await page
    .getByRole("button", { name: "需求头脑风暴", exact: true })
    .first()
    .click();
  await expect(page.getByLabel("讨论目标", { exact: true })).toBeDisabled();
  await page.reload();
  await expect(
    page.getByRole("heading", { name: "Workflow 定义与版本", exact: true }),
  ).toBeVisible();
  await page
    .getByRole("button", { name: "浏览器工作流", exact: false })
    .last()
    .click();
  await page.screenshot({
    path: info.outputPath("workflow-manager.png"),
    fullPage: true,
  });
});
test("Agent 组合管理不依赖 GitHub 连接且可以创建 Model 和 Tool", async ({
  page,
}) => {
  await setup(page);
  await page.getByRole("button", { name: "规范与身份", exact: true }).click();
  await expect(
    page.getByRole("heading", { name: "Agent 组合与实例" }),
  ).toBeVisible();
  await page.getByRole("button", { name: "Model", exact: true }).click();
  const panel = page
    .locator("section")
    .filter({ has: page.getByRole("heading", { name: "Agent 组合与实例" }) });
  await panel.getByLabel("名称", { exact: true }).fill("Browser Model");
  await panel.getByLabel("模型标识", { exact: true }).fill("fixture-model");
  await panel.getByRole("button", { name: "创建定义", exact: true }).click();
  await expect(
    panel.getByRole("button", { name: /Browser Model v1/ }),
  ).toBeVisible();
  await panel.getByRole("button", { name: "Tool", exact: true }).click();
  await panel
    .getByLabel("名称", { exact: true })
    .fill("Browser experience tool");
  await panel.getByLabel("执行处理器").selectOption("experience.lookup");
  await panel.getByRole("button", { name: "创建定义", exact: true }).click();
  await expect(
    panel.getByRole("button", { name: /Browser experience tool v1/ }),
  ).toBeVisible();
});

test("全新数据集内建完整 Sol High / Codex 套件，角色表单自动采用默认组合", async ({
  page,
}) => {
  await setup(page);
  const defaults = page.getByRole("region", { name: "内建默认套件" });
  await expect(defaults.getByText(/GPT-6.1 Sol High · Codex/)).toBeVisible();
  await defaults.getByText("查看 Workflow、规范、角色与运行配置").click();
  for (const role of ["需求分析师", "架构师", "开发", "测试", "评审"])
    await expect(
      defaults.getByText(`${role} · Sol High / Codex`, { exact: true }),
    ).toBeVisible();
  await page.getByRole("button", { name: "规范与身份", exact: true }).click();
  const catalog = page.locator("section").filter({
    has: page.getByRole("heading", { name: "Agent 组合与实例", exact: true }),
  });
  await expect(catalog.getByLabel("Model 精确版本")).toHaveValue(
    "builtin-model-gpt-6-1-sol-high-v1",
  );
  await catalog
    .getByRole("combobox", { name: "Role 精确版本", exact: true })
    .selectOption("builtin-role-developer-v1");
  await expect(catalog.getByLabel("Model 精确版本")).toHaveValue(
    "builtin-model-gpt-6-1-sol-high-v1",
  );
  await expect(catalog.getByLabel("Harness 精确版本")).toHaveValue(
    "builtin-harness-codex-managed-v1",
  );
});

test("Role 可以新增、编辑和删除，默认 Agent 自动创建", async ({ page }) => {
  await setup(page);
  await page.getByRole("button", { name: "规范与身份", exact: true }).click();
  const panel = page.getByRole("region", { name: "角色管理" });
  await panel.getByLabel("Role 名称", { exact: true }).fill("浏览器安全评审员");
  await panel
    .getByRole("combobox", { name: "执行职责", exact: true })
    .selectOption("reviewer");
  await panel
    .getByLabel("Role 职责说明", { exact: true })
    .fill("Review security boundaries and failure handling with evidence.");
  await panel.getByRole("button", { name: "创建 Role", exact: true }).click();
  await expect(
    panel.getByRole("button", { name: /浏览器安全评审员 独立评审/ }),
  ).toBeVisible();
  await panel
    .getByRole("button", { name: /浏览器安全评审员 独立评审/ })
    .click();
  await panel
    .getByLabel("Role 名称", { exact: true })
    .fill("浏览器高级安全评审员");
  await panel
    .getByRole("button", { name: "保存 Role 新版本", exact: true })
    .click();
  await expect(
    panel.getByRole("button", { name: /浏览器高级安全评审员 独立评审 · v2/ }),
  ).toBeVisible();
  await panel
    .getByRole("button", { name: "删除 浏览器高级安全评审员", exact: true })
    .click();
  await expect(
    panel.getByRole("button", { name: /浏览器高级安全评审员 独立评审/ }),
  ).toHaveCount(0);
  await page.reload();
  await expect(
    page
      .getByRole("region", { name: "角色管理" })
      .getByRole("button", { name: /浏览器高级安全评审员 独立评审/ }),
  ).toHaveCount(0);
});

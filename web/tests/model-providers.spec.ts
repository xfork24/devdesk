import { test, expect } from "@playwright/test";
import { mkdtempSync } from "node:fs";
import { tmpdir } from "node:os";
import { join } from "node:path";

test("DSH 服务商凭据独立，Ollama 免密且模型草稿可保存", async ({
  page,
}, info) => {
  const errors: string[] = [];
  page.on("pageerror", (error) => errors.push(error.message));
  await page.goto("/");
  if (
    (await page.request.get("/capabilities").then((r) => r.json()))
      .setup_required
  ) {
    await page
      .getByLabel("数据目录")
      .fill(mkdtempSync(join(tmpdir(), "devdesk-model-provider-web-")));
    await page.getByRole("button", { name: "初始化 / 打开数据集" }).click();
    await expect(page.getByLabel("数据目录")).toHaveCount(0);
  }
  const configured: string[] = [];
  // Vault writes are simulated. This test never stores a key or calls a model.
  await page.route("**/rpc", async (route) => {
    const call = route.request().postDataJSON();
    if (call.method === "codex.auth.models")
      return route.fulfill({
        json: {
          jsonrpc: "2.0",
          id: call.id,
          result: { models: [{ model: "gpt-6.1-sol" }] },
        },
      });
    if (call.method === "models.list")
      return route.fulfill({
        json: {
          jsonrpc: "2.0",
          id: call.id,
          result: {
            source: "host_endpoint",
            models: [
              {
                id: "fixture-local-model",
                context_window: 32768,
                max_output_tokens: 4096,
              },
            ],
          },
        },
      });
    if (!call.method.startsWith("credentials.")) return route.fallback();
    const provider = call.params.model_provider || "deepseek";
    if (call.method === "credentials.configure") configured.push(provider);
    await route.fulfill({
      json: {
        jsonrpc: "2.0",
        id: call.id,
        result: {
          model_provider: provider,
          credential_version: {
            resource: { kind: "setting", id: `credentials.dsh.${provider}` },
            revision: configured.includes(provider) ? 1 : 0,
          },
          provider_ref: `fixture-provider-${provider}:1`,
          configured: configured.includes(provider),
          available: true,
          storage: "fixture_vault",
          auth_kind: provider === "ollama" ? "none" : "api_key",
        },
      },
    });
  });
  await page.getByRole("button", { name: "设置", exact: true }).click();
  const select = page.getByLabel("DSH 模型服务商", { exact: true });
  const harnessHeading = page.getByRole("heading", {
    name: "DeepSeek Harness",
    exact: true,
  });
  await expect(harnessHeading).toBeVisible();
  await expect(select.locator("option")).toHaveCount(4);
  await expect(select).toHaveValue("minimax-cn");
  for (const provider of ["zai-coding-cn", "minimax-cn"]) {
    await select.selectOption(provider);
    await expect(harnessHeading).toBeVisible();
    await page
      .getByLabel("配置或更换 API key", { exact: true })
      .fill("FAKE_BROWSER_TEST_VALUE");
    await page.getByRole("button", { name: "保存密钥", exact: true }).click();
    await expect(
      page.getByLabel("配置或更换 API key", { exact: true }),
    ).toHaveValue("");
    await expect(
      page.getByText("已保存到系统凭据库", { exact: true }),
    ).toBeVisible();
  }
  expect(configured).toEqual(["zai-coding-cn", "minimax-cn"]);
  await select.selectOption("ollama");
  await expect(select.locator('option[value="ollama"]')).toHaveText(
    "本地 Ollama",
  );
  await expect(harnessHeading).toBeVisible();
  await expect(page.getByLabel("配置或更换 API key")).toHaveCount(0);
  await expect(page.getByText(/本地服务无需 API key/)).toBeVisible();
  const address = page.getByLabel("Ollama 协议地址", { exact: true });
  await expect(address).toHaveValue("http://127.0.0.1:11434");
  await address.fill("http://127.0.0.1:11435/v1");
  await page
    .getByRole("button", { name: "保存 Ollama 地址", exact: true })
    .click();
  await expect(address).toHaveValue("http://127.0.0.1:11435");
  await expect(page.getByText(/Ollama 默认服务地址已保存/)).toBeVisible();
  await page.reload();
  await select.selectOption("ollama");
  await expect(address).toHaveValue("http://127.0.0.1:11435");
  await page.screenshot({
    path: info.outputPath("ollama-settings.png"),
    fullPage: true,
  });
  await page.getByRole("button", { name: "基础组件", exact: true }).click();
  await page.getByRole("tab", { name: "Model", exact: true }).click();
  await page.getByRole("button", { name: "新建 Model", exact: true }).click();
  await page.getByLabel("模型后端", { exact: true }).selectOption("ollama");
  await expect(page.getByLabel("Ollama 服务地址", { exact: true })).toHaveValue(
    "http://127.0.0.1:11435",
  );
  await page.getByLabel("名称", { exact: true }).fill("Ollama 浏览器回归");
  await page
    .getByLabel("模型标识", { exact: true })
    .selectOption("fixture-local-model");
  await page
    .getByLabel("Ollama 服务地址", { exact: true })
    .fill("http://127.0.0.1:11434");
  await page.getByLabel("上下文容量", { exact: true }).fill("32768");
  await page.getByLabel("最大输出容量", { exact: true }).fill("4096");
  await page.getByLabel("最大输出 Tokens", { exact: true }).fill("2048");
  await page.getByRole("button", { name: "创建 Model", exact: true }).click();
  await expect(
    page.getByText("新修订已保存，已有使用方保持原引用。", { exact: true }),
  ).toBeVisible();
  const response = await page.request.post("/rpc", {
    data: {
      jsonrpc: "2.0",
      id: 1,
      method: "query.configurations",
      params: { kind: "model", include_history: true },
    },
  });
  const record = (await response.json()).result.records.find(
    (r: { name: string }) => r.name === "Ollama 浏览器回归",
  );
  expect(record.data).toMatchObject({
    provider: "ollama",
    endpoint: "http://127.0.0.1:11434",
    context_window: 32768,
    max_output_tokens: 4096,
    parameters: { max_tokens: 2048 },
  });
  expect(errors).toEqual([]);
});

import { expect, test, Page } from "@playwright/test";
import { spawn } from "node:child_process";
import { mkdtempSync, rmSync } from "node:fs";
import { tmpdir } from "node:os";
import { join, resolve } from "node:path";
import { createInterface } from "node:readline";

async function settings(page: Page, origin = "/") {
  await page.goto(origin);
  const capabilities = await page.request
    .get("/capabilities")
    .then((response) => response.json());
  if (capabilities.setup_required) {
    await page
      .getByLabel("数据目录", { exact: true })
      .fill(mkdtempSync(join(tmpdir(), "devdesk-connection-client-")));
    await page
      .getByRole("button", { name: "初始化 / 打开数据集", exact: true })
      .click();
  }
  await page.getByRole("button", { name: "设置", exact: true }).click();
  await expect(
    page.getByRole("heading", { name: "数据与运行", exact: true }),
  ).toBeVisible();
}

async function protectedKernel() {
  const directory = mkdtempSync(join(tmpdir(), "devdesk-connection-target-"));
  const key = "synthetic-connection-key-for-browser-fixture-only";
  const child = spawn(
    resolve(
      import.meta.dirname,
      "../../target",
      process.env.DEVDESK_E2E_PROFILE ?? "debug",
      "devdesk-host",
    ),
    [
      "--data",
      directory,
      "--host",
      process.env.DEVDESK_CONNECTION_TEST_HOST ?? "127.0.0.1",
      "--port",
      "0",
    ],
    {
      env: { ...process.env, DEVDESK_ACCESS_TOKEN: key },
      stdio: ["ignore", "pipe", "pipe"],
    },
  );
  let diagnostics = "";
  child.stderr.on("data", (data) => {
    diagnostics = (diagnostics + data).slice(-4000);
  });
  const stop = async () => {
    if (child.exitCode === null) {
      await new Promise<void>((done) => {
        const timer = setTimeout(() => child.kill("SIGKILL"), 5000);
        child.once("exit", () => {
          clearTimeout(timer);
          done();
        });
        child.kill("SIGINT");
      });
    }
    rmSync(directory, { recursive: true, force: true });
  };
  try {
    const origin = await new Promise<string>((done, fail) => {
      const lines = createInterface({ input: child.stdout });
      const timer = setTimeout(
        () => fail(new Error("Target kernel startup timed out")),
        15000,
      );
      child.once("exit", () => {
        clearTimeout(timer);
        fail(new Error(diagnostics));
      });
      lines.on("line", (line) => {
        if (line.startsWith("http://")) {
          clearTimeout(timer);
          lines.close();
          done(line.trim());
        }
      });
    });
    return { origin, key, stop };
  } catch (error) {
    await stop();
    throw error;
  }
}

test("内核连接切换真实后端、认证后隔离数据，退出后仍可连接原内核", async ({
  page,
  baseURL,
}, testInfo) => {
  const target = await protectedKernel();
  let releaseSnapshot = () => {};
  try {
    // Remote HTTP has no native randomUUID; commands must still get secure IDs.
    await page.addInitScript(() => {
      Object.defineProperty(Crypto.prototype, "randomUUID", {
        configurable: true,
        value: undefined,
      });
    });
    await settings(page);
    const source = await page.request
      .post("/rpc", {
        data: { jsonrpc: "2.0", id: 1, method: "query.workspace", params: {} },
      })
      .then((response) => response.json());
    await page.evaluate(() =>
      window.history.replaceState(
        null,
        "",
        "?tab=settings&project_id=old-kernel-project&feature_id=old-feature",
      ),
    );
    await page
      .getByLabel("内核地址", { exact: true })
      .fill(target.origin + "/");
    await page.getByRole("button", { name: "连接内核", exact: true }).click();
    await expect(page).toHaveURL(target.origin + "/?tab=settings");
    await expect(
      page.getByRole("region", { name: "内核认证", exact: true }),
    ).toBeVisible();
    await expect(page.getByLabel("连接密钥", { exact: true })).toHaveAttribute(
      "type",
      "password",
    );
    await page
      .getByLabel("连接密钥", { exact: true })
      .fill("incorrect-fixture-key");
    await page.getByRole("button", { name: "验证并连接", exact: true }).click();
    await expect(page.getByRole("alert")).toContainText("连接密钥不正确");
    await page.getByLabel("连接密钥", { exact: true }).fill(target.key);
    await page.getByRole("button", { name: "验证并连接", exact: true }).click();
    await expect(
      page.getByRole("heading", { name: "数据与运行", exact: true }),
    ).toBeVisible();
    const project = await page.evaluate(async () => {
      const id = crypto.randomUUID();
      const response = await fetch("/rpc", {
        method: "POST",
        headers: { "Content-Type": "application/json" },
        body: JSON.stringify({
          jsonrpc: "2.0",
          id: 1,
          method: "project.create",
          params: {
            command_id: id,
            payload: { title: "Only in target kernel" },
          },
        }),
      });
      return { id, response: await response.json() };
    });
    expect(project.id).toMatch(
      /^[0-9a-f]{8}-[0-9a-f]{4}-4[0-9a-f]{3}-[89ab][0-9a-f]{3}-[0-9a-f]{12}$/,
    );
    expect(project.response.error).toBeUndefined();
    await page.reload();
    await expect(
      page.getByRole("heading", { name: "数据与运行", exact: true }),
    ).toBeVisible();
    await expect(page.getByLabel("连接密钥", { exact: true })).toHaveCount(0);
    const storage = await page.evaluate(() =>
      JSON.stringify({
        local: { ...localStorage },
        session: { ...sessionStorage },
      }),
    );
    expect(storage).not.toContain(target.key);
    expect(page.url()).not.toContain(target.key);
    await page.screenshot({
      path: testInfo.outputPath("kernel-connection-settings.png"),
      fullPage: true,
    });
    // Keep a genuine workspace response in flight across shutdown. A late
    // refresh must never restore the stopped kernel or replace its reconnect form.
    let snapshotReady = false;
    let captured = false;
    let snapshotDelivered = () => {};
    const held = new Promise<void>((resolve) => {
      releaseSnapshot = resolve;
    });
    const delivered = new Promise<void>((resolve) => {
      snapshotDelivered = resolve;
    });
    await page.route(target.origin + "/rpc", async (route) => {
      if (
        !captured &&
        route.request().postDataJSON().method === "query.workspace"
      ) {
        captured = true;
        const response = await route.fetch();
        snapshotReady = true;
        await held;
        await route.fulfill({ response });
        snapshotDelivered();
      } else await route.continue();
    });
    const change = await page.request
      .post(target.origin + "/rpc", {
        data: {
          jsonrpc: "2.0",
          id: 2,
          method: "project.update",
          params: {
            project_id: project.response.result.id,
            command_id: crypto.randomUUID(),
            target: project.response.result.id,
            expected_revision: 1,
            payload: { concurrency: 3 },
          },
        },
      })
      .then((response) => response.json());
    expect(change.error).toBeUndefined();
    await expect.poll(() => snapshotReady).toBe(true);
    await page.getByRole("button", { name: "退出", exact: true }).click();
    await page
      .getByRole("dialog")
      .getByRole("button", { name: "确认", exact: true })
      .click();
    await expect(
      page.getByText("内核已退出，请连接其他内核或重新启动后连接。", {
        exact: true,
      }),
    ).toBeVisible();
    releaseSnapshot();
    await delivered;
    await page.evaluate(
      () =>
        new Promise<void>((resolve) =>
          requestAnimationFrame(() => requestAnimationFrame(() => resolve())),
        ),
    );
    await expect(
      page.getByRole("heading", { name: "数据与运行", exact: true }),
    ).toHaveCount(0);
    await expect(
      page.getByText("内核已退出，请连接其他内核或重新启动后连接。", {
        exact: true,
      }),
    ).toBeVisible();
    await page.getByLabel("内核地址", { exact: true }).fill(baseURL!);
    await page.getByRole("button", { name: "连接内核", exact: true }).click();
    await expect(page).toHaveURL(baseURL + "/?tab=settings");
    await expect(
      page.getByRole("heading", { name: "数据与运行", exact: true }),
    ).toBeVisible();
    const restored = await page.request
      .post("/rpc", {
        data: { jsonrpc: "2.0", id: 1, method: "query.workspace", params: {} },
      })
      .then((response) => response.json());
    expect(restored.result.total_projects).toBe(source.result.total_projects);
    expect(
      restored.result.projects.some(
        (project: { title: string }) =>
          project.title === "Only in target kernel",
      ),
    ).toBe(false);
  } finally {
    releaseSnapshot();
    await target.stop();
  }
});

test("内核地址校验与连接失败保留当前设置及重试入口", async ({ page }) => {
  await settings(page);
  const original = page.url();
  for (const address of [
    "file:///tmp/kernel",
    "http://user:secret@127.0.0.1:4317",
    "http://127.0.0.1:4317/path",
  ]) {
    await page.getByLabel("内核地址", { exact: true }).fill(address);
    await page.getByRole("button", { name: "连接内核", exact: true }).click();
    await expect(page.getByRole("region", { name: "内核连接" }).getByRole("alert")).toContainText("内核地址仅包含");
    expect(page.url()).toBe(original);
  }
  const target = "http://127.0.0.1:54329";
  await page.route(target + "/connection", (route) =>
    route.abort("connectionrefused"),
  );
  await page.getByLabel("内核地址", { exact: true }).fill(target);
  await page.getByRole("button", { name: "连接内核", exact: true }).click();
  await expect(page.getByRole("region", { name: "内核连接" }).getByRole("alert")).toContainText("无法连接内核");
  expect(page.url()).toBe(original);
  await expect(
    page.getByRole("heading", { name: "全局并发额度", exact: true }),
  ).toBeVisible();
  await page.unroute(target + "/connection");
  await page.route(target + "/connection", (route) =>
    route.fulfill({
      json: { product: "devdesk", protocol_version: 2 },
      headers: { "Access-Control-Allow-Origin": "*" },
    }),
  );
  await page.getByRole("button", { name: "连接内核", exact: true }).click();
  await expect(page.getByRole("region", { name: "内核连接" }).getByRole("alert")).toContainText("协议版本不兼容");
  expect(page.url()).toBe(original);
  await expect(
    page.getByRole("button", { name: "连接内核", exact: true }),
  ).toBeEnabled();
});

test("内核首次连接失败时仍提供连接入口", async ({ page }) => {
  await page.route("**/capabilities", (route) =>
    route.abort("connectionrefused"),
  );
  await page.goto("/");
  await expect(
    page.getByText("当前内核无法连接，可重试或连接其他内核。", { exact: true }),
  ).toBeVisible();
  await expect(page.getByLabel("内核地址", { exact: true })).toBeVisible();
  await expect(
    page.getByRole("button", { name: "连接内核", exact: true }),
  ).toBeEnabled();
});

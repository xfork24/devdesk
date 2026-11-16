import { test, expect, type Page } from "@playwright/test";
import { mkdtempSync } from "node:fs";
import { tmpdir } from "node:os";
import { join } from "node:path";

async function rpc(page: Page, method: string, params: object = {}) {
  const response = await page.request.post("/rpc", {
    data: { jsonrpc: "2.0", id: 1, method, params },
  });
  const value = await response.json();
  expect(value.error).toBeUndefined();
  return value.result;
}

test("两个前端独立切换和编辑项目，刷新继续使用标签页 URL", async ({
  page,
  browser,
}) => {
  await page.goto("/");
  if ((await (await page.request.get("/capabilities")).json()).setup_required) {
    await page
      .getByLabel("数据目录")
      .fill(mkdtempSync(join(tmpdir(), "devdesk-project-isolation-")));
    await page.getByRole("button", { name: "初始化 / 打开数据集" }).click();
  }
  await expect(
    page.getByRole("button", { name: "设置", exact: true }),
  ).toBeVisible();
  const projects = await Promise.all(
    ["A", "B"].map((name) =>
      rpc(page, "project.create", {
        command_id: crypto.randomUUID(),
        payload: {
          title: `独立项目 ${name}`,
          repository: `fixture/isolated-${name}`,
        },
      }),
    ),
  );
  const [a, b] = projects.map((project) => project.id as string);
  const other = await browser.newContext();
  const opens: string[] = [];
  page.on("request", (request) => {
    if (request.url().endsWith("/rpc")) {
      const method = request.postDataJSON()?.method;
      if (["project.open", "project.open_github"].includes(method))
        opens.push(method);
    }
  });
  try {
    const second = await other.newPage();
    await Promise.all([
      page.goto(`/?tab=settings&project_id=${a}`),
      second.goto(`/?tab=settings&project_id=${b}`),
    ]);
    await expect(
      page.getByRole("heading", { name: "独立项目 A · Project 设置" }),
    ).toBeVisible();
    await expect(
      second.getByRole("heading", { name: "独立项目 B · Project 设置" }),
    ).toBeVisible();
    for (const id of [b, a, b, a]) {
      await page.getByLabel("当前项目", { exact: true }).selectOption(id);
      await expect(page).toHaveURL(new RegExp(`project_id=${id}`));
      await expect(second.getByLabel("当前项目", { exact: true })).toHaveValue(
        b,
      );
      await expect(second).toHaveURL(new RegExp(`project_id=${b}`));
    }
    await expect(
      page.getByRole("heading", { name: "独立项目 A · Project 设置" }),
    ).toBeVisible();
    for (const [front, name] of [
      [page, "A"],
      [second, "B"],
    ] as const) {
      const row = front.locator(".setting-row").filter({
        has: front.getByRole("heading", {
          name: `独立项目 ${name} · Project 设置`,
          exact: true,
        }),
      });
      await row.getByRole("button", { name: "编辑", exact: true }).click();
      await front
        .getByRole("dialog", { name: "编辑 Project" })
        .getByLabel("名称", { exact: true })
        .fill(`已编辑 ${name}`);
    }
    await Promise.all(
      [page, second].map(async (front) => {
        const dialog = front.getByRole("dialog", { name: "编辑 Project" });
        await dialog.getByRole("button", { name: "确认", exact: true }).click();
        await expect(dialog).toHaveCount(0);
      }),
    );
    expect(
      (await rpc(page, "query.ui_bootstrap", { project_id: a })).active_project
        .title,
    ).toBe("已编辑 A");
    expect(
      (await rpc(page, "query.ui_bootstrap", { project_id: b })).active_project
        .title,
    ).toBe("已编辑 B");
    await page.reload();
    await expect(page.getByLabel("当前项目", { exact: true })).toHaveValue(a);
    await expect(second.getByLabel("当前项目", { exact: true })).toHaveValue(b);
    const blank = await other.newPage();
    await blank.goto("/?tab=settings");
    await expect(blank.getByLabel("当前项目", { exact: true })).toHaveValue("");
    await expect(
      blank
        .getByLabel("当前项目", { exact: true })
        .locator(`option[value="${a}"]`),
    ).toHaveCount(1);
    await expect(
      blank
        .getByLabel("当前项目", { exact: true })
        .locator(`option[value="${b}"]`),
    ).toHaveCount(1);
    await expect(
      blank.getByRole("heading", { name: /· Project 设置$/ }),
    ).toHaveCount(0);
    await blank.close();
    expect(opens).toEqual([]);
  } finally {
    await other.close();
  }
});

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

test("两个前端独立显示项目仓库位置，刷新继续使用标签页 URL", async ({
  page,
  browser,
}) => {
  await page.goto("/");
  if ((await (await page.request.get("/capabilities")).json()).setup_required) {
    await page
      .getByLabel("数据目录")
      .fill(mkdtempSync(join(tmpdir(), "devdesk-project-isolation-")));
    await page.getByRole("button", { name: "初始化 / 打开数据集" }).click();
    await expect(page.getByLabel("数据目录")).toHaveCount(0);
  }
  await expect(
    page.getByRole("button", { name: "设置", exact: true }),
  ).toBeVisible();
  const paths = ["A", "B"].map((name) =>
    join(tmpdir(), `devdesk-isolated-${name}`, "repository"),
  );
  const projects = await Promise.all(
    ["A", "B"].map((name, index) =>
      rpc(page, "project.create", {
        command_id: crypto.randomUUID(),
        payload: {
          title: `独立项目 ${name}`,
          repository: `fixture/isolated-${name}`,
          workspace: paths[index],
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
  const dataPanel = (front: Page) =>
    front.locator("section").filter({
      has: front.getByRole("heading", { name: "数据与运行", exact: true }),
    });
  const repositoryRow = (front: Page) =>
    dataPanel(front).locator(".setting-row").last();
  try {
    const second = await other.newPage();
    await Promise.all([
      page.goto(`/?tab=settings&project_id=${a}`),
      second.goto(`/?tab=settings&project_id=${b}`),
    ]);
    for (const [front, path] of [
      [page, paths[0]],
      [second, paths[1]],
    ] as const) {
      const row = repositoryRow(front);
      await expect(
        row.getByRole("heading", { name: "repository 位置", exact: true }),
      ).toBeVisible();
      await expect(row.locator("code")).toHaveText(path);
      await expect(row.getByRole("button")).toHaveCount(0);
      await expect(row).not.toContainText("并发");
    }
    for (const id of [b, a, b, a]) {
      await page.getByLabel("当前项目", { exact: true }).selectOption(id);
      await expect(page).toHaveURL(new RegExp(`project_id=${id}`));
      await expect(repositoryRow(page).locator("code")).toHaveText(
        id === a ? paths[0] : paths[1],
      );
      await expect(second.getByLabel("当前项目", { exact: true })).toHaveValue(
        b,
      );
      await expect(second).toHaveURL(new RegExp(`project_id=${b}`));
      await expect(repositoryRow(second).locator("code")).toHaveText(paths[1]);
    }
    await page.reload();
    await expect(page.getByLabel("当前项目", { exact: true })).toHaveValue(a);
    await expect(repositoryRow(page).locator("code")).toHaveText(paths[0]);
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
      blank.getByRole("heading", { name: "repository 位置", exact: true }),
    ).toHaveCount(0);
    await blank.close();
    expect(opens).toEqual([]);
  } finally {
    await other.close();
  }
});

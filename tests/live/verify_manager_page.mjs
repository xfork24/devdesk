// Read-only verification of a real manager request and its exact navigation targets.
import { readFileSync, writeFileSync } from "node:fs";
import { createRequire } from "node:module";
import { dirname, join } from "node:path";
import assert from "node:assert/strict";

const require = createRequire(new URL("../../web/package.json", import.meta.url));
const { chromium, expect: playwrightExpect } = require("@playwright/test");
const expect = playwrightExpect.configure({ timeout: 15000 });
const requestFile = process.argv[2];
const { request, manager_url: managerUrl } = JSON.parse(readFileSync(requestFile, "utf8"));
const interaction = request.data.request.interaction;
const scopeOwner = new URL(managerUrl).searchParams.get("feature_id") || new URL(managerUrl).searchParams.get("project_id");
const browser = await chromium.launch({ headless: true, channel: "chromium" });
try {
  const context = await browser.newContext({ viewport: { width: 1440, height: 1100 } });
  const page = await context.newPage();
  const errors = [];
  const reads = [];
  page.on("response", async (response) => {
    if (!response.url().endsWith("/rpc") && !response.url().endsWith("/capabilities")) return;
    const call = response.request().postDataJSON();
    const result = await response.json().catch(() => ({}));
    reads.push({ method: call?.method || "capabilities", params: call?.params,
      status: response.status(), error: result.error, active_project_id: result.active_project_id,
      timing: response.request().timing() });
  });
  page.on("pageerror", (error) => errors.push(error.message));
  await page.goto(managerUrl);
  const manager = page.getByRole("region", { name: "项目经理协作", exact: true });
  const card = manager.getByRole("article", { name: interaction.title, exact: true }).first();
  try {
    await expect(card).toBeVisible();
  } catch (error) {
    const prefix = join(dirname(requestFile), request.id);
    writeFileSync(prefix + "-page-failure.json", JSON.stringify({
      manager_url: managerUrl, url: page.url(), errors, reads, body: await page.locator("body").innerText(),
    }, null, 2) + "\n");
    await page.screenshot({ path: prefix + "-page-failure.png", fullPage: true });
    throw error;
  }
  for (const text of ["需要你做什么", "相关材料", "项目经理已做的工作", "初审与验证结果"])
    await expect(card).toContainText(text);
  await expect(manager.getByText("调度与经理管理", { exact: true })).toHaveCount(0);
  await expect(manager.getByText("查看确切请求", { exact: true })).toHaveCount(0);
  const links = await card.getByRole("link").evaluateAll((nodes) =>
    nodes.map((node) => ({ text: node.textContent, href: node.href, target: node.getAttribute("href") })));
  // Follow the actual SPA links and browser Back. Repeatedly abandoning a new
  // tab leaves its already-started workspace read running on the local Host.
  const openLink = async (link) => {
    await card.locator(`a[href=${JSON.stringify(link.target)}]`).first().click();
  };
  const returnToManager = async () => {
    await page.goBack();
    await expect(card).toBeVisible();
  };
  const verified = [];
  for (const material of [...new Map([...interaction.materials, ...interaction.evidence]
    .filter((reference) => reference.output_id)
    .map((reference) => [reference.output_id, reference])).values()]) {
    const link = links.find((entry) => {
      const parameters = new URL(entry.href).searchParams;
      return parameters.get("output_id") === material.output_id && parameters.get("detail") === "documents";
    });
    assert(link, "Missing exact material link: " + material.label);
    await openLink(link);
    const body = page.locator("#material-" + material.output_id);
    await expect(body.getByRole("heading").first()).toBeVisible();
    await expect(body.getByRole("alert")).toHaveCount(0);
    verified.push({ kind: "material", output_id: material.output_id, href: link.href });
    await returnToManager();
  }
  const references = [...new Map([...interaction.materials, ...interaction.evidence]
    .map((reference) => [reference.execution_id, reference])).values()];
  for (const evidence of references) {
    for (const [detail, method] of [["records", "query.workflow_executions"], ["context", "query.workflow_contexts"]]) {
      if (detail === "context" && !evidence.session_id) continue;
      const link = links.find((entry) => {
        const parameters = new URL(entry.href).searchParams;
        return parameters.get("execution_id") === evidence.execution_id && parameters.get("detail") === detail;
      });
      assert(link, "Missing exact " + detail + " link");
      const parameters = new URL(link.href).searchParams;
      const linkedOwner = parameters.get("feature_id") || parameters.get("project_id") || scopeOwner;
      const observed = page.waitForResponse((response) => {
        if (!response.url().endsWith("/rpc")) return false;
        const call = response.request().postDataJSON();
        return call?.method === method && call.params?.owner_id === linkedOwner &&
          call.params?.execution_id === evidence.execution_id;
      }).then(async (response) => ({ request: response.request().postDataJSON(), value: await response.json() }));
      await openLink(link);
      const observedResponse = await observed;
      const response = observedResponse.value;
      assert(!response.error, JSON.stringify(response.error));
      if (detail === "context") {
        assert(response.result.agents.some((agent) => agent.entries.some((entry) =>
          entry.execution_id === evidence.execution_id && entry.turn_id === evidence.turn_id)), JSON.stringify({evidence, call: observedResponse.request, response}).slice(0, 6000));
      } else {
        assert.equal(response.result.record.id, evidence.execution_id);
      }
      verified.push({ kind: detail, execution_id: evidence.execution_id, href: link.href });
      await returnToManager();
    }
  }
  assert.deepEqual(errors, []);
  const prefix = join(dirname(requestFile), request.id);
  await card.screenshot({ path: prefix + "-manager.png" });
  await page.setViewportSize({ width: 390, height: 844 });
  await expect(card).toBeVisible();
  assert(await page.evaluate(() => document.documentElement.scrollWidth <= innerWidth));
  await card.screenshot({ path: prefix + "-manager-mobile.png" });
  writeFileSync(prefix + "-page-validation.json", JSON.stringify({
    passed: true, request_id: request.id, manager_url: managerUrl, links: verified, errors,
    summary: await card.innerText(),
  }, null, 2) + "\n");
  console.log("PASS real manager page", request.id, verified.length, "exact links");
  if (process.argv.includes("--approve")) {
    const reasonIndex = process.argv.indexOf("--reason");
    assert(reasonIndex >= 0 && process.argv[reasonIndex + 1]?.trim(), "Approval requires an actual review opinion");
    const controls = [];
    page.on("request", (request) => {
      if (!request.url().endsWith("/rpc")) return;
      const call = request.postDataJSON();
      if (call && !call.method.startsWith("query.") && call.method !== "runtime.preparation_status") controls.push(call);
    });
    await page.setViewportSize({ width: 1440, height: 1100 });
    await card.getByLabel("意见或修改要求").fill(process.argv[reasonIndex + 1]);
    const reply = page.waitForResponse((r) => r.url().endsWith("/rpc") && r.request().postDataJSON()?.method === "workflow.decision.resolve");
    await card.getByRole("button", { name: "确认：" + interaction.title, exact: true }).click();
    const response = await (await reply).json();
    assert.equal(response.error, undefined, JSON.stringify(response.error));
    assert.deepEqual(controls.map((c) => c.method), ["workflow.decision.resolve"], "The operator may only sign the human decision");
    writeFileSync(prefix + "-human-signature.json", JSON.stringify({ request_id: request.id, opinion: process.argv[reasonIndex + 1], controls, response }, null, 2) + "\n");
    console.log("SIGNED Human decision only", request.id);
  }
} finally {
  await browser.close();
}

# Instructions

- Following Playwright test failed.
- Explain why, be concise, respect Playwright best practices.
- Provide a snippet of code with the fix, if possible.

# Test info

- Name: workbench.spec.ts >> 真实接入设置偏好、密钥表单与浏览器 OAuth 链接
- Location: tests/workbench.spec.ts:540:1

# Error details

```
Error: apiResponse.json: Response has been disposed
```

# Page snapshot

```yaml
- generic [ref=f1e3]:
  - complementary [ref=f1e4]:
    - strong [ref=f1e10]: DevDesk
    - generic [ref=f1e11]: 软件工程工作台 v0.2
    - generic [ref=f1e12]:
      - text: 当前项目
      - combobox "当前项目" [ref=f1e13]:
        - option "DevDesk E2E"
        - option "异常回路"
        - option "分页验证"
        - option "清理契约"
        - option "隔离项目 A"
        - option "隔离项目 B" [selected]
      - button "创建项目" [ref=f1e14] [cursor=pointer]
    - navigation [ref=f1e16]:
      - button "工作总览" [ref=f1e17] [cursor=pointer]
      - button "角色讨论" [ref=f1e23] [cursor=pointer]
      - button "执行与交付" [ref=f1e27] [cursor=pointer]
      - button "文档与上下文" [ref=f1e32] [cursor=pointer]
      - button "规范与身份" [ref=f1e36] [cursor=pointer]
      - button "历史记录" [ref=f1e39] [cursor=pointer]
      - button "设置" [ref=f1e44] [cursor=pointer]
    - generic [ref=f1e50]:
      - strong [ref=f1e51]: Simulation mode
      - paragraph [ref=f1e52]: 工程流程与角色讨论
  - generic [ref=f1e53]:
    - banner [ref=f1e54]:
      - generic [ref=f1e55]:
        - generic [ref=f1e58]: 隔离项目 B
        - generic [ref=f1e61]: 总览
      - generic [ref=f1e62]:
        - generic [ref=f1e63]: 已连接
        - generic [ref=f1e67]: 模拟项目
    - main [ref=f1e68]:
      - generic [ref=f1e70]:
        - text: ENGINEERING WORKSPACE
        - heading "设置" [level=1] [ref=f1e71]
        - paragraph [ref=f1e72]: 本地数据与执行能力。
      - generic [ref=f1e73]:
        - generic [ref=f1e74]:
          - heading "数据与运行" [level=2] [ref=f1e75]
          - generic [ref=f1e76]: simulation
        - generic [ref=f1e77]:
          - generic [ref=f1e78]:
            - heading "本地 data 目录" [level=3] [ref=f1e79]
            - code [ref=f1e80]: /tmp/devdesk-web-CJNaHq
            - paragraph [ref=f1e81]: 全局配置与各项目分别保存；打开项目即可切换。更改 data 路径需退出后以 --data 新路径重启。
          - button "更改路径说明" [ref=f1e82] [cursor=pointer]
        - generic [ref=f1e84]:
          - heading "全局配置数据库" [level=3] [ref=f1e85]
          - code [ref=f1e86]: /tmp/devdesk-web-CJNaHq/global.sqlite3
          - paragraph [ref=f1e87]: 并发配置、共享定义与项目索引。
        - generic [ref=f1e89]:
          - heading "当前项目数据库" [level=3] [ref=f1e90]
          - code [ref=f1e91]: /tmp/devdesk-web-CJNaHq/projects/1bc35be3-ef97-4c0d-bab0-aa42732c79bc/engineering.sqlite3
          - paragraph [ref=f1e92]: 工程状态、执行历史和产物归属仅限当前项目。
        - generic [ref=f1e93]:
          - generic [ref=f1e95]:
            - heading "GitHub 仓库接入" [level=2] [ref=f1e96]
            - paragraph [ref=f1e97]: 使用独立项目登记仓库。宿主负责 Git/PR，Agent 容器仅访问所属 worktree。当前接入进度按操作回执显示。
          - generic [ref=f1e98]:
            - generic [ref=f1e99]:
              - text: GitHub owner/repo
              - textbox "GitHub owner/repo" [ref=f1e100]
            - generic [ref=f1e101]:
              - text: Mainline
              - textbox "GitHub Mainline" [ref=f1e102]: main
            - generic [ref=f1e103]:
              - button "检查 GitHub 规则与权限" [disabled] [ref=f1e104]
              - button "登记并克隆仓库" [disabled] [ref=f1e105]
        - generic [ref=f1e107]:
          - heading "项目与角色运行时偏好" [level=3] [ref=f1e108]
          - paragraph [ref=f1e109]: 隔离项目 B · 显式会话选择优先，其次角色、项目、全局。相同版本共用安装，每个范围保留独立会话。
          - generic [ref=f1e110]:
            - generic [ref=f1e111]:
              - text: 配置范围
              - combobox "运行时偏好范围" [ref=f1e112]:
                - option "项目默认" [selected]
                - option "需求分析师"
                - option "架构师"
                - option "开发"
                - option "测试"
                - option "评审"
            - generic [ref=f1e113]:
              - text: Harness
              - combobox "项目角色 Harness" [ref=f1e114]:
                - option "继承上层偏好" [selected]
                - option "Codex"
                - option "DeepSeek Harness"
            - generic [ref=f1e115]:
              - text: 共享安装
              - combobox "项目角色安装" [disabled] [ref=f1e116]:
                - option "自动解析唯一兼容安装" [selected]
            - generic [ref=f1e117]:
              - text: 模型 ID
              - textbox "项目角色模型" [disabled] [ref=f1e118]:
                - /placeholder: 留空使用供应商默认模型
            - button "保存偏好" [ref=f1e119] [cursor=pointer]
          - status
        - generic [ref=f1e120]:
          - generic [ref=f1e121]:
            - heading "默认 Agent Harness" [level=3] [ref=f1e122]
            - paragraph [ref=f1e123]: 后续真实会话的偏好；已有执行继续使用冻结绑定。安装登记与真实调用验收分别记录。
          - combobox "默认 Agent Harness" [disabled] [ref=f1e124]:
            - option "Codex"
            - option "DeepSeek Harness" [selected]
        - generic [ref=f1e126]:
          - heading "Codex · ChatGPT OAuth" [level=3] [ref=f1e127]
          - paragraph [ref=f1e128]: 通过官方网页授权；本机认证使用独立凭据目录。容器会话的登录能力单独核验。
          - generic [ref=f1e129]:
            - button "登记本机 Codex" [disabled] [ref=f1e130]
            - button "网页登录" [disabled] [ref=f1e131]
            - button "检查登录状态" [disabled] [ref=f1e132]
            - button "取消登录" [disabled] [ref=f1e133]
          - paragraph [ref=f1e134]:
            - link "打开官方授权网页" [ref=f1e135] [cursor=pointer]:
              - /url: https://auth.openai.com/authorize?fixture=true
            - text: ，完成后检查登录状态。
        - generic [ref=f1e137]:
          - heading "DeepSeek 官方 API key" [level=3] [ref=f1e138]
          - paragraph [ref=f1e139]: 已配置
          - paragraph [ref=f1e140]: 保存在本机系统凭据库，不在历史或普通查询中显示。
          - button "检查凭据访问" [disabled] [ref=f1e142]
          - generic [ref=f1e143]:
            - generic [ref=f1e144]: 配置或更换 API key
            - textbox "配置或更换 API key" [disabled] [ref=f1e145]
            - button "保存密钥" [disabled] [ref=f1e146]
        - generic [ref=f1e148]:
          - heading "共享运行时安装" [level=3] [ref=f1e149]
          - generic [ref=f1e150]:
            - button "登记 Codex Linux 镜像" [disabled] [ref=f1e151]
            - button "登记 DSH Linux 镜像" [disabled] [ref=f1e152]
          - paragraph [ref=f1e153]:
            - text: codex · fixture · macos/aarch64
            - code [ref=f1e154]: /fixture/codex
        - status
        - generic [ref=f1e155]:
          - generic [ref=f1e156]:
            - heading "全局并发额度" [level=3] [ref=f1e157]
            - paragraph [ref=f1e158]: 3 个 Run · 单项目默认 2 个
          - button "调整" [ref=f1e159] [cursor=pointer]
        - generic [ref=f1e160]:
          - generic [ref=f1e161]:
            - heading "隔离项目 B · Project 设置" [level=3] [ref=f1e162]
            - paragraph [ref=f1e163]: "Workspace: 未设置 · 项目并发 2"
          - generic [ref=f1e164]:
            - button "默认绑定" [ref=f1e165] [cursor=pointer]
            - button "编辑" [ref=f1e166] [cursor=pointer]
        - generic [ref=f1e167]:
          - generic [ref=f1e168]:
            - heading "退出内核" [level=3] [ref=f1e169]
            - paragraph [ref=f1e170]: 未确认的运行在下次启动后标记中断，可显式重试。
          - button "退出" [ref=f1e171] [cursor=pointer]
      - generic [ref=f1e172]:
        - generic [ref=f1e173]:
          - heading "数据生命周期" [level=2] [ref=f1e174]
          - generic [ref=f1e175]: 仅交互预览
        - generic [ref=f1e176]:
          - generic [ref=f1e177]:
            - heading "Archive" [level=3] [ref=f1e178]
            - paragraph [ref=f1e179]: 本轮预览操作范围，不执行导出、删除、恢复或搬迁。
          - button "预览" [ref=f1e180] [cursor=pointer]
        - generic [ref=f1e181]:
          - generic [ref=f1e182]:
            - heading "data 迁移" [level=3] [ref=f1e183]
            - paragraph [ref=f1e184]: 本轮预览操作范围，不执行导出、删除、恢复或搬迁。
          - button "预览" [ref=f1e185] [cursor=pointer]
      - generic [ref=f1e186]: Web UI 通过本地 Rust Host 运行。真实工程需配置 Harness、Docker 和 GitHub；平台验收记录见文档。
      - generic [ref=f1e187]:
        - text: DevDesk 0.2
        - generic [ref=f1e188]: 本地工程内核 · 持久化历史 · origin=simulated
```

# Test source

```ts
  449 |   await confirm(page);
  450 |   const recovery = page.getByRole("region", { name: "外部操作核对" });
  451 |   await recovery.getByRole("button", { name: "重试资源清理" }).first().click();
  452 |   const retry = recovery
  453 |     .locator(".document-row")
  454 |     .filter({ hasText: "retry_of=" })
  455 |     .first();
  456 |   await expect(retry).toContainText("applied");
  457 |   await retry.getByRole("button", { name: "查看回执" }).click();
  458 |   await expect(page.getByRole("dialog")).toContainText('"status": "released"');
  459 |   await expect(page.getByRole("dialog")).toContainText('"adapter"');
  460 |   await page.getByRole("button", { name: "关闭", exact: true }).click();
  461 | });
  462 | 
  463 | test("打开项目切换独立数据库，重载保持选择和共享配置", async ({ page }) => {
  464 |   const errors: string[] = [];
  465 |   page.on("pageerror", (e) => errors.push(String(e)));
  466 |   await ensureSetup(page);
  467 |   await page.getByRole("button", { name: "新项目", exact: true }).click();
  468 |   await page.getByLabel("项目名称").fill("隔离项目 A");
  469 |   await confirm(page);
  470 |   const selector = page.getByLabel("当前项目", { exact: true });
  471 |   await expect(selector.locator("option:checked")).toHaveText("隔离项目 A");
  472 |   const a = await selector.inputValue();
  473 |   await page.getByRole("button", { name: "新 Feature", exact: true }).click();
  474 |   await page.getByLabel("Feature 名称").fill("只属于 A 的 Feature");
  475 |   await confirm(page);
  476 |   await page.getByRole("button", { name: "执行与交付", exact: true }).click();
  477 |   await expect(
  478 |     page.getByLabel("当前 Feature", { exact: true }).locator("option:checked"),
  479 |   ).toHaveText("只属于 A 的 Feature");
  480 |   await page.getByRole("button", { name: "工作总览", exact: true }).click();
  481 |   await page.getByRole("button", { name: "新项目", exact: true }).click();
  482 |   await page.getByLabel("项目名称").fill("隔离项目 B");
  483 |   await confirm(page);
  484 |   await expect(selector.locator("option:checked")).toHaveText("隔离项目 B");
  485 |   const b = await selector.inputValue();
  486 |   await page.getByRole("button", { name: "执行与交付", exact: true }).click();
  487 |   await expect(
  488 |     page.getByLabel("当前 Feature", { exact: true }).locator("option"),
  489 |   ).not.toContainText(["只属于 A 的 Feature"]);
  490 |   await page.getByRole("button", { name: "工作总览", exact: true }).click();
  491 |   await page.getByRole("button", { name: "新 Feature", exact: true }).click();
  492 |   await page.getByLabel("Feature 名称").fill("只属于 B 的 Feature");
  493 |   await confirm(page);
  494 |   await page.getByRole("button", { name: "设置", exact: true }).click();
  495 |   await expect(
  496 |     page
  497 |       .locator(".setting-row")
  498 |       .filter({
  499 |         has: page.getByRole("heading", { name: "当前项目数据库", exact: true }),
  500 |       }),
  501 |   ).toContainText(`/projects/${b}/engineering.sqlite3`);
  502 |   await expect(
  503 |     page
  504 |       .locator(".setting-row")
  505 |       .filter({
  506 |         has: page.getByRole("heading", { name: "全局配置数据库", exact: true }),
  507 |       }),
  508 |   ).toContainText("global.sqlite3");
  509 |   await page.getByRole("button", { name: "调整", exact: true }).click();
  510 |   await page.getByLabel("并发额度 1—32").fill("3");
  511 |   await confirm(page);
  512 |   await selector.selectOption(a);
  513 |   await expect(
  514 |     page
  515 |       .locator(".setting-row")
  516 |       .filter({
  517 |         has: page.getByRole("heading", { name: "当前项目数据库", exact: true }),
  518 |       }),
  519 |   ).toContainText(`/projects/${a}/engineering.sqlite3`);
  520 |   await expect(
  521 |     page.getByText("3 个 Run · 单项目默认 2 个", { exact: true }),
  522 |   ).toBeVisible();
  523 |   await page.getByRole("button", { name: "执行与交付", exact: true }).click();
  524 |   await expect(
  525 |     page.getByLabel("当前 Feature", { exact: true }).locator("option"),
  526 |   ).toContainText(["选择 Feature", "只属于 A 的 Feature"]);
  527 |   await expect(
  528 |     page.getByLabel("当前 Feature", { exact: true }).locator("option"),
  529 |   ).not.toContainText(["只属于 B 的 Feature"]);
  530 |   await page.reload();
  531 |   await expect(selector.locator("option:checked")).toHaveText("隔离项目 A");
  532 |   await page.getByRole("button", { name: "执行与交付", exact: true }).click();
  533 |   await selector.selectOption(b);
  534 |   await expect(
  535 |     page.getByLabel("当前 Feature", { exact: true }).locator("option"),
  536 |   ).toContainText(["选择 Feature", "只属于 B 的 Feature"]);
  537 |   expect(errors).toEqual([]);
  538 | });
  539 | 
  540 | test("真实接入设置偏好、密钥表单与浏览器 OAuth 链接", async ({
  541 |   page,
  542 |   context,
  543 | }) => {
  544 |   await ensureSetup(page);
  545 |   await page.route("**/rpc", async (route) => {
  546 |     const request = route.request().postDataJSON();
  547 |     if (request.method === "query.workspace") {
  548 |       const response = await route.fetch();
> 549 |       const body = await response.json();
      |                                   ^ Error: apiResponse.json: Response has been disposed
  550 |       body.result.runtime_installations = [
  551 |         {
  552 |           id: "browser-fixture",
  553 |           key: {
  554 |             provider: "codex",
  555 |             version: "fixture",
  556 |             os: "macos",
  557 |             arch: "aarch64",
  558 |           },
  559 |           artifact: { canonical_path: "/fixture/codex" },
  560 |         },
  561 |       ];
  562 |       await route.fulfill({ json: body });
  563 |     } else if (request.method === "codex.auth.start") {
  564 |       await route.fulfill({
  565 |         json: {
  566 |           jsonrpc: "2.0",
  567 |           id: request.id,
  568 |           result: {
  569 |             loginId: "fixture-login",
  570 |             authUrl: "https://auth.openai.com/authorize?fixture=true",
  571 |           },
  572 |         },
  573 |       });
  574 |     } else if (request.method === "credentials.status") {
  575 |       await route.fulfill({
  576 |         json: {
  577 |           jsonrpc: "2.0",
  578 |           id: request.id,
  579 |           result: { configured: false, storage: "test_vault" },
  580 |         },
  581 |       });
  582 |     } else if (request.method === "credentials.configure") {
  583 |       expect(request.params.provider).toBe("dsh");
  584 |       await route.fulfill({
  585 |         json: {
  586 |           jsonrpc: "2.0",
  587 |           id: request.id,
  588 |           result: { configured: true, storage: "test_vault" },
  589 |         },
  590 |       });
  591 |     } else await route.continue();
  592 |   });
  593 |   await page.getByRole("button", { name: "设置", exact: true }).click();
  594 |   await page.getByLabel("默认 Agent Harness").selectOption("dsh");
  595 |   await expect(page.getByLabel("默认 Agent Harness")).toHaveValue("dsh");
  596 |   await expect(page.getByLabel("默认 Agent Harness")).toBeEnabled();
  597 |   await page.reload();
  598 |   await page.getByRole("button", { name: "设置", exact: true }).click();
  599 |   await expect(page.getByLabel("默认 Agent Harness")).toHaveValue("dsh");
  600 |   const password = page.getByLabel("配置或更换 API key");
  601 |   await expect(password).toHaveAttribute("type", "password");
  602 |   // Explicit synthetic fixture, intercepted before the host: this never writes a real Keychain item.
  603 |   await password.fill("synthetic-browser-fixture");
  604 |   await page.getByRole("button", { name: "保存密钥", exact: true }).click();
  605 |   await expect(password).toHaveValue("");
  606 |   await expect(
  607 |     page.getByText("已保存到系统凭据库", { exact: true }),
  608 |   ).toBeVisible();
  609 |   await expect(
  610 |     page.getByRole("button", { name: "登记本机 Codex", exact: true }),
  611 |   ).toBeEnabled();
  612 |   await context.route("https://auth.openai.com/**", (route) =>
  613 |     route.fulfill({ body: "Synthetic OAuth navigation fixture" }),
  614 |   );
  615 |   const popupPromise = page.waitForEvent("popup");
  616 |   await page.getByRole("button", { name: "网页登录", exact: true }).click();
  617 |   const popup = await popupPromise;
  618 |   await expect(popup).toHaveURL(
  619 |     "https://auth.openai.com/authorize?fixture=true",
  620 |   );
  621 |   await popup.close();
  622 |   await page.getByLabel("默认 Agent Harness").selectOption("codex");
  623 | });
  624 | 
```
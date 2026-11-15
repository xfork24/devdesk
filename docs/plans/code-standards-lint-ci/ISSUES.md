# 存量问题记录（非本次 lint/CI 改动引入）

以下问题在未含本次改动的干净 main 上复现（验证方法见文末），按用户指示**只记录、不修改**。每项给出根因、证据与建议处理方向。

## A. 项目经理协作面板折叠导致 4 个 e2e 用例失败（UI 回归，根因已定位）

**用例**：`workflow-read-isolation.spec.ts` 147 / 195 / 363（pending、failed 两参数）

**现象**：统一为"元素存在但 hidden"——`toBeVisible` 失败时 locator 已 resolve 到元素（如 `<p role="status">slow controller observed</p>`），或 `selectOption` 报 "element is not visible" 持续 15s。

**根因**：提交 `50f3774`（Separate workflow management and execution evidence readers，2026-11-15）把平铺可见的"调度与经理管理"内容包进 `web/src/components/WorkflowController.tsx` 的 `<details open={workspaceOnly || undefined}>`——协作模式（非工作台模式）默认折叠。而该套测试的验收合同要求以下内容无需点击直接可见：

| 用例 | 断言 | 元素位置（折叠区内） |
|---|---|---|
| 147 slow controller polling | `slow controller observed`（controller.reason） | WorkflowController.tsx 约 364 行 |
| 195 paused root manager resume | `经理 Agent 精确修订` / `经理授权上下文` select + `采用并恢复经理` | 约 261-291 行 |
| 363×2 manager runtime visible | `当前调度：agent · 代次 7`、reason、`人工接管` enabled | 约 300-364 行 |

页面快照证实 region 内只剩 `group: 调度与经理管理`（折叠态）。该提交合入时未跑此套件。

**建议方向**：`<details>` 默认展开（协作模式与工作台一致），或把运行时状态与恢复入口移出折叠区；合入时必须跑 `workflow-read-isolation.spec.ts`。注意：验证修复时需先 `npm --prefix web run build` + `cargo build -p devdesk-host` 重建——host 嵌入的是构建时的 web/dist，只改源码直接跑 Playwright 测的是旧界面。

## B. execution-layout"计划与执行历史"reload 后消失（数据链问题，根因待深查）

**用例**：`execution-layout.spec.ts:220`"直接调整运行计划、复用成果、绕过与移动端刷新"（基线行号 218）

**现象**：工作流 `.poll` 至 completed、计划变更断言全部通过后，切 390×844 移动视口并 reload，点击"计划与执行历史"超时。失败时页面快照中该 `<details>` 块（`WorkflowInstanceView.tsx:232`，渲染条件 `data.history.length` 非空）**整个不存在**。

**初步定位**：前序步骤已产生计划变更（`active_plan_revision_id` 变化断言通过），但 reload 后前端拿到的 `history` 为空——是内核 query 响应问题还是前端加载时序问题尚未判定。与 Docker 无关（该用例的流程可离线完成）。

**建议方向**：在 reload 后直接调 `query.workflow_instance` 核对 `history` 字段是否为空，区分后端/前端；注意本仓库 spec 行号会因格式化漂移，基线对照需按测试标题定位。

## C. Docker 网络环境依赖（本机拉不到镜像）

| 项 | 说明 |
|---|---|
| C-1 Rust `role_revisions_do_not_change_agents_pinned_to_the_previous_source`（host 单元测试） | agent 组合需 `docker build` 构建运行资源配方（`crates/adapters/src/runtime_preparation.rs:121`）；本机 Docker daemon 正常但零镜像、到 registry 网络不通 → `运行资源构建失败`。有镜像源的环境（如 CI）应能通过 |
| C-2 e2e `integrations.spec.ts:296`（MCP stdio 发现 + Docker 调用回执） | 用例本身依赖 Docker 调用回执，同类环境问题（此项未做单独基线对照，判定依据为失败模式与用例内容） |

## D. 全套运行时的连锁失败（非独立缺陷）

`workflow-authoring.spec.ts` 379 / 523 / 700 在全套运行时失败、**单独重跑全部通过**：前面的 Docker 失败把错误状态（页面残留 `Codex · 运行资源构建失败` 的 `role="status"` 元素）留在运行上下文，后续 `getByRole('status')` strict-mode 断言命中两个元素（379 的报错即 strict mode violation）。在 C 类环境恢复后应自行消失；若要根治可让失败状态不跨用例残留。

## 基线复现方法

```bash
git worktree add /tmp/devdesk-baseline main
cd /tmp/devdesk-baseline
npm --prefix web ci && npm --prefix web run build
cargo build --locked -p devdesk-host
(cd web && npx playwright test tests/workflow-read-isolation.spec.ts)   # A 类
(cd web && npx playwright test -g "直接调整运行计划")                     # B 类（按标题定位，行号会漂移）
cargo test -p devdesk-host --bin devdesk-host role_revisions_do_not_change  # C-1
git worktree remove /tmp/devdesk-baseline --force
```

上述用例在干净 main 上呈现与本报告一致的失败。本仓库 spec 的 `file:line` 过滤对行号漂移敏感（Prettier 格式化会改变行号），基线对照优先用 `-g <标题>`。

# 出厂配置精简与 dsh/minimax/glm 栈补全 · 执行与验收

日期：2026-11-12。基线：`main` @ `48603ff`。最终验收：`cargo test --workspace --locked` 26 套件 / 231 项全绿（含本计划新增 4 个回归）；`scripts/py/check-defaults.py` 自洽通过；静态 grep 审计全干净。

本报告按**最终状态**记录，过程性事件（实施→回滚→双检）在 §3 时间线。

## 1. 最终交付状态

| 项 | 结果 |
|---|---|
| 历史修订精简 | 删除 **56 个**历史文件 + `.DS_Store`；manifest 移除 spec v1 两条；目录内 146 个 identity 每个恰好 1 个文件；无孤儿 JSON/MD |
| dsh 栈新增 | 28 个文件：`builtin-model-minimax-m3-1-flash:1`（minimax-cn，1M/512K）、`builtin-model-glm-5-3-flash:1`（zai-coding-cn，1M/128K）+ 14 capability（默认 + 6 角色 ×2，工具仅 `dsh-native:1`，skills 镜像 codex）+ 12 agent（PM 为普通 agent 形态） |
| manifest/bundle | 每 identity 一条；`bundle_version` `2026.11.11-standard-v4` → `2026.11.12-standard-v5`；defaults.json 出厂仍指 codex 栈（harness :1 / capability :3 / agent :4,PM:5 / workflow :6 / standard :3，**与 main 修订一致**） |
| 版本硬编码清理 | 6 处 src + 10 处测试 fixture 的 `"0.2.2-rc.1"`/`"0.167.0"` 字面量 → `DEFAULT_DSH_VERSION`/`DEFAULT_CODEX_VERSION`；`crates/` 内字面量仅存 `domain/src/defaults.rs`；测试/web/live 的修订字面量与最终修订一致 |
| 门禁 | 4 处编排/平台工具门禁**维持 `provider == "codex"` 原样**（用户决定，见 §3）；`dynamic-tools` 相关代码零残留 |
| 边界回归（新增 4 测试） | `crates/adapters/tests/configuration_resources.rs`：①dsh 栈清单与推荐上下文精确断言；②dsh PM 绑编排被拒；③工厂流程重绑 worker 到 dsh 被工具授权拒绝；④自建无工具授权流程 + 自建权限上下文下 dsh worker 编译通过 |

## 2. dsh 栈适用边界（回归锁定的结论）

1. 配置层完整可用（materialize 全量组合校验通过）。
2. 编排：仅 codex；dsh PM 绑定编排角色被明确拒绝。
3. 出厂 feature/task 流程：engineering 权限上下文几乎每个 agent 步骤都授权 `workflow.outputs.read`（平台工具）或 `codex-native`，dsh capability 挂不了 → 任何角色重绑 dsh 都无法编译出厂流程。
4. 自建流程（无平台工具授权步骤 + 自建权限上下文）：dsh worker 正常绑定与编译（测试④）。**dsh 可以做 worker，限制来自步骤工具授权，不是机制禁令。**
5. 工厂流程全量 dsh 化（含平台工具）依赖 [p14](../dsh-tool-bridge/PLAN.md) 工具桥（挂起）。

## 3. 时间线（为何最终状态与初版计划不同）

1. **实施（当日上午—下午）**：完成 W01-W03；并按用户当时"修改代码以支持 dsh 编排"的指示实施了 W4 门禁能力化（`dynamic-tools` 标记、codex harness 升 rev2、7 capability/6 agent/tests-run/workflows/standard/permission-context 级联升修订），期间发现并处理了计划外级联（tests-run 工具 runtime 钉 harness 引用 → 整条链升版）；全部测试通过。
2. **回滚（当日晚间）**：用户核实 DSH SDK 无工具桥后决定**暂维持仅 codex 调度**，要求取消为 dsh 编排铺路的代码。回滚内容：`DYNAMIC_TOOLS_CAPABILITY`/`supports_dynamic_tools()` 删除、4 处门禁恢复原判断、harness/capability/agent/tests-run/workflows/standard/permission-context/defaults 全部还原为 main 原修订（删除级联新增的 19 个文件、git restore 20 个原文件、manifest 重建）；测试字面量与 web/live 同步回退。回滚后全量测试通过。
3. **双检（同日）**：静态审计 5 组 grep 全干净；新增边界回归 4 个；发现并修正"dsh worker 可切换跑工厂流程"的过强表述（§2.3/2.4）；清理陈旧文档（resources/configuration/README 的 082c081 基线描述、p12 执行报告 W04 追记）。

## 4. 验收对照（最终）

- A01 26 套件全绿 ✓（231 项）
- A02 identity 唯一 + 目录自洽（check-defaults.py 输出 146 定义）✓
- A03 残留 grep：已删修订引用、版本字面量、`dynamic-tools` 全部零命中 ✓
- A04 4 个边界回归全绿 ✓
- A05 defaults 指向 codex 栈 ✓

## 5. 全量检查与未尽事项

- `./scripts/check/check.sh` **全量通过**（2026-11-12，exit 0）：check-permissions、check-defaults（146 定义自洽）、web 构建、Rust 全量（26 套件）、debug/release 双档签名构建、HTTP 合同、template_reset、dataset_protocol、runtime_preparation、Playwright debug 50/50 + release 50/50。
- 检查过程中修复一处本次改动引出的 e2e 依赖：`settings-interactions.spec.ts` 的角色讨论测试依赖"目录默认第一个 agent 是 codex"，dsh agent 入目录后默认项变化 → 改为显式选择 `builtin-agent-sol-high-analyst:4`（web/tests/settings-interactions.spec.ts:397-400）。另 `workflow-authoring` 曾在全量并发下出现一次 `expected_revision` 偶发（单独复跑 2×10/10 通过，最终全量 50/50 通过，非本次改动引起）。
- codex 额度不足：实跑 feature 开发闭环仍未做（需真实凭据），与静态/自动化验证互补。
- dsh 栈不能跑出厂流程（§2.3）——依赖 p14 工具桥，配置层无法单独解决。

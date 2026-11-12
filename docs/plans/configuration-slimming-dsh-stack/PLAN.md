# P13 · 精简并补全出厂配置：清理历史修订 + 同步 dsh/minimax/glm 栈

日期：2026-11-12。状态：**已完成（范围经一次调整：门禁能力化实施后按用户决定回滚）**。来源：用户需求——"精简并补全 resources/configuration 下面的所有配置：①同一对象配置身份仅保留最新版本，删除历史版本，代码中版本硬编码移除；②model/agent/capability 同步支持 dsh harness 与 minimax/glm 模型（minimax 用 MiniMax-M3.1-Flash，glm 用 glm-5.3-flash，采用推荐模型上下文设置）"。

## 0. 范围演变（先读）

- 初版计划含"门禁能力化"（W4：4 处 `provider=="codex"` 改为 `dynamic-tools` 能力声明，为 dsh 编排铺路）。实施完成当晚，用户决定**暂维持现状、仅允许 codex 作为调度者**，W4 及其配置级联已整体回滚。
- 本文件的方案设计按**最终范围**描述；实施过程、回滚与双检细节见 [EXECUTION.md](EXECUTION.md)。

## 1. 背景与问题

1. 出厂目录带 56 个历史修订文件（agents v3/v4、capabilities v2/v3、workflows v5/v6、deliveries v1、specs v1、skills v1、tools v1、bindings rev1/2），不在 manifest 中、从未成为配置记录，仅被 `build.rs` 的 `collect()` 嵌入二进制。
2. 代码里散落运行时版本字面量（`"0.2.2-rc.1"`/`"0.167.0"` 与 `defaults.rs` 常量并存）。
3. 只有一套模型栈（codex + gpt-6.1-sol）；dsh harness 已发布却无任何可用组合（无 glm/MiniMax 的 model 对象，无 dsh capability/agent）。

## 2. 目标与非目标

**目标**

1. 同一 identity 仅保留最新修订：删除全部历史文件，manifest 收敛为每 identity 一条。
2. 补全 dsh 栈出厂配置：MiniMax（`MiniMax-M3.1-Flash`，上下文 1000000/524288）与 GLM（`glm-5.3-flash`，1000000/131072）各一套 model + capability（默认+6 角色）+ agent（6 角色），全部仅挂 `builtin-tool-dsh-native:1`。
3. 版本硬编码清理：运行时版本字面量合并到 `DEFAULT_DSH_VERSION`/`DEFAULT_CODEX_VERSION`；基线防漂移改由 manifest 自洽脚本承担（p12 W06 机制）。
4. 明确 dsh 栈适用边界并用回归锁定（见 §5 边界）。

**非目标**

- **不改 4 处编排/平台工具门禁**（`core/configuration.rs:100,105`、`core/controllers.rs`、`core/configured_workflows.rs` 保持 `provider == "codex"`）——仅 codex 可作为调度者（用户 2026-11-12 决定；曾实施能力化后回滚）。
- 不复制 workflow/standard（与模型无关，role_agent_bindings 切换）。
- defaults.json 出厂默认保持 codex / gpt-6.1-sol。
- 不重命名文件名后缀与内部 revision 不一致的现存文件；不动 DeepSeek 冻结连接回退分支、`dsh-model-catalog.json`、容器与冻结配方。
- 不实现 DSH 工具桥（[p14](../dsh-tool-bridge/PLAN.md)，挂起）。

## 3. 方案设计

### 3.1 历史修订删除（最终保留状态）

| 目录 | 保留（codex 侧与 main 相同的最新修订） |
|---|---|
| agents/ | 6 个 codex agent（v4，PM v5）+ 12 个 dsh agent（`builtin-agent-{minimax\|glm}-{role}` rev1） |
| capabilities/ | 7 个 codex（v3，PM v4）+ 14 个 dsh（`builtin-capability-{role}-{vendor}-dsh` 与默认 `{vendor}-dsh`，rev1） |
| models/ | gpt-6-1-sol-high v1 + minimax-m3-1-flash v1 + glm-5-3-flash v1 |
| harnesses/ | codex-managed v1、dsh-multi-provider v1 |
| workflows/ | feature/task `-v7.json`（rev6） |
| standards/ | engineering v3 |
| permissions/bindings/ | engineering-v3 |
| 其余 | 21 交付、25 spec（v1 两条已移除出 manifest 并删除）、10 skill、32 tool |

### 3.2 dsh 栈新增（28 个文件）

- 模型身份精确到 wire 模型（`builtin-model-minimax-m3-1-flash` / `builtin-model-glm-5-3-flash`），栈级对象用厂商短名；skills 镜像 codex 对应角色（engineering-context:2 开头）；工具仅 `dsh-native:1`；PM 为普通 agent 形态（含编排 skill，但无控制工具，不能编排）。
- manifest 收敛 + `bundle_version` → `2026.11.12-standard-v5`；defaults.json 不变。

### 3.3 版本硬编码清理

- 6 处 src + 10 处测试 fixture 的运行时版本字面量 → 常量；`tests/support/defaults.rs` 与散落字面量随最终修订同步（维持 main 原修订号）。
- 原 082c081 hash 基线断言由 p12 的 `scripts/check/check-defaults.py` 取代（目录自洽）。

## 4. 工作项

| 编号 | 工作项 | 状态 |
|---|---|---|
| W01 | 删除 56 个历史修订文件 + `.DS_Store`；manifest 移除 spec v1 两条 | 已完成 |
| W02 | 新增 dsh 栈 28 文件 + manifest/bundle_version | 已完成 |
| W03 | 运行时版本字面量合并常量 + 测试字面量同步 | 已完成 |
| W04 | 门禁能力化（`dynamic-tools` + codex harness 升修订级联） | **已实施后按用户决定回滚，未保留**（4 处门禁维持 `provider == "codex"`，出厂 codex 修订链与 main 一致） |
| W05 | 边界回归：dsh 栈清单 / 编排拒绝 / 工厂流程工具授权边界 / 自建流程可用 | 已完成（4 个测试，见执行报告） |

## 5. dsh 栈适用边界（最终结论，回归锁定）

1. **配置层**：dsh 双栈完整发布，通过 materialize 全量组合校验；PM 为普通 agent。
2. **编排**：绑定 dsh PM 为编排角色被编排门禁明确拒绝（仅 codex 可调度）。
3. **工厂默认流程**：engineering 权限上下文给几乎每个 agent 步骤授权 `workflow.outputs.read`（平台工具）/ `codex-native`，dsh capability 无法挂载 → 把任何角色重绑到 dsh agent 都无法编译**出厂** feature/task 流程。
4. **自建流程**：不含平台工具授权步骤的工作流 + 自建权限上下文 → dsh worker 可正常绑定并编译（回归 `dsh_workers_compile_custom_workflows_without_platform_tool_grants`）。即 dsh **可以**做 worker，限制来自步骤工具授权而非机制本身。
5. 要在工厂流程全量使用 dsh（含 outputs-read、tests.run 等平台工具）依赖 [p14](../dsh-tool-bridge/PLAN.md) 工具桥。

## 6. 验收标准（最终）

- A01 `cargo test --workspace --locked` 全绿。
- A02 每 identity 恰 1 个文件；manifest↔目录↔defaults 引用↔md 正文归属自洽（check-defaults.py）。
- A03 全仓无已删除修订/版本字面量残留（grep）。
- A04 dsh 栈：模型推荐上下文、native-only 工具、skills 镜像、编排拒绝、边界 3/4 如上（4 个回归测试）。
- A05 defaults.json 仍指向 codex 栈。

## 7. 风险与兼容

- 已有数据集不受影响（记录在库）；bundle_hash 变化使旧数据集显示非纯净直至模板重置（既有语义）。
- dsh 栈不能跑工厂流程（见 §5.3）——这是平台工具桥缺失的直接后果，配置层无法单独解决。

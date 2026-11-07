# DevDesk 0.2 验收记录

日期：2026-11-08。平台：Linux x86_64，Ubuntu 24.04.5 LTS。执行模式：simulation。

## 验证结果

- `cargo test --workspace`：32 项通过（29 集成/契约/调度/存储测试，3 领域测试）。
- `npm --prefix web run build`：TypeScript 检查与 Vite 构建通过。
- `python3 tests/http_contract.py`：真实 debug host 的会话鉴权、Host/Origin、幂等、Artifact、安全退出与重启检查通过。
- `python3 tests/http_contract.py target/release/devdesk-host`：release host 同一 HTTP 检查通过，工作目录为空的临时目录。
- Chromium Web E2E：3 项通过，24.2 秒；无浏览器 pageerror。使用真实 Rust API、SQLite 和文件，没有浏览器假后端。
- `cargo build --release --locked -p devdesk-host`：通过；运行时从空目录成功读取内嵌 HTML/JS，无 Node 服务。
- 独立阶段评审发现的阻塞已关闭，见 [评审记录](reports/independent-review.md)。

[工作台实际截图](assets/workbench.png)。

## 原计划验收矩阵

| 项目 | 证据 | 结果 |
|---|---|---|
| A1 需求到mainline | a1_full_loop；Web主闭环含人工需求、双Task、收尾、Merge、报告/历史 | Linux通过 |
| A2 串并行/额度/写许可 | a2_parallel_runs、logical_write_permit；真实后端同时running记录 | 通过 |
| A3 技术失败重试 | a3_technical_retry；Web异常回路的Task与Merge重试 | 通过 |
| A4 退回新Attempt | a4_rejection；Web退回后新Attempt，旧版本保留 | 通过 |
| A5 输入失效 | a5_input_change；inflight失效不阻塞调度器 | 通过 |
| A6 目标变化重验 | a6_target_change；旧候选阻塞，新候选重新验证/Review | 通过 |
| A7 规范升级/替换/继承 | a7_spec_upgrade、alternative_spec；旧锁不回写，Task独立绑定 | 通过 |
| A8 暂停/取消/Override | pause/cancel、a8_quality_override、取消不可复活与Completed冻结 | 通过 |
| A9 刷新/重启/幂等 | a9_restart、merge_command_replay、HTTP重启、Web刷新 | 通过 |
| A10 Context/Snapshot | a10_context；冻结Attempt上下文、Snapshot JSON、Web比较入口 | 通过 |
| A11 模拟边界 | a11_hard_tdd、simulated_never_live；UI/报告来源标记 | 通过 |
| A12 Linux/macOS实机 | Linux后端与完整Web已运行；macOS未运行 | Linux通过；macOS pending |

浏览器自动化覆盖三个代表性场景，不声称每个后端异常都已逐一做浏览器自动化。全部异常命令均有Web入口，后端规则由上述集成测试覆盖。

## 可重复小样本

同一测试数据集：10 Projects / 100 Features / 1,000 Tasks。分页列表、详情和事件三个查询共约 **28.82ms**（debug单次本地测量，包含完整状态加载；不是性能SLA）。

release 可执行文件 **6,604,784 bytes**（约6.30MiB）；内嵌Web JS 266.64kB（gzip83.35kB），CSS14.64kB（gzip4.09kB）。从空工作目录启动到首个HTML返回约 **6.17ms**，属本机单次测量，不包含浏览器启动时间。

## 已知限制与下一轮边界

1. **macOS Apple Silicon 实机验收未执行。** 构建路径无平台业务API，但兼容结构与编译说明不替代实机结果，F1/F5全平台验收仍pending。
2. Git/Test/Harness/Sandbox只有模拟实现；没有真实代码合入、模型推理或被管理项目的测试。live模式未开放。
3. Archive、迁移只预览；不执行导出、清理、冷数据恢复或搬迁。重启标记中断并显式重试，不透明重建真实环境。
4. 工程存储port当前每次工作单元加载类型化状态；查询输出分页，表有外键与索引。已覆盖小样本，未承担大量Run历史/百万级压测。后续可在同一port内改增量查询。
5. Spec仅解释内置有限规则；模拟建议为确定性模板，可编辑，不是智能生成。Review结果与Override保留原事实。
6. 当前Delivery列表的Review展示有界；多次不确定决定的大量历史可通过分页事件与报告读取，尚未提供独立全文检索。协议不支持JSON-RPC batch/notification。

Linux原型交付后停止扩展。下一轮待实际使用反馈，再讨论真实Git/Test/Codex适配、macOS验收及数据生命周期。

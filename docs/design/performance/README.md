# Workflow 性能基准与时序

这里保存真实完整工作流的只读计时、验收结果和图表。净耗时排除审批请求创建后的外部响应，以及有系统电源日志佐证的主机休眠；完成后的观察延迟不计入。审批准备、接纳、测试、Git、模型服务和真实收尾均保留。

## 优化前后

- [优化实现、完整回归与效果分析](workflow-optimization-2026-11-20.md)
- [完整交互对比图](workflow-comparison-2026-11-20.html)：离线可用，可切换原始/净口径及前后两轮；点击阶段或步骤查看时间戳与三类组件分配。
- 优化后完整 SVG：[净流程](workflow-timeline-2026-11-20-optimized-net.svg) · [原始墙钟](workflow-timeline-2026-11-20-optimized-raw.svg)
- 同一格式的优化前完整 SVG：[净流程](workflow-timeline-2026-11-20-before-net.svg) · [原始墙钟](workflow-timeline-2026-11-20-before-raw.svg)
- 可重算数据：[优化后基准](workflow-benchmark-2026-11-20-optimized.json) · [优化后组件账本](workflow-cost-breakdown-2026-11-20-optimized.json) · [前后对比及源文件摘要](workflow-comparison-2026-11-20.json)

## 原有基准与图表

- [原完整回归与基准报告](workflow-benchmark-2026-11-20.md) · [原始数据](workflow-benchmark-2026-11-20.json)
- [原 15 分钟组件拆解](workflow-cost-breakdown-2026-11-20.md) · [组件数据](workflow-cost-breakdown-2026-11-20.json)
- 原有交互图：[原始墙钟](workflow-timeline-2026-11-20-raw.html) · [净流程](workflow-timeline-2026-11-20-net.html)
- 原有 SVG：[原始墙钟](workflow-timeline-2026-11-20.svg) · [净流程](workflow-timeline-2026-11-20-net.svg)

原有八份文件从 `docs/design/` 迁入本目录。原基准和组件 JSON、HTML、SVG 保持原始字节；Markdown 仅调整相对链接及复核命令。旧 JSON 内的历史来源路径保留不改，新对比数据记录当前文件名和 SHA-256。

图中的阶段、Feature 实施和 Task 子流程包含后代，不能把各行持续时间相加。三类互斥组件账本才可相加为净流程总时长；阶段窗口是从另一个角度观察同一份时间。

## 重建图表

从仓库根目录执行下列命令，只读取已保存 JSON，不启动模型、创建容器或推进流程。重新抽取原始基准和组件账本还需对应的本地持久化数据库，方法见各报告。

```bash
python3 tests/live/workflow_performance.py \
  --before docs/design/performance/workflow-benchmark-2026-11-20.json \
  --before-costs docs/design/performance/workflow-cost-breakdown-2026-11-20.json \
  --after docs/design/performance/workflow-benchmark-2026-11-20-optimized.json \
  --after-costs docs/design/performance/workflow-cost-breakdown-2026-11-20-optimized.json \
  --directory docs/design/performance
```

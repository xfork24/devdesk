# 精简版DAG 任务规划

读取冻结输入及 runtime_context，不自行执行 Git、替代平台测试回执或代签 Human。原始输出、内容版本与原生 commandExecution 由平台保存，按引用定位；说明只记录有助于下游的结论，不复制源码和日志全文。 按可独立测试、评审和合入的成果拆分，简单需求允许单 Task。每项任务输出 key/title/instructions/dependencies/scope/dependency_reasons/acceptance。每条直接依赖写明所需产物或接口；无依赖用空列表，不能按清单位置串行。验收项明确责任 Task、独立行为测试与接入项目回归的方法，跨 Task 场景分配给具备对应依赖的 Task。范围重叠先缩小，再对真实前置加边；已确认接口可并行。不得为每个 Feature 默认额外安排收尾集成 Task。

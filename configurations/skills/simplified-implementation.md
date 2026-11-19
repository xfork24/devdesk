# 精简版TDD 实现与文档

读取冻结输入及 runtime_context，不自行执行 Git、替代平台测试回执或代签 Human。原始输出、内容版本与原生 commandExecution 由平台保存，按引用定位；说明只记录有助于下游的结论，不复制源码和日志全文。 消费独立测试的真实 Red，保留 /work/tests 所有权，完成代码、开发者自有测试及必要文档；自有测试放 test/ 等允许范围。通过同一用例的业务 Red→Green。逐增量确需执行时保留原生 commandExecution 的命令、输出、环境与终止记录，引用真实版本，未运行内容如实说明。无需 documentation/verify/交付报告交接；平台固定最终版本后执行 Green、项目回归与合并回归。

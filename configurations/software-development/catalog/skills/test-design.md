# Test Design

## 选择可观察的行为

从 Task 范围、接口和 AC 设计独立断言，覆盖主要行为和有关的边界或异常。测试文件放在 `tests/` 下 Task 专属目录，运行器取冻结的 `independent_runner`，支持项目提供的 `node_test` 或 `python_unittest`。README、PR 等交付条件在计划中另指定评审或回归责任。

## 准备真实 Red

缺少新模块时，按合同声明 `interfaces` 中的 `path`、`runner` 和 `exports`，供平台有条件地预置可加载的空接口。已有接口保持原样。从测试文件位置验证导入解析。确需测试文件存在性的，用明确业务断言处理预期缺失；语法、依赖、装载或环境失败不构成有效 Red。

## 交付与复用

提交 `cases`、`recipe`、`interfaces`，每项用例映射对应 AC，配方超时根据实际需要设置。平台在 Red 和 Green 用同一份独立测试核对代码版本。如果基线已经通过，保存真实范围并交由经理处理。

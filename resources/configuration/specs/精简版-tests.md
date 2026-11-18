# 精简版独立测试规范

按 Task 验收写业务断言，cases 映射验收项。recipe 使用冻结 independent_runner，文件范围位于 tests/。interfaces 仅用于条件空接口，不实现业务。不将加载、环境错误或已有成功当有效 Red。

# Rust 开发规范

格式和常规静态检查由 rustfmt 与 clippy 统一处理。代码审查重点放在职责、错误语义、版本检查和外部副作用，不手工发明第二套格式。

## 层次与接口

domain 保持纯计算；core 通过 ports 使用外部能力；adapters 实现数据库、文件、进程与远端协议；host 完成装配和服务入口。ID 和时间通过应用环境提供，不能让纯领域规则直接取系统值。

新增模块沿用当前目录和命名：文件/函数 snake_case，类型 PascalCase，常量 SCREAMING_SNAKE_CASE。模块可用 `mod` 加明确 `pub use` 暴露入口；不要为了调用方便扩大本应私有的实现。

crate 内既有 `use crate::*` 或 adapters 的 `use super::*` 属于当前组织方式。整理代码时避免无关的全仓导入重写。

## 数据与失败

JSON 字段采用现有 serde 合同；输入对象需要拒绝未知字段时使用 deny_unknown_fields，缺省值必须有明确业务语义，不能机械给所有字段加 default。

错误应保留可判断的领域代码和操作身份。外部失败需要区分“确认未发生”“已发生”和“结果未知”，不能都转换成可安全重试的普通失败。

unwrap 用于测试或已经验证的不变量；生产 expect 应解释前提。避免把凭据或完整用户输入放进错误和日志。

资源提交必须包含用于决定动作的原版本。SQLite 竞争的技术重试不能重新准备业务命令，也不能再次发送外部副作用。

## Lint 例外

workspace 使用统一 lint 配置，不以全局 allow 解决单点问题。参数数量、DTO 大小等确需例外时，在具体位置加 allow 和解释原因；提交说明交代新增例外的必要性。

格式交给 rustfmt，包括它不会主动展开的 JSON 构造式。不为了格式偏好改动与任务无关的大段代码。

## 测试与平台

单元测试放文件末尾的 cfg(test) 模块；集成测试按仓库现有结构组织，跨包夹具使用 tests/support。临时数据用独立目录，生产代码不引入接口替身回退。

平台代码通过 cfg 门控，测试保留对公共校验的覆盖。真实 Docker、模型、GitHub 验证使用明确入口和前置条件，与离线回归区分。

## 检查入口

```bash
cargo fmt --all -- --check
cargo clippy --workspace --all-targets --locked -- -D warnings
cargo test --workspace --locked
```

工具链以 [rust-toolchain.toml](../../../rust-toolchain.toml) 为准。`scripts/check/check-style.sh` 还检查并构建 Web；首次执行需准备前端依赖。范围和选择方法见[测试指南](../testing-and-extension.md)。

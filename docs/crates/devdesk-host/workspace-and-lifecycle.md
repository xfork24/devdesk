# 启动、装配与生命周期

从 [main.rs](../../../crates/host/src/main.rs) 读程序启动，再进入 [workspace.rs](../../../crates/host/src/workspace.rs)。前者负责进程和 HTTP，后者管理数据集内的应用对象。

## Workspace 怎样组织对象

Workspace 持有 GlobalStore、配置 Engine、项目 Engine 缓存、运行准备服务、事件中心和后台调度状态。`Workspace::open` 创建执行适配器，`open_with_adapters` 支持明确传入适配器，生产入口使用真实实现。

`engine` 函数创建 FileArtifacts、NativeTools，并给 Engine 接入经验读取和配置后端。项目读取通过 `project_engine` 选定作用域；一旦进入具体 Engine，就不会因为另一浏览器切换项目而换库。

## 生命周期与业务状态分离

[lifecycle.rs](../../../crates/host/src/lifecycle.rs) 的 Lifecycle 保存 Host instance、revision 和状态，`begin` 为 setup/shutdown 检查原实例、原版本及 command_id。已完成的同命令返回原结果，正在执行的操作返回明确进行中状态。

setup 先取得本次操作资格，再在耗时加载后核对并完成。浏览器断开不应让 Host 丢失正在完成的初始化。Host 重启后 instance 改变，旧页面不能继续操作新进程。

生命周期回执是当前进程的有界记录；工程命令回执保存于数据库。不要用前者代替跨重启的业务幂等。

## 关闭流程

停止请求进入共同的停机路径：阻止新调度、关闭相关流、等待后台阶段并核对所属 worker。`stop_owned_runtime` 处理运行停止，不能只退出 HTTP 就认为写进程已经结束。

关闭浏览器不触发这一流程。重新选择数据集需要退出 Host 后启动，不能在已打开的 Workspace 上隐式切库或搬迁数据。

## 验证与修改

新增装配依赖应在明确创建点注入，避免组件内部创建第二套存储或 Harness。修改初始化和退出时测试迟到完成、重复命令、旧 instance、数据集锁和客户端断开。

入口：[lifecycle_contract.py](../../../tests/entrypoints/lifecycle_contract.py)、[dataset_protocol.py](../../../tests/entrypoints/dataset_protocol.py)，以及 workspace.rs 和 lifecycle.rs 内的单元测试。

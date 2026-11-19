# Harness 协议与 Docker 容器

这里把 core 的 SessionStart、TurnDispatch 等请求转换成真实进程操作。理解顺序是 Harness 端口实现 → 协议连接 → Docker 资源，不要从一条 Docker 命令推断业务状态。

## 会话启动

[harness.rs](../../../crates/adapters/src/harness.rs) 提供受管 Harness 和路由实现，[harness_startup.rs](../../../crates/adapters/src/harness_startup.rs) 处理启动阶段。请求携带确定的会话、运行安装、权限和连接信息。

适配器为会话准备私有目录及运行输入，使用 DockerSandbox 创建容器，再由 JsonlProcess 建立协议连接。安装镜像可共享，状态和连接按会话区分。

## 协议转换

[jsonl.rs](../../../crates/adapters/src/jsonl.rs) 管理请求 ID、读写、等待和消息帧。Docker 的 attach_command 生成 `docker start --attach --interactive`，使 Host 经标准输入输出连接容器内程序。

| core 操作 | 供应商转换 |
|---|---|
| 打开或恢复会话 | Codex thread/start 或 thread/resume；DSH 对应会话准备 |
| dispatch_turn | Codex turn/start；DSH session/prompt |
| poll_session | 将通知、响应、交互和终态转换成 ConversationObservation |
| answer_tool_call | 回复原 item/tool/call 或 devdesk/tool/call 请求 |

`provider_turn_text` 选择已经准备好的任务输入；供应商线程恢复时避免把同一历史正文再重复注入。协议日志和正式响应应分别处理。

## Docker 原语

[docker.rs](../../../crates/adapters/src/docker.rs) 定义 ContainerSpec、ContainerReceipt 和 DockerSandbox。ContainerSpec 包含项目、会话、操作身份、镜像、挂载、网络和命令。operation_id 同时帮助识别同一操作的容器。

create/inspect/start/attach 完成启动，logs/logs_split 读取输出，stop 核对不再运行，remove 在停止后删除并核对结果。移除容器不删除共享镜像。Agent 不获得 Docker socket。

权限在实际挂载和进程环境中落实。路径检查、敏感错误脱敏和资源身份核验是适配器责任，但“是否允许派发这个步骤”已经由 core 判断。

## 中断时看什么

连接断开、进程退出和工作流完成是不同事件。适配器返回观察结果，core 决定是否需要核对、失败或接纳。stop/cleanup 接口应提供实际停止与移除信息，不能仅返回“已发送停止命令”。

验证入口：harness.rs 中的协议转换测试、[docker_resources.rs](../../../crates/adapters/tests/docker_resources.rs)、[原生验收脚本](../../../tests/entrypoints/native_acceptance.py)。原生协议检查不等于真实模型推理成功。

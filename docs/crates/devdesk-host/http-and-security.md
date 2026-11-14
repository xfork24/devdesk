# HTTP 入口、连接认证与事件流

[代码手册总览](../overview.md) · [完整执行链](../execution-flows.md) · [本包概览](overview.md)

浏览器只能获得当前 Host 签发的会话；远程监听还要求显式连接密钥。连接发现不泄露数据集，工程事实通过固定 Engine 的 RPC/SSE 提供。源码文件同时包含启动、HTTP 处理、静态资源与连接参数测试。

## 本页文件

- [src/main.rs](#file-src-main-rs)：服务启动、请求围栏与固定作用域 SSE

<a id="file-src-main-rs"></a>

## src/main.rs：服务启动、请求围栏与固定作用域 SSE

源码：[crates/host/src/main.rs](../../../crates/host/src/main.rs)。

App 保存可选 Workspace、随机会话 cookie、规范 authority/origin、连接密钥及停机信号。未指定数据目录可先提供 setup；初始化后不把一个运行中的 Workspace 随意热替换成另一数据集。

`guard` 对所有请求核对 Host，出现 Origin 时除 GET /connection 发现外要求同源。/rpc、/events、/setup、/shutdown、/capabilities 和 /artifacts/* 需要 Host 会话 cookie。cookie 为 HttpOnly、SameSite=Strict，HTTPS origin 才加 Secure；敏感响应禁缓存，并设置 CSP、nosniff 等头。/session 在配置密钥时核对密钥后签发 cookie；/connection 仅返回产品、协议和认证要求，可跨源发现，不授予数据访问。

connection_origin 只接受无凭据、路径、查询和 fragment 的 http(s) origin。非 loopback 或显式 public URL 要求 DEVDESK_ACCESS_TOKEN；值为 32..512 位非空白 ASCII，监听所有网卡还须指定客户端连接地址。

RPC 使用有 id 的单请求 JSON-RPC，领域错误投影为统一错误形状；/rpc 请求体上限 4 MiB。Artifact 按正式引用读取，不暴露任意文件路径。SSE 在建立时固定 Engine、scope 与游标，事件 ID 带工程范围，每 250ms 读取一批，项目切换不会让旧流混入新库。

启动处理 --data/--host/--port/--public-url、固定助手更新与显式凭据授权等入口，嵌入 web/dist。关闭先阻止新调度，等待当前调度阶段退出，再停止归属明确的执行者。文件末测试覆盖远程密钥、wildcard 和 origin 规范化；完整 HTTP 行为由跨进程测试补充。

代码定位：[`App`](../../../crates/host/src/main.rs#L34)、[`guard`](../../../crates/host/src/main.rs#L67)、[`connection`](../../../crates/host/src/main.rs#L121)、[`session`](../../../crates/host/src/main.rs#L133)、[`connection_origin`](../../../crates/host/src/main.rs#L161)、[`connection_key`](../../../crates/host/src/main.rs#L185)、[`rpc`](../../../crates/host/src/main.rs#L237)、[`events`](../../../crates/host/src/main.rs#L355)、[`run`](../../../crates/host/src/main.rs#L422)。

关联阅读：[host/src/workspace.rs](workspace-and-lifecycle.md#file-src-workspace-rs)、[core/src/history.rs](../devdesk-core/conversations-and-history.md#file-src-history-rs)、[adapters/src/lib.rs](../devdesk-adapters/storage-and-source-codecs.md#file-src-lib-rs)、[adapters/src/credentials_macos.rs](../devdesk-adapters/credentials-and-permissions.md#file-src-credentials-macos-rs)。

---

[代码手册总览](../overview.md) · [完整执行链](../execution-flows.md) · [本包概览](overview.md)

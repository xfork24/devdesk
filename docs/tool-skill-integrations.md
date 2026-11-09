# 工具与技能接入

左侧「工具与技能接入」包含 HTTP API、MCP、Skills、原生工具和调用记录。基础组件只管理 Model/Harness；Capability 继续明确选择确切 Tool/Skill 修订，项目为 Role 选择 Agent。

## 配置步骤

1. 保存 HTTP/MCP 接入源。HTTP 地址固定 Base URL；MCP 固定协议、HTTP 地址或镜像内已安装程序的参数数组。
2. 发布具体动作。HTTP 支持手工映射与 OpenAPI 3.1 JSON/YAML 预览、operation/名称/认证选择；MCP 按绑定账户分页发现，显示目录差异后选择发布。
3. 创建接入绑定，选择 Harness/Sandbox、认证方式、目标 origin、限制和允许的确切工具修订。先在 Permission 页面为该 Harness/Sandbox 分配可执行的权限 profile；工具不绕过该权限。
4. 在 Capability 选择具体 Tool/Skill，再组合 Agent。在项目管理页面为该 Agent 所需的来源选择绑定，也可在接入页面将绑定用于当前项目。
5. 准备工具执行环境并显式发起测试。到调用记录查看终态、来源、远端副作用、参数、审批/取消及文件结果。保存定义、凭据可用、镜像就绪、测试通过和 Agent 获得授权分别判断。

已有 Feature 冻结接入、工具、绑定及 Skill 材料。项目改绑不会改写 Feature；讨论在下一次静止边界创建后继 Session。普通停用拒绝新的选择并检查引用；紧急停用阻止后续派发，进行中的副作用仍需按回执核对。

## 认证与隔离

Bearer 和 Header API key 由系统凭据存储注入；配置只保存 `integration-secret:<uuid>`。HTTP 参数不能覆盖认证 Header。每个项目仍需显式采用绑定，全局定义不会自动授予全局账户。

OAuth 支持公共客户端的授权码 + PKCE S256、同账户刷新和撤销。先配置来源端点/client_id/scopes，保存待授权绑定，发起授权，打开服务页面并检查结果，再保存取得引用的新绑定修订。回调地址显示在来源编辑器中。账户或 scope 变化需新授权；不支持隐式授权、客户端密码、浏览器返回 token 或任意 OAuth 扩展。刷新或授权交换中断后不盲重放可能已消费的凭据。

macOS 新的接入凭据账户需要新版签名助手。原有 Host 和已有 Codex/DSH 登录无需重置；源码开发者显式执行 `./scripts/platform/macos/install-macos-credential-helper.sh --update`，或在包含新版助手的运行包设置中升级。正常构建不会替换已授权的共享助手。首次访问新凭据可能需要系统授权。Linux 使用已解锁的 Secret Service。

工具 HTTP/MCP 客户端在固定 Docker 镜像中运行；只获得当前运行允许的文件挂载、网络、工具与账户。源地址、解析后的私网地址和重定向受绑定检查；本机/内网服务需要明确允许。自定义 Harness 文件规则若无法由首版外部执行器完整执行，会明确拒绝。

## 支持范围

- HTTP：对象输入 Schema 实际校验，2020-12 与 draft-07、包内引用；path/query/header 与 JSON body 映射，JSON/文本/有完整性摘要的文件结果。输出 Schema 可省，只验证结构化内容。复杂序列化、远程 Schema 引用和转换脚本拒绝导入。
- OpenAPI：3.1.x 子集；server 必须与来源一致，变量需先展开。支持 Bearer、Header API key、授权码 OAuth 的单方案或明确选择的 OR 分支；AND 组合与未支持认证返回原因。
- MCP：2025-11-25 和 2026-07-28，stdio 与 Streamable HTTP。固定目录摘要；描述漂移需重新发现/发布。服务端模型采样、用户询问、roots/resources/prompts、长任务及多轮扩展未开放；stdio 的交互请求明确拒绝，不调用额外模型。
- Skill：Markdown、ZIP 静态 UTF-8 材料，或公开 HTTPS Git 的确切 40 位 commit。保留原文、来源、资源及工具依赖；脚本不安装、不自动运行。包材料在 `/skills` 只读挂载；内联 Markdown 与包材料不会重复注入，也不自动启用仓库中未选择的原生 Skill。

写请求失败或取消不代表远端回滚。`remote_effect=unknown` 的调用进入核对；重启接纳已有实际回执，缺少回执不自动重发。人工核对单独记录来源，不伪造成外部成功。

## 验证

普通回归：`cargo test --workspace --locked`、Web 构建及 debug/release Playwright。真实 Docker HTTP/MCP/OAuth 协议回归：

```bash
DEVDESK_TOOL_TEST_IMAGE=sha256:<固定镜像完整摘要> cargo test --locked -p devdesk-adapters --test tool_integrations docker_ -- --ignored --nocapture
```

真实模型验收只使用 Codex，见 [分阶段记录](plans/tool-skill-integrations/EXECUTION.md)。OAuth 协议夹具使用内存凭据替身，不代表已完成真实第三方账户或本机 Keychain 的交互授权。

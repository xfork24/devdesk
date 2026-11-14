# Git、GitHub、Docker 与测试执行

[代码手册总览](../overview.md) · [完整执行链](../execution-flows.md) · [本包概览](overview.md)

这些端口面对真实仓库、容器和测试结果，必须核对当前对象的归属、冻结内容、操作身份和原始回执。工程路径、候选提交、远端引用与测试代码快照分别有自己的边界。

## 本页文件

- [src/git.rs](#file-src-git-rs)：受管仓库、候选提交与精确引用推进
- [src/github.rs](#file-src-github-rs)：稳定 PR 身份、仓库政策与回执核验
- [src/docker.rs](#file-src-docker-rs)：归属明确的容器与不扩权的挂载
- [src/tests.rs](#file-src-tests-rs)：持久化测试作业与结构化 Node 结果

<a id="file-src-git-rs"></a>

## src/git.rs：受管仓库、候选提交与精确引用推进

源码：[crates/adapters/src/git.rs](../../../crates/adapters/src/git.rs)。

CliGit 总是接收明确 GitContext 并核对规范化工程仓库，不依赖宿主当前目录。运行 Git 时清除环境注入的 GIT 配置，限制 hooks、filters、fsmonitor 和 shell transport，防止受测仓库借普通查询执行宿主命令。

clone 与 worktree 以稳定操作文件记录归属、分支、路径和回执。恢复 clone 要核对原 origin；恢复 worktree 要匹配登记的工程/执行身份。冻结代码导出逐项检查文件、符号链接和内容摘要，不能覆盖已经漂移的导出。

候选提交要求实际父提交、预期 target 和完整 diff 符合约束；写入范围、保护路径与初始脏状态也参与核对。本地推进以 CAS 检查当前 HEAD/目标，远端推进使用精确 lease；未知结果返回 Applied/NotApplied/Unknown 供上层恢复，不能把 CLI 超时等同于未推送。

Git 端口不决定业务评审是否通过。core 汇集测试、独立评审、候选与发布合同后，才会派发相应操作。

代码定位：[`CliGit`](../../../crates/adapters/src/git.rs#L23)、[`RepositoryFacts`](../../../crates/adapters/src/git.rs#L7)、[`WorktreeReceipt`](../../../crates/adapters/src/git.rs#L15)、[`valid_oid`](../../../crates/adapters/src/git.rs#L27)、[`valid_repository`](../../../crates/adapters/src/git.rs#L33)。

关联阅读：[core/src/repositories.rs](../devdesk-core/integrations-and-repositories.md#file-src-repositories-rs)、[core/src/platform_tools.rs](../devdesk-core/manager-and-platform-tools.md#file-src-platform-tools-rs)、[adapters/src/github.rs](engineering-resources.md#file-src-github-rs)、[adapters/tests/git_resources.rs](testing.md#file-tests-git-resources-rs)。

<a id="file-src-github-rs"></a>

## src/github.rs：稳定 PR 身份、仓库政策与回执核验

源码：[crates/adapters/src/github.rs](../../../crates/adapters/src/github.rs)。

GitHubPullRequests 使用 gh API 读取仓库身份、权限、保护分支和 rulesets，分页读取完整规则；无法落实的政策保守阻断，管理员身份不是绕过分支保护的授权。

创建 PR 使用稳定 request marker、确切 head/base 和候选分支身份。若创建后丢失本地回执，ensure 先查找原 marker 对应 PR，避免再建一个。读取回执核对仓库、候选、分支、编号和合入 SHA，不能仅凭 PR 标题认领。

适配器不调用 GitHub merge API，也不提供管理员 bypass。发布用例中的候选推送、PR 创建/读取、目标引用推进由不同端口步骤组成；GitHub 的 merged 事实仍需独立读取并匹配，PR 已创建不等于交付已完成。

代码定位：[`GitHubPullRequests`](../../../crates/adapters/src/github.rs#L6)、[`diagnose`](../../../crates/adapters/src/github.rs#L8)、[`policy`](../../../crates/adapters/src/github.rs#L45)。

关联阅读：[core/src/platform_tools.rs](../devdesk-core/manager-and-platform-tools.md#file-src-platform-tools-rs)、[adapters/src/git.rs](engineering-resources.md#file-src-git-rs)、[adapters/tests/github_resources.rs](testing.md#file-tests-github-resources-rs)、[host/src/github_projects.rs](../devdesk-host/workspace-and-lifecycle.md#file-src-github-projects-rs)。

<a id="file-src-docker-rs"></a>

## src/docker.rs：归属明确的容器与不扩权的挂载

源码：[crates/adapters/src/docker.rs](../../../crates/adapters/src/docker.rs)。

ContainerSpec 固定用途、数据集/工程/会话/操作、不可变镜像、代码与状态路径、权限及资源预算。DockerSandbox 用确定的操作身份找到容器，再核对 devdesk.root/spec/project/session 等标签；同名但不属于当前操作的容器不会被接管。

路径、符号链接、挂载重叠和宿主目录边界在调用 Docker 前检查。容器按冻结策略设置只读根、cap drop、tmpfs、网络和资源限制；模型凭据只进入授权的 Harness 路径，不交给普通测试容器。嵌套 bwrap/AppArmor 依赖已核对内容的 Linux 主机资产，不能由容器内文件自称可信。

start、inspect、stop、remove 是独立动作。回执使用 Docker 实际纳秒时间戳计算运行时间和超限情况，迟到观察不能把超时变成合规。清理只处理当前归属资源，不删除其他会话或共享镜像。

代码定位：[`ContainerPurpose`](../../../crates/adapters/src/docker.rs#L8)、[`ContainerSpec`](../../../crates/adapters/src/docker.rs#L19)、[`ContainerReceipt`](../../../crates/adapters/src/docker.rs#L57)、[`DockerSandbox`](../../../crates/adapters/src/docker.rs#L144)、[`container_timestamp`](../../../crates/adapters/src/docker.rs#L71)、[`runtime_millis`](../../../crates/adapters/src/docker.rs#L81)。

关联阅读：[adapters/src/harness.rs](harness-and-protocols.md#file-src-harness-rs)、[adapters/src/tests.rs](engineering-resources.md#file-src-tests-rs)、[adapters/src/permissions.rs](credentials-and-permissions.md#file-src-permissions-rs)、[adapters/tests/docker_resources.rs](testing.md#file-tests-docker-resources-rs)。

<a id="file-src-tests-rs"></a>

## src/tests.rs：持久化测试作业与结构化 Node 结果

源码：[crates/adapters/src/tests.rs](../../../crates/adapters/src/tests.rs)。

CommandTests 是生产 Tests 端口。它以 operation UUID、请求摘要和冻结 Git 导出建立私有作业目录，原子保存请求和结果；后续 poll、stop、cleanup 与 reconcile 都找回原操作。

当前 NodeTest 配方使用固定镜像和内嵌 Node reporter，容器禁网、无模型凭据、代码只读，并保存有界 stdout/stderr。测试断言失败与运行环境失败根据实际结构化报告区分，不能从任意 stdout 中伪造一段“通过”就获得成功。

停止需要实际容器/写入撤销证明，结果终态与资源清理分别记录。Host 重启后若结果未知，先检查原容器和持久化文件，不用同一业务请求自动再跑一次。executor 和 reporter 的实际内容身份也进入回执，便于解释历史结果。

代码定位：[`CommandTests`](../../../crates/adapters/src/tests.rs#L17)、[`Job`](../../../crates/adapters/src/tests.rs#L12)、[`recipe`](../../../crates/adapters/src/tests.rs#L50)、[`write`](../../../crates/adapters/src/tests.rs#L58)。

关联阅读：[core/src/platform_tools.rs](../devdesk-core/manager-and-platform-tools.md#file-src-platform-tools-rs)、[domain/src/testing.rs](../devdesk-domain/work-and-materials.md#file-src-testing-rs)、[adapters/src/docker.rs](engineering-resources.md#file-src-docker-rs)、[adapters/tests/command_tests.rs](testing.md#file-tests-command-tests-rs)。

---

[代码手册总览](../overview.md) · [完整执行链](../execution-flows.md) · [本包概览](overview.md)

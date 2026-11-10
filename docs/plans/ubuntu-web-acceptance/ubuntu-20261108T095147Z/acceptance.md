# Ubuntu Web 实机验收：ubuntu-20261108T095147Z

> 历史记录：未提供链接的原始文件路径仅记录当时执行；相关中间产物或已删除源码不随当前工程提供。

日期：2026-11-08。执行者：Codex。本轮为实现者自检。**尚未通过整体验收**：离线基线、Linux 测试凭据、镜像登记和运行包基础检查通过；完整真实工程链在 Docker 嵌套沙箱阶段失败。没有将 macOS 历史、Sim 或缺少证据的步骤计为通过。

## 源码与环境

接续提交 `a620e92df36e536d66f74a67b31cd7e7c4670c48`，分支 main，初始工作区干净。未强制覆盖或切换用户源码；本轮修复在工作区，源文件摘要与补丁见 source-files.json（历史路径：`source-files.json`）、source-changes.patch（历史路径：`source-changes.patch`）；新增探针源码保存在仓库 tests/live。运行上下文见 context.json（历史路径：`context.json`），初始环境见 environment.log（历史路径：`environment.log`），用户安装后的版本和账户检查见 dependencies-after-install.log（历史路径：`dependencies-after-install.log`）。

| 项目 | 实际值 |
|---|---|
| OS / kernel / CPU | Ubuntu 24.04.5 LTS；7.0.0-42-generic；x86_64；glibc 2.39 |
| Rust / Cargo | 1.99.1，固定工具链 |
| Node / npm | v24.13.0 / 11.8.0 |
| Python / Git | 3.12.12 / 2.43.0 |
| gh / 账户 | 2.45.0；xfork24 |
| Docker Engine | 29.9.3，linux/amd64；当前用户在 docker 组 |
| Linux 凭据 | libsecret-tools 0.21.4；GNOME Keyring 46.1；实际用户 D-Bus、默认集合已解锁 |
| 本机 Codex | 0.167.0，真实 Linux ELF；原生独立安装，不是 npm 启动器 |
| 专用数据集 | `/home/hengliu/workspace/devdesk/data/ubuntu-20261108T095147Z` |
| 专用 Host | `http://127.0.0.1:51751`；重启归属见 Host 回执（历史路径：`host-after-oauth-fix-ownership.json`） |
| GitHub | 私有 `xfork24/devdesk-integration-test`，账户有 ADMIN 权限；只读规则检查（历史路径：`U1-github.json`） compatible=true |

GitHub 诊断时主干仍为 macOS 历史提交 `ee4785bd579f13c9479800a64ad53bfa83f04f5d`。这不是本轮产物；本轮未创建 PR 或推进远端。DevDesk 自身仓库没有作为被管理项目。

## 结果与命令

精确命令、起止时间和退出码见 commands.jsonl（历史路径：`commands.jsonl`）。早期失败保留独立文件，不覆盖为成功。

| 检查 | 退出码 / 结论 | 证据 |
|---|---|---|
| Web npm ci、Chromium | 安装完成；首次 `--with-deps` 因 sudo 密码退出 1，实际现有库支持 Chromium | 安装失败（历史路径：`playwright-install.log`）、浏览器安装（历史路径：`playwright-browser.log`） |
| 最终 `./scripts/check.sh` | 0；100 个 Rust 测试通过，5 个显式 live 用例 ignored；debug/release HTTP、复制后的 standalone、两轮各 6 个 Chromium 通过 | 最终回归（历史路径：`offline-final-with-oauth-fix.log`） |
| 固定 Codex/DSH 镜像构建 | 均 0；Ubuntu 本机 Linux/amd64 产物 | Codex（历史路径：`codex-image.log`）、DSH（历史路径：`dsh-image.log`）、inspect（历史路径：`images.json`） |
| Web 设置页 | 原生 Codex/镜像登记、默认 Codex→DSH→Codex 通过 | 设置（历史路径：`U1-settings.json`）、截图（历史路径：`U1-settings.png`）、镜像登记（历史路径：`U1-images.json`）、截图（历史路径：`U1-images.png`） |
| 宿主预检 | 正常桌面退出 0；失效 D-Bus 修复前错误地退出 0，修复后正确退出 1 | 正常（历史路径：`preflight.json`）、修复前（历史路径：`preflight-unavailable-before-fix.json`）、修复后（历史路径：`preflight-unavailable-after-fix.json`） |
| Linux 测试凭据 | 0；浏览器保存/更新、实际系统库读写、旧引用保留、删除、重启和不可用 D-Bus 错误通过 | 测试凭据（历史路径：`U1-linux-credentials.json`）、日志（历史路径：`linux-credentials.log`） |
| OAuth 恢复回归 | 0；原生请求超时/退出后释放、下一次显式尝试成功；重复 start 不重复派发 | 契约回归（历史路径：`oauth-contract.log`） |
| 真实 OAuth 启动恢复 | 0；真实 Codex 生成授权地址后故意丢弃本地 HTTP 响应；再次点击/刷新/弹窗阻止均恢复同一流程；原生取消有效 | 真实恢复探针（历史路径：`U1-oauth-recovery.json`） |
| DSH 原生讨论 | 0；两轮原生续聊 + 浏览器第三轮，复用同一 Session，随机 marker 保留 | 双轮报告（历史路径：`R3-dsh-application.json`） |
| 浏览器采纳 / 仓库连接 | 均 0；正式 Project Requirement、真实仓库与 Feature worktree 已建立 | 浏览器（历史路径：`R5-browser-proposal.json`）、截图（历史路径：`R5-browser-proposal.png`）、仓库（历史路径：`R4-repository.json`） |
| 讨论资源与产物 | 三个实际执行者 released，11 个已提交 Artifact 哈希通过 | 释放与哈希（历史路径：`U2-discussion-artifacts.json`） |
| 真实 Git | 0；2/2，真实 worktree、单父候选、CAS 和恢复 | Git（历史路径：`real-git.log`） |
| 真实 Node 测试 | 2/2；断言 Red、Green、语法/环境错误分类、停止和清理通过 | 资源日志（历史路径：`real-resources.log`） |
| 真实 Docker 资源 | 101；2/4，通过挂载拒绝/生命周期，两个嵌套沙箱失败 | 资源日志（历史路径：`real-docker.log`） |
| 全部组 `--mode full` | 1；real-git 通过，real-docker 失败，未进入模型/工程阶段 | 完整结果（历史路径：`live-check-full-git-docker-harness-github-web-workflow-recovery.json`）、控制台（历史路径：`full.log`） |
| 安装登记与公共层 | 部分通过；两镜像各并发登记四次返回原 ID，共享五层 | 独立登记证据（历史路径：`U5-registration-only.json`） |
| 完整安装复用探针 | 1；当前没有跨项目/角色 Session 引用，未满足前置证据 | 探针日志（历史路径：`installation-probe.log`）、未通过结果（历史路径：`R8-installations.json`） |
| `--mode verify-existing` | 1；尚无首次 full 的原生会话/交付证据；没有替代首次执行 | 复核结果（历史路径：`live-check-verify-existing-git-docker-harness-github-web-workflow-recovery.json`） |
| `./scripts/package.sh` | 0；Linux target、SHA256、包清单见专门回执 | 包回执（历史路径：`U6-package.json`） |
| 源码外运行包 | HTTP/SSE、Linux 测试凭据、已有项目重启查询和执行者释放通过；双 Harness 正式会话待验 | 最终包 HTTP（历史路径：`package-final-http.log`）、最终包凭据（历史路径：`U6-package-final-credentials.json`） |

预检只检查依赖/凭据能力，不能代表嵌套沙箱或 full 已通过。真实 OAuth 探针拦截官方网页导航，没有替用户完成官方登录，也未读取、保存或输出 OAuth token。用户已在设置页保存 DeepSeek key，正式读取检查及实际 DSH 模型讨论通过；实际 key 泄漏扫描（历史路径：`U1-secret-leak-scan.json`）在 93 个报告/SQLite/协议文件和两个归属进程 argv 中未发现秘密。Codex 官方 OAuth 尚待完成，见 最终状态观察（历史路径：`U1-final-credentials-observation.json`）。

## 实际阻塞与修复

Codex/DSH 两个真实资源测试均报 `bwrap: Failed to make / slave: Permission denied`。无模型凭据的独立复现记录了 `docker-default (enforce)`、非 root、cap-drop ALL、no-new-privileges、只读根文件系统、none 网络和仅 /state、/work 挂载；复现回执（历史路径：`U2-nested-sandbox-policy.json`）显示命令退出 1，所属诊断容器已删除。宿主 AppArmor 与 user namespace 设置见 只读诊断（历史路径：`U2-host-security.log`）。

Docker 默认 AppArmor 策略禁止 mount；结合实际进程策略和失败位置，**推断**该规则阻止了 bwrap 的挂载传播操作。没有取得本次 mount 的内核 audit 明细，因此不将推断写成直接审计结论。[Docker 官方 AppArmor 说明](https://docs.docker.com/engine/security/apparmor/)说明默认策略及挂载约束；[bubblewrap 原始说明](https://github.com/containers/bubblewrap/blob/main/README.md)说明其 mount namespace 边界。没有设置 privileged/unconfined、增加 capability、禁用宿主安全策略或在宿主执行模型修改工具。后续应在原隔离契约内解决兼容性，再重跑受影响步骤。

本轮修复：

- Git 导出契约测试误用了带 Docker 镜像查询的测试初始化，已分离；保留 首次失败（历史路径：`offline-regression.log`），最终工作区测试通过。
- Chromium 设置测试结束时，SSE 刷新仍在读取路由响应；已等待最终操作并 drain 拦截请求，消除 disposed response 竞态。
- doctor 原来仅检查 D-Bus 变量，失效地址也能通过；现在只读查询真实集合 Locked 元数据，三秒上界，不读取或解锁秘密。失效后端正确非零退出。
- 嵌套沙箱探针旧写死 macOS image ID，已解析本机固定版本/显式镜像并冻结 ID；记录实际策略、归属与清理；缺镜像明确失败且不自动拉取。
- 用户实测网页登录失败后 pending 卡住：修复前状态（历史路径：`U1-oauth-stuck-before-fix.json`）。登录请求出错时现在清除 pending 并释放专属认证子进程；本地 HTTP 响应丢失或页面刷新时，显式重试取回已有 loginId/授权地址，避免重复派发。弹窗阻止时保留官方链接，错误信息允许重试。最终回归与真实 Codex 启动/取消均验证；未将原生超时注入说成真实 OpenAI 故障。

Linux 凭据测试仅使用独立临时数据集和随机测试条目。实际集合锁定未实测，采用不可达 D-Bus 的受控后端不可用验证；未锁定或清除用户集合/旧 key。响应、SQLite 和浏览器持久存储未发现测试秘密；秘密经密码框和私有 stdin 传递。全部测试 Host 与条目有释放/删除回执。当前 51751 Host 作为用户登录入口保留运行，不能算已释放；本轮三个 DSH 讨论执行者已释放，尚未启动正式工程 Run。

本轮 DSH 项目 `218e2e9d-fe19-42f4-87fa-9c107906ac7c`、原生 Session `2a4faff0-bf5a-4087-b6f3-cc39fae0c752`、Requirement `881343d8-6d2f-4a32-8a53-b533a4bcd2ad`；各 Turn/Execution/container/Artifact 详见原生会话与释放报告。它们是讨论证据，未伪称为 Task/Run 或 PR 完成证据。

最终包 SHA256：`c01c37ad81d303715eabf64ba810d00afe2333e6ca75edfe4b994058e759ebea`。初次包/旧 Host 检查仍保留 `U6-package-before-oauth-fix.json`、`package-http.log`、`U6-package-credentials.json`；旧包另外保存在 `dist/ubuntu-20261108T095147Z/before-oauth-fix/`。最终包内 Host 包含 OAuth 修复，源码外 HTTP/SSE、Linux 凭据/项目恢复已重新通过。双 Harness 包内真实会话待验。

## A01–A22

| 编号 | Ubuntu 结果及证据边界 |
|---|---|
| A01 | 部分：实际设置页默认切换通过；历史 Session 绑定切换待验。 |
| A02 | 部分：DSH 实际连续三轮复用原生 Session；Codex、多项目和多角色会话隔离待验。 |
| A03 | 部分：实际并发登记/五层共享通过；跨项目角色引用缺失，完整探针失败；不同版本/摘要冲突仅 registry 契约测试。 |
| A04 | 契约：原生协议/失败处理回归通过；未实际触发模型限流、无效模型或公共网络故障；OAuth 超时注入单列。 |
| A05 | 实际通过：DSH 第三轮经 Chromium 讨论、提取并显式采纳 Project Requirement，Feature 创建前完成；R5 浏览器报告/截图。 |
| A06 | 部分：真实 Requirement、仓库与 Feature worktree 已建立；正式规划/Review/initial Snapshot/Task worktree 待验，规则由契约覆盖。 |
| A07 | 部分：DSH 实际两轮 client_message_id 重放返回原 Turn；采纳/陈旧/跨项目拒绝由契约覆盖。 |
| A08 | 部分/失败：调度/写许可契约通过；真实嵌套沙箱失败，双供应商工程并行待验。 |
| A09 | 部分：实际 Node Red/Green、环境/语法分类及回执通过；工程任务冻结版本的逐阶段证据待验。 |
| A10 | 契约：Review/Override 门禁回归通过；本轮无独立原生 Reviewer 决定。 |
| A11 | 部分：真实本地 Git 单父候选/CAS 通过；工程接纳待验。 |
| A12 | 待验：本轮未创建或合入 PR，不能引用旧 macOS PR 当本轮结果。 |
| A13 | 部分：实际本地 Git/CAS 测试通过；远端 lease/旧目标重验待验。 |
| A14 | 实际：专用仓库兼容诊断/连接通过；对 openai/codex 只读识别到规则并阻塞，未修改规则或 bypass；R4。 |
| A15 | 部分：实际 Node 停止及 Docker 生命周期通过；两供应商原生 interrupt/forced stop 待验。 |
| A16 | 待验：没有进行 consumed-input SIGKILL/恢复；契约回归不替代真实崩溃场景。 |
| A17 | 部分：本地 Git/实际 Docker 幂等通过；PR marker/落库前外部故障路径待验，Core 注入另算契约。 |
| A18 | 部分：实际容器停止/删除幂等通过；导出/清理故障只来自契约注入，未伪称真实 daemon 故障。 |
| A19 | 部分：实际 HTTP/SSE 项目隔离、分库、选择恢复通过；跨项目遗留模型执行者恢复待验。 |
| A20 | 契约/实际本地存储：工作区历史升级、分库和 Sim 回归通过；新 Ubuntu 数据集与旧 macOS 分离。 |
| A21 | 部分/失败：只读根、非 root、挂载与无 socket 的外层实际边界通过；内层写工具/隐藏状态测试因 bwrap 失败，未完成。 |
| A22 | 部分：Linux 独立包及系统凭据基础通过；Feature 收尾、最终 Snapshot、递归 Delivery、PR 及工程 Artifact 哈希待验。 |

## H / INV 映射

H01→固定镜像/原生 ELF/registry 实际登记；H02→DSH 三轮原生会话通过、Codex 待验；H03→JSONL 原生协议契约；H04→有界超时契约与真实 OAuth 恢复，工程取消/崩溃待验；H05→实际 Node/Git，模型工具与正式版本待验；H06→原生 resume/后继契约，DSH 实际多轮续用、Codex 待验；H07→调度契约，真实双 Task 待验；H08→工作区迁移/旧记录回归；H09→实际容器生命周期与公共安装，DSH 讨论封存/释放通过，正式工程封存待验。

INV01→追踪链契约，DSH 浏览器正式需求通过、后续工程链待验；INV02→实际 Git/worktree；INV03→registry/绑定契约，DSH 实际 Session 冻结、Codex 待验；INV04→DAG/额度/写许可契约；INV05→Run/Turn/Execution/Artifact 契约，正式外部对象待验；INV06→Review/Override 契约；INV07→本地单父/CAS 实际，PR/远端接纳待验；INV08→本地 Git 对象/Artifact 契约，工程 Snapshot 待验，Archive 环境重建仍延期；INV09→真实 Docker 释放与故障注入，双模型资源封存待验；INV10→真实 Web/Rust，本轮 native 客户端延期；INV11/INV12→Archive/data 搬迁按原范围延期；INV13→安装内容身份实际，后继/技术重试规则由契约覆盖。

## 继续执行

DeepSeek key 已配置并真实调用；用户继续在当前设置页完成 Codex 官方 OAuth；不用聊天传递秘密。先解决保持既定隔离要求的 Ubuntu bwrap 兼容性，再在新的报告子目录运行 full，保留本轮失败记录。随后执行跨项目/角色安装探针、verify-existing、双 Harness 包内真实会话与全量证据复核。只有 U0–U6 必要项全部通过，才可将 Ubuntu 标为已验收。

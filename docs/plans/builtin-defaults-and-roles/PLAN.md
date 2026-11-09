# 内建默认套件、Role 与全新数据初始化

日期：2026-11-09。状态：实现完成，验收结果见 EXECUTION.md。

交付一套开箱可用的 Global 默认配置：GPT-6.1 Sol High、当前本机及项目固定的 Codex CLI 0.167.0、Feature/Task Workflow、三种用途的 Spec、Standard、五个 Role 及其完整 Agent/AgentID、Skills、Tools、Permission 和 Linux Docker Sandbox。新 GitHub Project 自动采用套件，Feature 创建时冻结精确版本；显式自定义 Project 配置保持独立。

Role 是可新增、编辑、删除的版本化领域对象，名称及职责说明独立于 Agent 的模型与能力。RoleRevision 通过 role_ref 被 AgentRevision 引用；执行职责用于现有工程内核的需求分析、架构设计、开发实现、测试验证、独立评审门禁，不作为 Role 身份。自定义角色可以复用任一执行职责。

编辑 Role 追加新的 Role、Skill 和 Agent 修订，并创建具有新 ID 的 Agent 实例。修改当前默认 Role 时追加默认套件，采用内建默认的 Project 为后续 Feature 采用新配置；已创建 Feature 和旧实例不被重绑。删除保存停用事实，隐藏角色并阻止新组合使用；历史读取及已冻结 Feature 保留。当前默认流程所需角色的执行职责保持不变，名称与职责说明可修改。删除默认流程使用的角色前需要同执行职责的替代角色，系统会生成采用替代角色的新套件。

Codex Harness 固定发布版本和构建配方摘要；首次运行绑定实际核验的 Docker 镜像，物化记录不可变。镜像丢失时明确阻塞，不使用同名新镜像覆盖旧 Harness/AgentID。

按用户明确要求删除本工作区 data/ 中所有旧数据库、运行数据、备份和本机验收数据；只对属于该目录的 DevDesk Docker 容器做清理。重新初始化默认套件和全局库，不重建旧工程历史，不操作远端仓库或其他 Codex 数据目录。

# 源闭包与退役清单

此清单来自当前 manifest/defaults 的确切源引用。详细版本次保留；移除须另行执行。数据集中的历史源修订不删除。

精简版默认闭包 124 项；详细版闭包 146 项；共享 68 项；详细版使用而精简版不使用 78 项，其中可从当前出厂目录独立移除 66 项。其他工作流或资源仍引用的项目继续保留。

详细版 Feature 22 节点/8 阶段、Task 20 节点/8 阶段。与实施基线比较，执行正文完全一致，仅 Feature 嵌套 Task 引用推进到同内容的命名后继。

| 详细版专用源 | 出厂清理候选 | 当前反向引用 |
|---|---|---|
| `builtin-agent-sol-high-analyst:6` | 可移除 | 无 |
| `builtin-agent-sol-high-architect:6` | 可移除 | 无 |
| `builtin-agent-sol-high-developer:6` | 可移除 | 无 |
| `builtin-agent-sol-high-project-manager:13` | 可移除 | 无 |
| `builtin-agent-sol-high-reviewer:6` | 可移除 | 无 |
| `builtin-agent-sol-high-tester:6` | 可移除 | 无 |
| `builtin-capability-analyst-codex:5` | 可移除 | `builtin-agent-sol-high-analyst:6` |
| `builtin-capability-architect-codex:5` | 可移除 | `builtin-agent-sol-high-architect:6` |
| `builtin-capability-developer-codex:5` | 可移除 | `builtin-agent-sol-high-developer:6` |
| `builtin-capability-project-manager-codex:12` | 可移除 | `builtin-agent-sol-high-project-manager:13` |
| `builtin-capability-reviewer-codex:5` | 可移除 | `builtin-agent-sol-high-reviewer:6` |
| `builtin-capability-tester-codex:5` | 可移除 | `builtin-agent-sol-high-tester:6` |
| `builtin-delivery-acceptance-report:1` | 可移除 | `builtin-workflow-feature-sol-high:11`、`builtin-workflow-task-sol-high:10` |
| `builtin-delivery-baseline-doc:1` | 可移除 | `builtin-workflow-task-sol-high:10` |
| `builtin-delivery-code-candidate:1` | 可移除 | `builtin-workflow-feature-sol-high:11`、`builtin-workflow-task-sol-high:10` |
| `builtin-delivery-collection:2` | 可移除 | `builtin-workflow-feature-sol-high:11` |
| `builtin-delivery-design:3` | 可移除 | `builtin-workflow-feature-sol-high:11`、`builtin-workflow-task-sol-high:10` |
| `builtin-delivery-diff:2` | 可移除 | `builtin-workflow-feature-sol-high:11`、`builtin-workflow-task-sol-high:10` |
| `builtin-delivery-engineering-constraints:1` | 可移除 | `builtin-workflow-feature-sol-high:11` |
| `builtin-delivery-execution-evidence:2` | 可移除 | `builtin-workflow-feature-sol-high:11`、`builtin-workflow-task-sol-high:10` |
| `builtin-delivery-feature-report:1` | 可移除 | `builtin-workflow-feature-sol-high:11` |
| `builtin-delivery-implementation-doc:1` | 可移除 | `builtin-workflow-task-sol-high:10` |
| `builtin-delivery-publication-report:1` | 可移除 | `builtin-workflow-feature-sol-high:11`、`builtin-workflow-task-sol-high:10` |
| `builtin-delivery-task-completion-report:1` | 可移除 | `builtin-workflow-task-sol-high:10` |
| `builtin-delivery-task-delivery-report:1` | 可移除 | `builtin-workflow-task-sol-high:10` |
| `builtin-delivery-task-plan:3` | 可移除 | `builtin-workflow-feature-sol-high:11` |
| `builtin-delivery-test-recipe:2` | 可移除 | `builtin-workflow-feature-sol-high:11`、`builtin-workflow-task-sol-high:10` |
| `builtin-delivery-test-report:2` | 可移除 | `builtin-workflow-feature-sol-high:11`、`builtin-workflow-task-sol-high:10` |
| `builtin-permission-step-feature-sol-high-candidate_review:1` | 可移除 | `builtin-workflow-feature-sol-high:11` |
| `builtin-permission-step-feature-sol-high-closure:1` | 可移除 | `builtin-workflow-feature-sol-high:11` |
| `builtin-permission-step-feature-sol-high-closure_review:1` | 可移除 | `builtin-workflow-feature-sol-high:11` |
| `builtin-permission-step-feature-sol-high-design:1` | 可移除 | `builtin-workflow-feature-sol-high:11` |
| `builtin-permission-step-feature-sol-high-design_review:1` | 可移除 | `builtin-workflow-feature-sol-high:11` |
| `builtin-permission-step-feature-sol-high-plan_review:1` | 可移除 | `builtin-workflow-feature-sol-high:11` |
| `builtin-permission-step-feature-sol-high-planning:1` | 可移除 | `builtin-workflow-feature-sol-high:11` |
| `builtin-permission-step-feature-sol-high-release_recipe:1` | 可移除 | `builtin-workflow-feature-sol-high:11` |
| `builtin-permission-step-feature-sol-high-requirements:1` | 可移除 | `builtin-workflow-feature-sol-high:11` |
| `builtin-permission-step-feature-sol-high-requirements_review:1` | 可移除 | `builtin-workflow-feature-sol-high:11` |
| `builtin-permission-step-task-sol-high-candidate_review:1` | 可移除 | `builtin-workflow-task-sol-high:10` |
| `builtin-permission-step-task-sol-high-documentation:1` | 可移除 | `builtin-workflow-task-sol-high:10` |
| `builtin-permission-step-task-sol-high-implementation:1` | 可移除 | `builtin-workflow-task-sol-high:10` |
| `builtin-permission-step-task-sol-high-red_baseline:1` | 可移除 | `builtin-workflow-task-sol-high:10` |
| `builtin-permission-step-task-sol-high-review:1` | 可移除 | `builtin-workflow-task-sol-high:10` |
| `builtin-permission-step-task-sol-high-tests:1` | 可移除 | `builtin-workflow-task-sol-high:10` |
| `builtin-sandbox-docker:1` | 可移除 | `builtin-tool-tests-run:1` |
| `builtin-skill-architecture:2` | 仍被其他资源使用 | `builtin-capability-architect-codex:5`、`builtin-capability-architect-glm-dsh:2`、`builtin-capability-architect-minimax-dsh:2`、`builtin-workflow-feature-sol-high:11` |
| `builtin-skill-engineering-context:3` | 仍被其他资源使用 | `builtin-capability-analyst-codex:5`、`builtin-capability-analyst-glm-dsh:2`、`builtin-capability-analyst-minimax-dsh:2`、`builtin-capability-architect-codex:5`、`builtin-capability-architect-glm-dsh:2`、`builtin-capability-architect-minimax-dsh:2`、`builtin-capability-developer-codex:5`、`builtin-capability-developer-glm-dsh:2`、`builtin-capability-developer-minimax-dsh:2`、`builtin-capability-glm-dsh:2`、`builtin-capability-minimax-dsh:2`、`builtin-capability-project-manager-codex:12`、`builtin-capability-project-manager-glm-dsh:5`、`builtin-capability-project-manager-minimax-dsh:5`、`builtin-capability-reviewer-codex:5`、`builtin-capability-reviewer-glm-dsh:2`、`builtin-capability-reviewer-minimax-dsh:2`、`builtin-capability-sol-high-codex:4`、`builtin-capability-tester-codex:5`、`builtin-capability-tester-glm-dsh:2`、`builtin-capability-tester-minimax-dsh:2` |
| `builtin-skill-project-planning:2` | 仍被其他资源使用 | `builtin-capability-project-manager-codex:12`、`builtin-capability-project-manager-glm-dsh:5`、`builtin-capability-project-manager-minimax-dsh:5`、`builtin-workflow-feature-sol-high:11` |
| `builtin-skill-release-verification:2` | 仍被其他资源使用 | `builtin-capability-tester-codex:5`、`builtin-capability-tester-glm-dsh:2`、`builtin-capability-tester-minimax-dsh:2`、`builtin-workflow-feature-sol-high:11` |
| `builtin-skill-tdd-implementation:2` | 仍被其他资源使用 | `builtin-capability-developer-codex:5`、`builtin-capability-developer-glm-dsh:2`、`builtin-capability-developer-minimax-dsh:2`、`builtin-workflow-task-sol-high:10` |
| `builtin-skill-test-design:2` | 仍被其他资源使用 | `builtin-capability-tester-codex:5`、`builtin-capability-tester-glm-dsh:2`、`builtin-capability-tester-minimax-dsh:2`、`builtin-workflow-feature-sol-high:11`、`builtin-workflow-task-sol-high:10` |
| `builtin-skill-workflow-orchestration:5` | 仍被其他资源使用 | `builtin-capability-project-manager-codex:12`、`builtin-capability-project-manager-glm-dsh:5`、`builtin-capability-project-manager-minimax-dsh:5` |
| `builtin-spec-delivery-acceptance-report:1` | 可移除 | `builtin-delivery-acceptance-report:1` |
| `builtin-spec-delivery-baseline-doc:1` | 可移除 | `builtin-delivery-baseline-doc:1` |
| `builtin-spec-delivery-code-candidate:1` | 可移除 | `builtin-delivery-code-candidate:1` |
| `builtin-spec-delivery-engineering-constraints:1` | 仍被其他资源使用 | `builtin-capability-glm-dsh:2`、`builtin-capability-minimax-dsh:2`、`builtin-capability-sol-high-codex:4`、`builtin-delivery-engineering-constraints:1` |
| `builtin-spec-delivery-execution-evidence:1` | 可移除 | `builtin-delivery-execution-evidence:2` |
| `builtin-spec-delivery-feature-report:1` | 可移除 | `builtin-delivery-feature-report:1` |
| `builtin-spec-delivery-implementation-doc:1` | 可移除 | `builtin-delivery-implementation-doc:1` |
| `builtin-spec-delivery-publication-report:1` | 可移除 | `builtin-delivery-publication-report:1` |
| `builtin-spec-delivery-task-completion-report:1` | 可移除 | `builtin-delivery-task-completion-report:1` |
| `builtin-spec-delivery-task-delivery-report:1` | 可移除 | `builtin-delivery-task-delivery-report:1` |
| `builtin-spec-delivery-task-report:1` | 可移除 | `builtin-delivery-collection:2` |
| `builtin-spec-stage-architecture:2` | 仍被其他资源使用 | `builtin-capability-architect-codex:5`、`builtin-capability-architect-glm-dsh:2`、`builtin-capability-architect-minimax-dsh:2`、`builtin-delivery-design:3`、`builtin-workflow-feature-sol-high:11` |
| `builtin-spec-stage-closure:2` | 可移除 | `builtin-workflow-feature-sol-high:11` |
| `builtin-spec-stage-implementation:2` | 仍被其他资源使用 | `builtin-capability-developer-codex:5`、`builtin-capability-developer-glm-dsh:2`、`builtin-capability-developer-minimax-dsh:2`、`builtin-workflow-task-sol-high:10` |
| `builtin-spec-stage-project-planning:3` | 仍被其他资源使用 | `builtin-capability-project-manager-codex:12`、`builtin-capability-project-manager-glm-dsh:5`、`builtin-capability-project-manager-minimax-dsh:5`、`builtin-delivery-task-plan:3`、`builtin-workflow-feature-sol-high:11` |
| `builtin-spec-stage-release:2` | 可移除 | `builtin-workflow-feature-sol-high:11`、`builtin-workflow-task-sol-high:10` |
| `builtin-spec-stage-test-design:2` | 仍被其他资源使用 | `builtin-capability-tester-codex:5`、`builtin-capability-tester-glm-dsh:2`、`builtin-capability-tester-minimax-dsh:2`、`builtin-delivery-test-recipe:2` |
| `builtin-standard-engineering:9` | 可移除 | 无 |
| `builtin-tool-git-advance:1` | 可移除 | `builtin-workflow-feature-sol-high:11`、`builtin-workflow-task-sol-high:10` |
| `builtin-tool-git-candidate:1` | 可移除 | `builtin-workflow-feature-sol-high:11`、`builtin-workflow-task-sol-high:10` |
| `builtin-tool-git-publish:1` | 可移除 | `builtin-workflow-feature-sol-high:11`、`builtin-workflow-task-sol-high:10` |
| `builtin-tool-tasks-complete:1` | 可移除 | `builtin-workflow-task-sol-high:10` |
| `builtin-tool-tests-run:1` | 可移除 | `builtin-workflow-feature-sol-high:11`、`builtin-workflow-task-sol-high:10` |
| `builtin-tool-workspace-snapshot:2` | 可移除 | `builtin-workflow-feature-sol-high:11`、`builtin-workflow-task-sol-high:10` |
| `builtin-workflow-feature-sol-high:11` | 可移除 | `builtin-standard-engineering:9` |
| `builtin-workflow-task-sol-high:10` | 可移除 | `builtin-standard-engineering:9`、`builtin-workflow-feature-sol-high:11` |

清理每项时一并核对其 `_file` 正文。共享源及所有保留源的传递依赖不得删除。项目/工作/自定义引用不由出厂目录静态图推断；清理前读取各兼容数据集的 project_configuration、work_configuration、workflow_instance 与自定义 Source 引用，对当前使用选择显式调整或先完成运行，历史冻结源保留。

完整正向和反向引用、路径及正文表保存在临时证据目录 `catalog-audit.json`。隔离删减编译与运行结果见实施验收记录。

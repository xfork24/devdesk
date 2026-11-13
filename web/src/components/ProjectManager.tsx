import {
  ActionButton,
  ActionForm,
  ActionSelect,
  useActionFeedback,
} from "./ActionFeedback";
import { WorkflowController } from "./WorkflowController";
import { ProjectIntegrationBindings } from "./ProjectIntegrationBindings";
import { useCallback, useEffect, useState } from "react";
import {
  loadCatalog,
  RevisionSelect,
  useDraft,
  type Row,
  type Request,
} from "./configuration-ui";
export function ProjectManager({
  request,
  project,
  onOpen,
  refresh,
}: {
  request: Request;
  project?: Row;
  onOpen: (id: string) => Promise<unknown>;
  refresh: () => Promise<unknown>;
}) {
  const {
    error,
    setError,
    message,
    setMessage,
    capture: captureFeedback,
    beginFeedback,
    endFeedback,
  } = useActionFeedback();

  const [projects, setProjects] = useState<Row[]>([]),
    [records, setRecords] = useState<Row[]>([]),
    [configuration, setConfiguration] = useState<Row>(),
    [defaults, setDefaults] = useState<Row>(),
    [health, setHealth] = useState<Row>(),
    [requirements, setRequirements] = useState<Row>(),
    [busy, setBusy] = useState(false);
  const [orchestration, setOrchestration] = useState<Row>({ mode: "manual" });
  const [repository, setRepository] = useState(""),
    [mainline, setMainline] = useState("main"),
    [title, setTitle] = useState("");
  const [draft, setDraft] = useDraft(
    `devdesk:source-flat-v1:project:${project?.id || "global"}`,
    () => ({
      orchestrator_role_ref: "",
      permission_context_ref: "",
      workflow_ref: "",
      standard_ref: "",
      role_agent_bindings: {} as Record<string, string>,
      source_configuration_id: "",
    }),
  );
  const load = useCallback(async () => {
    const [workspace, records, configuration, preset] = await Promise.all([
      request("query.workspace", { limit: 100 }),
      loadCatalog(request, project),
      project
        ? request("query.project_configuration", { project_id: project.id })
        : Promise.resolve({}),
      request("query.configuration_defaults", {
        project_id: project?.id,
      }).catch(() => undefined),
    ]);
    setProjects(workspace.projects || []);
    setOrchestration(
      configuration.configuration?.data.orchestration || { mode: "manual" },
    );
    const ids = new Set(records.map((row: Row) => row.id));
    setRecords([
      ...records,
      ...(configuration.pinned_records ?? [])
        .filter((row: Row) => !ids.has(row.id))
        .map((row: Row) => ({ ...row, template_archived: true })),
    ]);
    setConfiguration(
      configuration.configuration
        ? {
            ...configuration.configuration,
            uses_archived_templates: configuration.uses_archived_templates,
          }
        : undefined,
    );
    setDefaults(preset);
    setHealth(workspace);
    if (configuration.configuration?.data.workflow_ref)
      setDraft((current) =>
        current.workflow_ref
          ? current
          : {
              orchestrator_role_ref:
                configuration.configuration.data.orchestrator_role_ref || "",
              permission_context_ref:
                configuration.configuration.data.permission_context_ref || "",
              workflow_ref: configuration.configuration.data.workflow_ref,
              standard_ref: configuration.configuration.data.standard_ref || "",
              role_agent_bindings:
                configuration.configuration.data.role_agent_bindings || {},
              source_configuration_id: configuration.configuration.id,
            },
      );
    else if (preset && !configuration.configuration)
      setDraft((current) =>
        current.workflow_ref
          ? current
          : {
              orchestrator_role_ref: preset.orchestrator_role_ref,
              permission_context_ref: preset.permission_context_ref,
              workflow_ref: preset.workflow_ref,
              standard_ref: "",
              role_agent_bindings: preset.role_agent_bindings,
              source_configuration_id: "",
            },
      );
  }, [request, project?.id, project?.revision]);
  useEffect(() => {
    let active = true;
    void load().catch((e) => {
      if (active) setError(String(e));
    });
    return () => {
      active = false;
    };
  }, [load]);
  useEffect(() => {
    let active = true;
    setRequirements(undefined);
    if (!project || !draft.workflow_ref) return;
    void request("query.workflow_requirements", {
      id: draft.workflow_ref,
      project_id: project.id,
      standard_ref: draft.standard_ref || null,
    })
      .then((result) => {
        if (active) {
          setRequirements(result);
          setError("");
        }
      })
      .catch((e) => {
        if (active) setError(String(e));
      });
    return () => {
      active = false;
    };
  }, [request, project?.id, draft.workflow_ref, draft.standard_ref]);
  const workflows = records.filter((r) => r.type === "workflow");
  const standards = records.filter(
    (r) => r.type === "standard" && Array.isArray(r.root_workflow_refs),
  );
  async function save() {
    captureFeedback();

    if (!project) return;
    (beginFeedback(), setBusy(true));
    setError("");
    try {
      const row = await request("project.configuration.update", {
        project_id: project.id,
        command_id: crypto.randomUUID(),
        target: project.id,
        expected_revision: project.revision,
        payload: {
          orchestration,
          orchestrator_role_ref: draft.orchestrator_role_ref || null,
          permission_context_ref: draft.permission_context_ref,
          workflow_ref: draft.workflow_ref,
          standard_ref: draft.standard_ref || null,
          role_agent_bindings: draft.role_agent_bindings,
          source_configuration_id:
            draft.source_configuration_id || configuration?.id,
        },
      });
      setConfiguration(row);
      setDraft((current) => ({ ...current, source_configuration_id: row.id }));
      await refresh();
      await load();
      setMessage("项目配置已保存；用于之后创建的 Feature。");
    } catch (e) {
      setError(String(e));
    } finally {
      (endFeedback(), setBusy(false));
    }
  }
  return (
    <>
      <section className="panel">
        <div className="panel-heading">
          <div>
            <h2>项目管理</h2>
            <p>
              先选择流程，再为流程所需 Role 绑定 Agent。规范来自 Workflow Step
              和 Delivery，保存时固定 Standard 快照。
            </p>
          </div>
        </div>
        {project && (
          <WorkflowController
            request={request}
            instance={{ id: project.id, data: { status: "active" } }}
            projectId={project.id}
            refresh={async () => {
              await refresh();
            }}
            workspaceOnly
          />
        )}
        {defaults && (
          <div className="notice">
            <strong>默认配置已准备好</strong>
            <p>
              分阶段 Feature / Task 开发流程 · 已绑定职责 Agent · GPT-6.1 Sol
              High · Codex · 默认 Tools / Skills · Docker 与默认 Permissions。
            </p>
            {project && (
              <ActionButton
                disabled={busy}
                onClick={() => {
                  captureFeedback();
                  return setDraft({
                    orchestrator_role_ref: defaults.orchestrator_role_ref,
                    permission_context_ref: defaults.permission_context_ref,
                    workflow_ref: defaults.workflow_ref,
                    standard_ref: "",
                    role_agent_bindings: defaults.role_agent_bindings,
                    source_configuration_id: configuration?.id || "",
                  });
                }}
              >
                采用默认配置
              </ActionButton>
            )}
          </div>
        )}
        {project && (
          <p>
            仓库：{health?.repository_ready ? "已接入" : "待接入"} · 项目配置：
            {health?.configuration_ready ? "已保存且有效" : "待保存或调整"} ·
            运行环境：首次执行时核验安装与凭据
          </p>
        )}
        {health?.configuration_status?.issues?.map((issue: string) => (
          <p key={issue} className="muted">
            {issue}
          </p>
        ))}
        <div className="configuration-list project-list">
          {projects.map((row) => (
            <ActionButton
              key={row.id}
              className={row.id === project?.id ? "selected" : ""}
              disabled={busy}
              onClick={() => {
                captureFeedback();

                (beginFeedback(), setBusy(true));
                void onOpen(row.id)
                  .catch((e) => setError(String(e)))
                  .finally(() => (endFeedback(), setBusy(false)));
              }}
            >
              <strong>{row.title}</strong>
              <span>
                {row.workspace_definition?.repository || "待配置仓库"}
              </span>
            </ActionButton>
          ))}
        </div>
        <details>
          <summary>集成新项目</summary>
          <ActionForm
            onSubmit={async (e) => {
              captureFeedback();

              e.preventDefault();
              (beginFeedback(), setBusy(true));
              setError("");
              try {
                const row = await request("project.open_github", {
                  command_id: crypto.randomUUID(),
                  payload: { repository, mainline, title },
                });
                await onOpen(row.id);
                setRepository("");
                setTitle("");
                setMessage("项目已登记，请完成下面的 Workflow 与角色配置。");
              } catch (e) {
                setError(String(e));
              } finally {
                (endFeedback(), setBusy(false));
              }
            }}
          >
            <div className="form-grid">
              <label>
                GitHub 仓库
                <input
                  required
                  value={repository}
                  onChange={(e) => {
                    captureFeedback();
                    return setRepository(e.target.value);
                  }}
                  placeholder="owner/repo"
                />
              </label>
              <label>
                主分支
                <input
                  required
                  value={mainline}
                  onChange={(e) => {
                    captureFeedback();
                    return setMainline(e.target.value);
                  }}
                />
              </label>
              <label>
                项目名称
                <input
                  value={title}
                  onChange={(e) => {
                    captureFeedback();
                    return setTitle(e.target.value);
                  }}
                />
              </label>
            </div>
            <ActionButton className="primary" disabled={busy}>
              登记项目并配置
            </ActionButton>
          </ActionForm>
        </details>
      </section>
      {project && (
        <section className="panel" aria-label="项目配置">
          <div className="panel-heading">
            <div>
              <h2>{project.title} · 配置</h2>
              {configuration?.uses_archived_templates && (
                <p className="muted">
                  来自重置前定义：当前项目继续使用已保存的精确版本。重新选择并保存后采用新的定义。
                </p>
              )}
              <p>
                {configuration?.data.workflow_ref
                  ? `已保存 v${configuration.version}`
                  : "待配置：选择 Workflow 并完成所有 Role 的 Agent 绑定"}
              </p>
            </div>
          </div>
          <ActionForm
            onSubmit={(e) => {
              captureFeedback();

              e.preventDefault();
              void save();
            }}
          >
            <div className="form-grid">
              <RevisionSelect
                label="项目 Workflow"
                records={
                  draft.standard_ref
                    ? workflows.filter((r) =>
                        standards
                          .find((s) => s.id === draft.standard_ref)
                          ?.workflow_refs?.includes(r.id),
                      )
                    : workflows
                }
                value={draft.workflow_ref}
                required
                onChange={(value) => {
                  captureFeedback();
                  return setDraft({
                    ...draft,
                    workflow_ref: value,
                    standard_ref: "",
                  });
                }}
              />
              <label>
                Standard 规范快照
                <ActionSelect
                  value={draft.standard_ref}
                  onChange={(e) => {
                    captureFeedback();
                    const standard = standards.find(
                      (r) => r.id === e.target.value,
                    );
                    return setDraft({
                      ...draft,
                      standard_ref: e.target.value,
                      workflow_ref: standard?.root_workflow_refs.includes(
                        draft.workflow_ref,
                      )
                        ? draft.workflow_ref
                        : standard?.root_workflow_refs[0] || draft.workflow_ref,
                    });
                  }}
                >
                  <option value="">从所选 Workflow 自动生成快照</option>
                  {standards.map((r) => (
                    <option key={r.id} value={r.id}>
                      {r.name} · v{r.version}
                    </option>
                  ))}
                </ActionSelect>
              </label>
            </div>
            <p>
              <a
                href={`?tab=workflows&config_tab=workflow&project_id=${encodeURIComponent(project.id)}`}
              >
                新建或编辑 Workflow
              </a>{" "}
              ·{" "}
              <a
                href={`?tab=workflows&config_tab=spec&project_id=${encodeURIComponent(project.id)}`}
              >
                管理 Spec 与 Standard
              </a>{" "}
              ·{" "}
              <a
                href={`?tab=actors&project_id=${encodeURIComponent(project.id)}&config_tab=agent`}
              >
                新建或编辑 Agent
              </a>
            </p>
            <label>
              默认调度模式
              <ActionSelect
                aria-label="默认调度模式"
                value={orchestration.mode}
                onChange={(e) =>
                  setOrchestration({ ...orchestration, mode: e.target.value })
                }
              >
                <option value="manual">人工控制</option>
                <option value="rust">Rust 自动调度</option>
                <option value="agent">Agent 调度</option>
              </ActionSelect>
            </label>
            {orchestration.mode === "rust" && (
              <RevisionSelect
                label="自动调度策略"
                records={records.filter(
                  (r) => r.type === "orchestration_policy",
                )}
                value={orchestration.policy_ref || ""}
                onChange={(policy_ref) =>
                  setOrchestration({ ...orchestration, policy_ref })
                }
              />
            )}
            <RevisionSelect
              label="执行授权"
              records={records.filter((r) => r.type === "permission_context")}
              value={draft.permission_context_ref}
              onChange={(permission_context_ref) =>
                setDraft({ ...draft, permission_context_ref })
              }
            />
            <label>
              编排角色
              <select
                aria-label="编排角色"
                value={draft.orchestrator_role_ref}
                onChange={(e) =>
                  setDraft({
                    ...draft,
                    orchestrator_role_ref: e.target.value,
                  })
                }
              >
                <option value="">仅手动操作工作流</option>
                {records
                  .filter((r) => r.type === "role")
                  .map((r) => (
                    <option key={r.id} value={r.id}>
                      {r.name}
                    </option>
                  ))}
              </select>
            </label>
            {draft.orchestrator_role_ref && (
              <RevisionSelect
                label="编排 Agent"
                records={records.filter(
                  (r) =>
                    r.type === "agent" &&
                    records.some(
                      (role) =>
                        role.type === "role" &&
                        role.id === r.role_ref &&
                        role.id === draft.orchestrator_role_ref,
                    ),
                )}
                value={
                  draft.role_agent_bindings[draft.orchestrator_role_ref] || ""
                }
                onChange={(value) =>
                  setDraft({
                    ...draft,
                    role_agent_bindings: {
                      ...draft.role_agent_bindings,
                      [draft.orchestrator_role_ref]: value,
                    },
                  })
                }
              />
            )}
            {requirements && (
              <>
                <h3>流程所需 Role → Agent</h3>
                <div
                  className="project-role-bindings"
                  role="group"
                  aria-label="流程所需 Role → Agent"
                >
                  {requirements.roles.map((item: Row) => {
                    const agents = records.filter(
                      (r) =>
                        r.type === "agent" &&
                        records.some(
                          (role) =>
                            role.type === "role" &&
                            role.id === r.role_ref &&
                            role.id === item.role_id,
                        ),
                    );
                    return (
                      <RevisionSelect
                        key={item.role_id}
                        label={`${item.role.name} 的 Agent`}
                        records={agents}
                        required
                        value={draft.role_agent_bindings[item.role_id] || ""}
                        onChange={(value) => {
                          captureFeedback();
                          return setDraft({
                            ...draft,
                            role_agent_bindings: {
                              ...draft.role_agent_bindings,
                              [item.role_id]: value,
                            },
                          });
                        }}
                      />
                    );
                  })}
                </div>
                <p className="notice">
                  已解析 {requirements.specification_view.length}{" "}
                  个规范节点（包含子流程）。
                </p>
                <p className="muted">
                  {draft.standard_ref
                    ? "已固定所选规范快照；实际验收以执行证据为准。"
                    : "保存项目配置时自动生成不可变规范快照。"}
                </p>
                <details>
                  <summary>查看规范审计快照</summary>
                  {requirements.specification_view.map((binding: Row) => (
                    <div className="document-row" key={binding.path}>
                      <strong>{binding.path}</strong>
                      <span>
                        {binding.spec_refs
                          .map(
                            (id: string) =>
                              records.find((r) => r.id === id)?.name ||
                              "固定 Spec",
                          )
                          .join("、") || "系统动作"}
                      </span>
                    </div>
                  ))}
                </details>
              </>
            )}
            <ActionButton className="primary" disabled={busy || !requirements}>
              保存项目配置
            </ActionButton>
          </ActionForm>
        </section>
      )}
      {error && (
        <p role="alert" className="error">
          {error}
        </p>
      )}
      {message && (
        <p role="status" className="notice">
          {message}
        </p>
      )}
      {project && (
        <ProjectIntegrationBindings
          request={request}
          project={project}
          catalog={records}
          agents={Object.values(draft.role_agent_bindings)}
        />
      )}
    </>
  );
}

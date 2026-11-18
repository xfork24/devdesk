import { displayText } from "../display-text";
import { useReadRequest, ignoreCancelledRefresh } from "./use-read-request";
import { ActionButton, ActionForm, ActionSelect } from "./ActionFeedback";
import { useActionFeedback } from "./action-feedback";
import { ProjectIntegrationBindings } from "./ProjectIntegrationBindings";
import { useCallback, useEffect, useRef, useState } from "react";
import {
  decodeConfigurationIndex,
  loadCatalog,
  useDraft,
} from "./configuration-state";
import { RevisionSelect } from "./configuration-ui";
import { type Row, type Request } from "./rpc-types";
export function ProjectManager({
  request: parentRequest,
  project,
  onOpen,
  refresh,
  bootstrap,
}: {
  request: Request;
  project?: Row;
  onOpen: (id: string) => Promise<unknown>;
  refresh: () => Promise<unknown>;
  bootstrap?: Row;
}) {
  const request = useReadRequest(parentRequest);
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
    [requirements, setRequirements] = useState<Row>(),
    [busy, setBusy] = useState(false);
  const [orchestration, setOrchestration] = useState<Row>({ mode: "agent" });
  const orchestrationEdited = useRef(false);
  const [preview, setPreview] = useState<Row>();
  const [previewError, setPreviewError] = useState("");
  const [baseline, setBaseline] = useState<Row>();
  const health = bootstrap;
  const [overrideKind, setOverrideKind] = useState("");
  const [overrideSource, setOverrideSource] = useState("");
  const [overrideReplacement, setOverrideReplacement] = useState("");
  const [repository, setRepository] = useState(""),
    [mainline, setMainline] = useState("main"),
    [title, setTitle] = useState("");
  const [draft, setDraft] = useDraft(
    `devdesk:standard-project-v2:${project?.id || "global"}`,
    () => ({
      orchestrator_role_ref: "",
      harness_permission_ref: "",
      sandbox_ref: "",
      sandbox_permission_ref: "",
      workflow_ref: "",
      standard_ref: "",
      standard_overrides: [] as {
        source_ref: string;
        replacement_ref: string;
      }[],
      role_agent_bindings: {} as Record<string, string>,
      source_configuration_id: "",
    }),
  );
  const loadGeneration = useRef(0);
  const previewScope = useRef("");
  const load = useCallback(async () => {
    const ticket = ++loadGeneration.current;
    const [workspace, records, configuration, preset] = await Promise.all([
      request("query.project_directory", { limit: 100, project_offset: 0 }),
      loadCatalog(request, project?.id),
      project?.id
        ? request("query.project_configuration", {
            project_id: project?.id,
            view: "selection",
          })
        : Promise.resolve({}),
      request("query.configuration_defaults", {
        project_id: project?.id,
        view: "selection",
      }).catch(() => undefined),
    ]);
    if (ticket !== loadGeneration.current) return;
    const saved = configuration.standard;
    if (ticket !== loadGeneration.current) return;
    setProjects(workspace.projects || []);
    if (!orchestrationEdited.current)
      setOrchestration(
        configuration.configuration?.data.orchestration ||
          preset?.orchestration || { mode: "agent" },
      );
    const harnessProfile =
      configuration.configuration?.data.harness_permission_ref ||
      preset?.harness_permission_ref ||
      "";
    const ids = new Set(records.map((row: Row) => row.id));
    setRecords([
      ...records,
      ...(configuration.pinned_records ?? [])
        .filter((row: Row) => !ids.has(row.id))
        .map((row: Row) => decodeConfigurationIndex(row)),
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
    if (configuration.configuration?.data.workflow_ref)
      setDraft((current) =>
        current.workflow_ref
          ? {
              ...current,
              harness_permission_ref:
                current.harness_permission_ref || harnessProfile,
            }
          : {
              sandbox_ref: configuration.configuration.data.sandbox_ref || "",
              sandbox_permission_ref:
                configuration.configuration.data.sandbox_permission_ref || "",
              orchestrator_role_ref:
                configuration.configuration.data.orchestrator_role_ref || "",
              harness_permission_ref: harnessProfile,
              workflow_ref: configuration.configuration.data.workflow_ref,
              standard_ref:
                saved?.provenance?.base_standard_ref ||
                configuration.configuration.data.standard_ref ||
                "",
              standard_overrides: saved?.provenance?.overrides || [],
              role_agent_bindings:
                configuration.configuration.data.role_agent_bindings || {},
              source_configuration_id: configuration.configuration.id,
            },
      );
    else if (preset && !configuration.configuration)
      setDraft((current) =>
        current.workflow_ref
          ? {
              ...current,
              harness_permission_ref:
                current.harness_permission_ref || harnessProfile,
            }
          : {
              sandbox_ref: preset.sandbox_ref || "",
              sandbox_permission_ref: preset.sandbox_permission_ref || "",
              orchestrator_role_ref: preset.orchestrator_role_ref,
              harness_permission_ref: harnessProfile,
              workflow_ref: preset.workflow_ref,
              standard_ref: preset.standard_ref,
              standard_overrides: [],
              role_agent_bindings: preset.role_agent_bindings,
              source_configuration_id: "",
            },
      );
  }, [request, project?.id, setDraft]);
  useEffect(() => {
    let active = true;
    void load().catch((e) => {
      if (active) setError(String(e));
    });
    return () => {
      active = false;
      loadGeneration.current += 1;
    };
  }, [load, project?.revision, setError]);
  useEffect(() => {
    let active = true;
    const scope = JSON.stringify([
      project?.id,
      draft.standard_ref,
      draft.standard_overrides,
      draft.harness_permission_ref,
    ]);
    if (previewScope.current !== scope) {
      previewScope.current = scope;
      setRequirements(undefined);
      setPreview(undefined);
      setBaseline(undefined);
    }
    setPreviewError("");
    if (!project?.id || !draft.standard_ref) return;
    void request("query.project_standard_preview", {
      project_id: project.id,
      standard_ref: draft.standard_ref,
      standard_overrides: draft.standard_overrides,
      harness_permission_ref: draft.harness_permission_ref || undefined,
      view: "summary",
    })
      .then((result) => {
        if (active) {
          setBaseline(result.baseline);
          setPreview(result);
          setRequirements(result.requirements);
        }
      })
      .catch((e) => {
        if (active) setPreviewError(String(e));
      });
    return () => {
      active = false;
    };
  }, [
    request,
    project?.id,
    draft.standard_ref,
    draft.standard_overrides,
    draft.harness_permission_ref,
  ]);
  const managerRole = records.find(
    (r) => r.type === "role" && r.id === defaults?.orchestrator_role_ref,
  );
  const standards = records.filter(
    (r) => r.type === "standard" && Array.isArray(r.root_workflow_refs),
  );
  async function save() {
    captureFeedback();

    if (!project) return;
    loadGeneration.current += 1;
    beginFeedback();
    setBusy(true);
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
          harness_permission_ref: draft.harness_permission_ref,
          sandbox_ref: draft.sandbox_ref || undefined,
          sandbox_permission_ref: draft.sandbox_permission_ref || undefined,
          standard_ref: draft.standard_ref,
          standard_overrides: draft.standard_overrides,
          standard_preview_hash: preview?.preview_hash,
          role_agent_bindings: draft.role_agent_bindings,
          source_configuration_id:
            draft.source_configuration_id || configuration?.id,
        },
      });
      setConfiguration(row);
      setDraft((current) => ({ ...current, source_configuration_id: row.id }));
      await refresh();
      await load().catch(ignoreCancelledRefresh);
      setMessage(
        draft.standard_overrides.length
          ? "项目专属规范快照已保存；已有 Feature 的冻结版本保持不变。"
          : "项目配置已保存，直接使用所选 Standard；已有 Feature 的冻结版本保持不变。",
      );
    } catch (e) {
      setError(String(e));
    } finally {
      endFeedback();
      setBusy(false);
    }
  }
  return (
    <>
      <section className="panel">
        <div className="panel-heading">
          <div>
            <h2>项目管理</h2>
            <p>
              选择基础 Standard，按需覆盖规范版本，再为流程所需角色绑定 Agent。
              有覆盖时保存为项目专属快照；流程请在“执行与交付”启动。
            </p>
          </div>
        </div>
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
                  setOverrideKind("");
                  setOverrideSource("");
                  setOverrideReplacement("");
                  orchestrationEdited.current = true;
                  setOrchestration(defaults.orchestration || { mode: "agent" });
                  return setDraft({
                    sandbox_ref: defaults.sandbox_ref || "",
                    sandbox_permission_ref:
                      defaults.sandbox_permission_ref || "",
                    orchestrator_role_ref: defaults.orchestrator_role_ref,
                    harness_permission_ref:
                      defaults.harness_permission_ref || "",
                    workflow_ref: defaults.workflow_ref,
                    standard_ref: defaults.standard_ref,
                    standard_overrides: [],
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
            仓库：{displayText(health?.repository_ready ? "已接入" : "待接入")}{" "}
            · 项目配置：
            {displayText(
              health?.configuration_ready ? "已保存且有效" : "待保存或调整",
            )}{" "}
            · 运行环境：首次执行时核验安装与凭据
          </p>
        )}
        {health?.configuration_status?.issues?.map((issue: string) => (
          <p key={issue} className="muted">
            {displayText(issue)}
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

                beginFeedback();
                setBusy(true);
                void onOpen(row.id)
                  .catch((e) => setError(String(e)))
                  .finally(() => {
                    endFeedback();
                    setBusy(false);
                  });
              }}
            >
              <strong>{displayText(row.title)}</strong>
              <span>
                {displayText(
                  row.workspace_definition?.repository || "待配置仓库",
                )}
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
              beginFeedback();
              setBusy(true);
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
                endFeedback();
                setBusy(false);
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
              <h2>{displayText(project.title)} · 配置</h2>
              {configuration?.uses_archived_templates && (
                <p className="muted">
                  来自重置前定义：当前项目继续使用已保存的精确版本。重新选择并保存后采用新的定义。
                </p>
              )}
              <p>
                {displayText(
                  configuration?.data.workflow_ref
                    ? `已保存 v${configuration.version}`
                    : "待配置：选择 Standard 并完成角色的 Agent 绑定",
                )}
              </p>
            </div>
          </div>
          <ActionForm
            className="project-configuration-form"
            onSubmit={(e) => {
              captureFeedback();

              e.preventDefault();
              void save();
            }}
          >
            <RevisionSelect
              label="基础 Standard"
              records={standards}
              value={draft.standard_ref}
              required
              onChange={(standard_ref) => {
                captureFeedback();
                setOverrideKind("");
                setOverrideSource("");
                setOverrideReplacement("");
                setDraft({ ...draft, standard_ref, standard_overrides: [] });
              }}
            />
            <details>
              <summary>
                项目覆盖（{draft.standard_overrides.length} 项）
              </summary>
              <p>
                替换某个 Spec、Delivery 或 Workflow 的确切版本；该修订在此
                Standard 中的所有引用同步生效。
              </p>
              <div className="project-configuration-row">
                <label>
                  规范类型
                  <ActionSelect
                    aria-label="规范类型"
                    value={overrideKind}
                    onChange={(e) => {
                      setOverrideKind(e.target.value);
                      setOverrideSource("");
                      setOverrideReplacement("");
                    }}
                  >
                    <option value="">请选择</option>
                    <option value="workflow">Workflow</option>
                    <option value="spec">Spec</option>
                    <option value="delivery_type">Delivery</option>
                  </ActionSelect>
                </label>
                <label>
                  要覆盖的规范项
                  <ActionSelect
                    aria-label="要覆盖的规范项"
                    value={overrideSource}
                    disabled={!overrideKind}
                    onChange={(e) => {
                      setOverrideSource(e.target.value);
                      setOverrideReplacement("");
                    }}
                  >
                    <option value="">请选择</option>
                    {(baseline?.nodes || [])
                      .filter((node: Row) => node.kind === overrideKind)
                      .map((node: Row) => (
                        <option key={node.reference} value={node.reference}>
                          {displayText(node.name)} ·{" "}
                          {displayText(node.reference)}
                        </option>
                      ))}
                  </ActionSelect>
                </label>
                <RevisionSelect
                  label="替换为"
                  records={records.filter(
                    (r) => r.type === overrideKind && r.id !== overrideSource,
                  )}
                  value={overrideReplacement}
                  onChange={setOverrideReplacement}
                />
              </div>
              <ActionButton
                type="button"
                disabled={!overrideSource || !overrideReplacement}
                onClick={() => {
                  captureFeedback();
                  setDraft({
                    ...draft,
                    standard_overrides: [
                      ...draft.standard_overrides.filter(
                        (o) => o.source_ref !== overrideSource,
                      ),
                      {
                        source_ref: overrideSource,
                        replacement_ref: overrideReplacement,
                      },
                    ],
                  });
                  setOverrideSource("");
                  setOverrideReplacement("");
                }}
              >
                添加覆盖
              </ActionButton>
              {draft.standard_overrides.map((item) => (
                <div key={item.source_ref} className="setting-row">
                  <span>
                    {displayText(item.source_ref)} →{" "}
                    {displayText(item.replacement_ref)}
                  </span>
                  <ActionButton
                    type="button"
                    aria-label={displayText(`移除覆盖 ${item.source_ref}`)}
                    onClick={() => {
                      captureFeedback();
                      setDraft({
                        ...draft,
                        standard_overrides: draft.standard_overrides.filter(
                          (o) => o.source_ref !== item.source_ref,
                        ),
                      });
                    }}
                  >
                    移除覆盖
                  </ActionButton>
                </div>
              ))}
            </details>
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
            <div className="project-configuration-row project-orchestration-fields">
              <label>
                默认调度模式
                <ActionSelect
                  aria-label="默认调度模式"
                  value={orchestration.mode}
                  onChange={(e) => {
                    orchestrationEdited.current = true;
                    setOrchestration({
                      ...orchestration,
                      mode: e.target.value,
                    });
                  }}
                >
                  <option value="manual">人工控制</option>
                  <option value="rust">Rust 自动调度</option>
                  <option value="agent">Agent 调度</option>
                </ActionSelect>
              </label>
              <RevisionSelect
                label="Harness Profile"
                records={records.filter(
                  (r) =>
                    r.type === "permission_profile" &&
                    r.data.target_kind === "harness",
                )}
                value={draft.harness_permission_ref}
                onChange={(harness_permission_ref) =>
                  setDraft({
                    ...draft,
                    harness_permission_ref: harness_permission_ref,
                  })
                }
              />
              <RevisionSelect
                label="Sandbox"
                records={records.filter((r) => r.type === "sandbox")}
                value={draft.sandbox_ref}
                onChange={(sandbox_ref) => setDraft({ ...draft, sandbox_ref })}
              />
              <RevisionSelect
                label="Sandbox Profile"
                records={records.filter(
                  (r) =>
                    r.type === "permission_profile" &&
                    r.data.target_kind === "sandbox",
                )}
                value={draft.sandbox_permission_ref}
                onChange={(sandbox_permission_ref) =>
                  setDraft({ ...draft, sandbox_permission_ref })
                }
              />
              <div className="project-configuration-row project-orchestration-agent-fields">
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
                      .filter(
                        (r) =>
                          r.type === "role" &&
                          managerRole &&
                          r.definition_id === managerRole.definition_id,
                      )
                      .map((r) => (
                        <option key={r.id} value={r.id}>
                          {displayText(r.name)}
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
                      draft.role_agent_bindings[draft.orchestrator_role_ref] ||
                      ""
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
              </div>
            </div>
            <p className="muted">
              项目管理与流程编排仅由项目经理角色承担；其他角色负责流程中的专业步骤。
            </p>
            {requirements && (
              <>
                <h3>流程所需 Role → Agent</h3>
                <div
                  className="project-configuration-row project-role-bindings"
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
                  已解析 {displayText(requirements.specification_view.length)}{" "}
                  个规范节点（包含子流程）。
                </p>
                <p className="muted">
                  保存后固定项目规范、来源和差异；实际验收以执行证据为准。
                </p>
              </>
            )}
            <ActionButton
              className="primary"
              disabled={busy || !requirements || !preview}
            >
              保存项目配置
            </ActionButton>
          </ActionForm>
        </section>
      )}
      {(error || previewError) && (
        <p role="alert" className="error">
          {displayText(error || previewError)}
        </p>
      )}
      {message && (
        <p role="status" className="notice">
          {displayText(message)}
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

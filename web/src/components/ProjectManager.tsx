import { ProjectIntegrationBindings } from "./ProjectIntegrationBindings";
import { useCallback, useEffect, useState } from "react";
import {
  latest,
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
  const [projects, setProjects] = useState<Row[]>([]),
    [records, setRecords] = useState<Row[]>([]),
    [configuration, setConfiguration] = useState<Row>(),
    [defaults, setDefaults] = useState<Row>(),
    [health, setHealth] = useState<Row>(),
    [requirements, setRequirements] = useState<Row>(),
    [error, setError] = useState(""),
    [busy, setBusy] = useState(false),
    [message, setMessage] = useState("");
  const [repository, setRepository] = useState(""),
    [mainline, setMainline] = useState("main"),
    [title, setTitle] = useState("");
  const [draft, setDraft] = useDraft(
    `devdesk:v2:project:${project?.id || "global"}`,
    () => ({
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
    setRecords(records);
    setConfiguration(configuration.configuration);
    setDefaults(preset);
    setHealth(workspace);
    if (configuration.configuration?.data.schema_version === 2)
      setDraft((current) =>
        current.workflow_ref
          ? current
          : {
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
  const workflows = records.filter(
    (r) =>
      r.kind === "workflow" &&
      r.data.schema_version >= 2 &&
      r.data.process_kind === "feature",
  );
  const standards = records.filter(
    (r) => r.kind === "standard" && Array.isArray(r.data.baseline_rules),
  );
  async function save() {
    if (!project) return;
    setBusy(true);
    setError("");
    try {
      const row = await request("project.configuration.update", {
        project_id: project.id,
        command_id: crypto.randomUUID(),
        target: project.id,
        expected_revision: project.revision,
        payload: {
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
              先选择流程，再为流程所需 Role 绑定 Agent。规范来自 Workflow
              Step，Standard 可选。
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
              <button
                disabled={busy}
                onClick={() =>
                  setDraft({
                    workflow_ref: defaults.workflow_ref,
                    standard_ref: "",
                    role_agent_bindings: defaults.role_agent_bindings,
                    source_configuration_id: configuration?.id || "",
                  })
                }
              >
                采用默认配置
              </button>
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
            <button
              key={row.id}
              className={row.id === project?.id ? "selected" : ""}
              disabled={busy}
              onClick={() => {
                setBusy(true);
                void onOpen(row.id)
                  .catch((e) => setError(String(e)))
                  .finally(() => setBusy(false));
              }}
            >
              <strong>{row.title}</strong>
              <span>
                {row.workspace_definition?.repository || "待配置仓库"}
              </span>
            </button>
          ))}
        </div>
        <details>
          <summary>集成新项目</summary>
          <form
            onSubmit={async (e) => {
              e.preventDefault();
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
                  onChange={(e) => setRepository(e.target.value)}
                  placeholder="owner/repo"
                />
              </label>
              <label>
                主分支
                <input
                  required
                  value={mainline}
                  onChange={(e) => setMainline(e.target.value)}
                />
              </label>
              <label>
                项目名称
                <input
                  value={title}
                  onChange={(e) => setTitle(e.target.value)}
                />
              </label>
            </div>
            <button className="primary" disabled={busy}>
              登记项目并配置
            </button>
          </form>
        </details>
      </section>
      {project && (
        <section className="panel" aria-label="项目配置">
          <div className="panel-heading">
            <div>
              <h2>{project.title} · 配置</h2>
              <p>
                {configuration?.data.schema_version === 2
                  ? `已保存 v${configuration.version}`
                  : "待配置：选择 Workflow 并完成所有 Role 的 Agent 绑定"}
              </p>
            </div>
          </div>
          <form
            onSubmit={(e) => {
              e.preventDefault();
              void save();
            }}
          >
            <div className="form-grid">
              <RevisionSelect
                label="项目 Workflow"
                records={workflows}
                value={draft.workflow_ref}
                required
                onChange={(value) =>
                  setDraft({ ...draft, workflow_ref: value })
                }
              />
              <label>
                Standard（可选质量基线）
                <select
                  value={draft.standard_ref}
                  onChange={(e) =>
                    setDraft({ ...draft, standard_ref: e.target.value })
                  }
                >
                  <option value="">
                    不选基线 · 仍执行 Step Spec 和工程门禁
                  </option>
                  {latest(standards).map((r) => (
                    <option key={r.id} value={r.id}>
                      {r.name} · v{r.version}
                    </option>
                  ))}
                </select>
              </label>
            </div>
            <p>
              <a
                href={`?tab=workflows&project_id=${encodeURIComponent(project.id)}`}
              >
                新建或编辑 Workflow
              </a>{" "}
              ·{" "}
              <a
                href={`?tab=specifications&project_id=${encodeURIComponent(project.id)}`}
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
            {requirements && (
              <>
                <h3>流程所需 Role → Agent</h3>
                {requirements.roles.map((item: Row) => {
                  const agents = records.filter(
                    (r) =>
                      r.kind === "agent" &&
                      records.some(
                        (role) =>
                          role.kind === "role" &&
                          role.id === r.data.role_ref &&
                          role.definition_id === item.role_id,
                      ),
                  );
                  return (
                    <RevisionSelect
                      key={item.role_id}
                      label={`${item.role.name} 的 Agent`}
                      records={agents}
                      required
                      value={draft.role_agent_bindings[item.role_id] || ""}
                      onChange={(value) =>
                        setDraft({
                          ...draft,
                          role_agent_bindings: {
                            ...draft.role_agent_bindings,
                            [item.role_id]: value,
                          },
                        })
                      }
                    />
                  );
                })}
                <p className="notice">
                  已解析 {requirements.specification_snapshot.length}{" "}
                  个规范节点（包含子流程）。
                </p>
                {requirements.baseline_check ? (
                  <div
                    className={
                      requirements.baseline_check.valid ? "notice" : "error"
                    }
                    role="status"
                  >
                    {requirements.baseline_check.valid
                      ? "基线覆盖检查通过"
                      : "基线覆盖不完整"}
                    <ul>
                      {requirements.baseline_check.errors.map(
                        (error: string) => (
                          <li key={error}>{error}</li>
                        ),
                      )}
                    </ul>
                  </div>
                ) : (
                  <p className="muted">
                    未选 Standard：规范仍按每个 Step 的显式 Spec 执行。
                  </p>
                )}
                <details>
                  <summary>查看规范审计快照</summary>
                  {requirements.specification_snapshot.map((binding: Row) => (
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
            <button
              className="primary"
              disabled={
                busy ||
                !requirements ||
                requirements.baseline_check?.valid === false
              }
            >
              保存项目配置
            </button>
          </form>
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

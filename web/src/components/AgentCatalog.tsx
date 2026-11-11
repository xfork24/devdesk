import {
  ActionButton,
  ActionForm,
  ActionSelect,
  useActionFeedback,
} from "./ActionFeedback";
import { ModelFields, type ModelProvider } from "./DshProviderSettings";
import {
  useCallback,
  useEffect,
  useRef,
  useState,
  type ReactNode,
} from "react";
import { RoleManager } from "./RoleManager";
import { JsonField } from "./WorkflowDeliveries";
import { WorkflowSpecifications } from "./WorkflowSpecifications";
import { isProjectRuntimeInstallation } from "./runtime-installations";
import {
  latest,
  loadCatalog,
  templateSourceLabel,
  loadIntegrationRecords,
  MarkdownField,
  MarkdownPreview,
  RevisionSelect,
  useDraft,
  useGroupTab,
  type Row,
  type Request,
} from "./configuration-ui";
const labels: Record<string, string> = {
  role: "Role",
  capability: "Capability",
  agent: "Agent",
  model: "Model",
  harness: "Harness",
  tool: "Tool",
  skill: "Skill",
  spec: "Spec",
  standard: "Standard",
  delivery_type: "Delivery",
  permission_context: "执行授权",
};
function initial(kind: string): Row {
  switch (kind) {
    case "agent":
      return {
        role_ref: "",
        capability_ref: "",
      };
    case "permission_context":
      return {
        sandbox_ref: "",
        harness: { mode: "follow_target" },
        sandbox: { mode: "follow_target" },
        step_grants: [],
        control_policy: {
          allowed_operations: [],
          human_operations: [],
          waivable_requirements: [],
          allow_task_revision: false,
          max_turns_per_wakeup: 8,
          max_tool_calls_per_turn: 64,
          timeout_seconds: 600,
        },
      };
    case "capability":
      return { model_ref: "", harness_ref: "", tools: [], skills: [] };
    case "model":
      return {
        provider: "openai",
        model: "",
        parameters: {},
        capabilities: [],
      };
    case "harness":
      return {
        provider: "codex",
        runtime_version: "",
        capabilities: [],
        model_providers: ["openai"],
      };
    case "tool":
      return {
        qualified_name: "",
        implementation: "harness.native",
        input_schema: { type: "object" },
        output_schema: { type: "object" },
        capabilities: [],
      };
    case "standard":
      return { schema_version: 3, body: "", baseline_rules: [] };
    case "delivery_type":
      return { format: "markdown", spec_refs: [], content_schema: null };
    case "spec":
      return { schema_version: 3, body: "", checks: [] };
    default:
      return { body: "" };
  }
}
export function AgentCatalog({
  request,
  project,
  installations,
  group = "actors",
}: {
  request: Request;
  project?: Row;
  installations: Row[];
  group?: "actors" | "foundations" | "specifications";
}) {
  const kinds =
    group === "actors"
      ? ["role", "capability", "agent"]
      : group === "foundations"
        ? ["model", "harness"]
        : ["spec", "standard"];
  const [kind, setKind] = useGroupTab(group, kinds);
  const [revision, setRevision] = useState(0);
  return (
    <>
      <section className="panel">
        <div className="panel-heading">
          <h2>
            {group === "actors"
              ? "角色与 Agent"
              : group === "foundations"
                ? "基础组件"
                : "规范"}
          </h2>
        </div>
        <div
          className="configuration-tabs"
          role="tablist"
          aria-label="配置类别"
        >
          {kinds.map((id) => (
            <ActionButton
              key={id}
              role="tab"
              aria-selected={id === kind}
              onClick={() => setKind(id)}
            >
              {labels[id]}
            </ActionButton>
          ))}
        </div>
      </section>
      {kind === "role" ? (
        <RoleManager
          key={`role:${project?.id}`}
          request={request}
          project={project}
          onChanged={() => setRevision((v) => v + 1)}
        />
      ) : (
        <ResourceEditor
          key={`${kind}:${project?.id}`}
          kind={kind}
          request={request}
          project={project}
          installations={installations}
          revision={revision}
        />
      )}
    </>
  );
}
export function ResourceEditor({
  kind,
  request,
  project,
  installations,
  revision,
  additionalEditor,
}: {
  kind: string;
  request: Request;
  project?: Row;
  installations: Row[];
  revision: number;
  additionalEditor?: { label: string; content: ReactNode };
}) {
  const [showAdditionalEditor, setShowAdditionalEditor] = useState(false);
  const {
    error,
    setError,
    message,
    setMessage,
    capture: captureFeedback,
    beginFeedback,
    endFeedback,
  } = useActionFeedback();

  const [providers, setProviders] = useState<ModelProvider[]>([]);
  const [runtimeOptions, setRuntimeOptions] = useState<Row>({
    recipes: [],
    installations: [],
  });
  const [records, setRecords] = useState<Row[]>([]),
    [busy, setBusy] = useState(false),
    [integrations, setIntegrations] = useState<Row[]>([]);
  const [draft, setDraft] = useDraft(
    `devdesk:source-v2:resource:${project?.id || "global"}:${kind}`,
    () => ({ selected: "", name: "", scope: "global", data: initial(kind) }),
  );
  const load = useCallback(async () => {
    const [records, modelProviders, integrationResources, runtimeOptions] =
      await Promise.all([
        loadCatalog(request, project),
        request("query.model_providers"),
        ["capability", "agent", "skill"].includes(kind)
          ? loadIntegrationRecords(request, project)
          : Promise.resolve([]),
        request("query.runtime_options"),
      ]);
    setRecords(records);
    setRuntimeOptions(runtimeOptions);
    setIntegrations(integrationResources);
    setProviders(modelProviders.providers || []);
    if (kind === "tool")
      setDraft((current) =>
        current.data.implementation === "experience.lookup"
          ? { selected: "", name: "", scope: "global", data: initial(kind) }
          : current,
      );
  }, [request, project?.id, revision]);
  useEffect(() => {
    let active = true;
    void load().catch((e) => {
      if (active) setError(String(e));
    });
    return () => {
      active = false;
    };
  }, [load]);
  const old = records.find((r) => r.id === draft.selected),
    data = draft.data;
  const projectInstallations = installations.filter(
    (r) => r.key?.provider === data.provider && isProjectRuntimeInstallation(r),
  );
  const patch = (value: Row) =>
    setDraft((current) => ({
      ...current,
      data: { ...current.data, ...value },
    }));
  const runtimeVersions: Row[] = (runtimeOptions.recipes || []).filter(
    (r: Row) => r.provider === data.provider,
  );
  const options = (family: string) => records.filter((r) => r.kind === family);
  const select = (
    label: string,
    family: string,
    key: string,
    multiple = false,
    required = false,
  ) => (
    <RevisionSelect
      label={label}
      records={options(family)}
      value={
        family === "tool"
          ? (data[key] || []).filter((id: string) =>
              options("tool").some((row) => row.id === id),
            )
          : data[key] || (multiple ? [] : "")
      }
      multiple={multiple}
      required={required}
      onChange={(value) => {
        captureFeedback();
        return patch({ [key]: value });
      }}
    />
  );
  const field = (label: string, key: string, required = false) => (
    <label>
      {label}
      <input
        required={required}
        value={data[key] || ""}
        onChange={(e) => {
          captureFeedback();
          return patch({ [key]: e.target.value });
        }}
      />
    </label>
  );
  const choose = (row: Row) => {
    setShowAdditionalEditor(false);
    let value = structuredClone(row.data);
    if (kind === "agent") {
      value = {
        role_ref: value.role_ref || "",
        capability_ref: value.capability_ref || "",
      };
    }
    setDraft({
      selected: row.id,
      name: row.name,
      scope: row.scope.kind,
      data: value,
    });
    const params = new URLSearchParams(location.search);
    params.set("config_object", row.id);
    history.replaceState(null, "", `?${params}`);
    setMessage("");
  };
  const routeConsumed = useRef(false);
  useEffect(() => {
    if (routeConsumed.current || !records.length) return;
    const selected = new URLSearchParams(location.search).get("config_object");
    if (selected) {
      const instance = records.find(
        (r) => r.id === selected && r.kind === "agent_instance",
      );
      const revision =
        kind === "agent" && instance
          ? instance.data.agent_revision_id
          : selected;
      const row = records.find((r) => r.id === revision && r.kind === kind);
      if (row) {
        routeConsumed.current = true;
        if (draft.selected !== row.id) choose(row);
      }
    }
  }, [records, kind]);
  async function save(remove = false) {
    captureFeedback();

    (beginFeedback(), setBusy(true));
    setError("");
    try {
      let payload = structuredClone(data);
      if (kind === "model" && !payload.model)
        throw new Error("请先从模型目录中选择模型标识");
      if (kind === "capability")
        payload.tools = (payload.tools || []).filter((id: string) =>
          options("tool").some((row) => row.id === id),
        );
      if (kind === "agent" && !payload.instructions)
        delete payload.instructions;
      if (
        kind === "harness" &&
        !runtimeVersions.some((r) => r.version === payload.runtime_version)
      )
        throw new Error("请选择已支持的固定运行版本");
      if (kind === "harness" && !payload.recipe_digest)
        delete payload.recipe_digest;
      if (kind === "tool") {
        if (payload.implementation === "harness.native") {
          // Native contracts belong to the Harness; keep existing metadata for history.
          payload.input_schema = old?.data.input_schema ?? { type: "object" };
          payload.output_schema = old?.data.output_schema ?? { type: "object" };
        } else {
          if (payload.input_text !== undefined)
            payload.input_schema = JSON.parse(payload.input_text);
          if (payload.output_text !== undefined)
            payload.output_schema = JSON.parse(payload.output_text);
        }
        delete payload.input_text;
        delete payload.output_text;
      }
      const row = await request(
        remove
          ? "configuration.retire"
          : `${kind}.${old ? "revise" : "create"}`,
        {
          project_id: project?.id,
          command_id: crypto.randomUUID(),
          target: old?.id,
          expected_revision: old?.revision,
          payload: remove
            ? { reason: "用户删除定义" }
            : {
                name: draft.name,
                scope:
                  draft.scope === "project"
                    ? { kind: "project", project_id: project!.id }
                    : { kind: "global" },
                data: payload,
              },
        },
      );
      if (remove)
        setDraft({
          selected: "",
          name: "",
          scope: "global",
          data: initial(kind),
        });
      else {
        setDraft((current) => ({ ...current, selected: row.id }));
        const params = new URLSearchParams(location.search);
        params.set("config_object", row.id);
        history.replaceState(null, "", `?${params}`);
      }
      await load();
      setMessage(
        remove ? "已停用；历史保留。" : "新修订已保存，已有使用方保持原引用。",
      );
    } catch (e) {
      setError(String(e));
    } finally {
      (endFeedback(), setBusy(false));
    }
  }
  const role = records.find((r) => r.id === data.role_ref),
    capability = records.find((r) => r.id === data.capability_ref);
  const model = records.find((r) => r.id === data.model_ref),
    harness = records.find((r) => r.id === data.harness_ref);
  return (
    <section className="panel" aria-label={`${labels[kind]} 管理`}>
      <div className="panel-heading">
        <div>
          <h2>{labels[kind]} 管理</h2>
          {templateSourceLabel(old) && (
            <span className="muted">{templateSourceLabel(old)}</span>
          )}
          <p>
            {kind === "agent"
              ? "给一个 Role 与一个 Capability 的组合命名。"
              : kind === "capability"
                ? "选择可复用的 Model、Harness、Tools 和 Skills。"
                : kind === "standard"
                  ? "可选质量基线检查规范覆盖，不改变 Step 的 Spec。"
                  : "修订独立保存，修改后由使用方显式采用。"}
          </p>
        </div>
      </div>
      <div className="configuration-layout">
        <div className="configuration-list">
          <ActionButton
            className={
              !old && !showAdditionalEditor
                ? "new-resource selected"
                : "new-resource"
            }
            onClick={() => {
              captureFeedback();

              setShowAdditionalEditor(false);
              const params = new URLSearchParams(location.search);
              params.delete("config_object");
              history.replaceState(null, "", `?${params}`);
              setDraft({
                selected: "",
                name: "",
                scope: "global",
                data: initial(kind),
              });
            }}
          >
            新建 {labels[kind]}
          </ActionButton>
          {additionalEditor && (
            <ActionButton
              className={showAdditionalEditor ? "selected" : ""}
              onClick={() => {
                setShowAdditionalEditor(true);
                setError("");
                setMessage("");
              }}
            >
              {additionalEditor.label}
            </ActionButton>
          )}

          {latest(records.filter((r) => r.kind === kind)).map((row) => (
            <ActionButton
              key={row.id}
              className={
                !showAdditionalEditor && draft.selected === row.id
                  ? "selected"
                  : ""
              }
              onClick={() => {
                captureFeedback();
                return choose(row);
              }}
            >
              <strong>{row.name}</strong>
              <span>
                v{row.version} ·{" "}
                {row.scope.kind === "global" ? "Global" : "项目"}
              </span>
            </ActionButton>
          ))}
        </div>
        {additionalEditor && (
          <div hidden={!showAdditionalEditor}>{additionalEditor.content}</div>
        )}
        <ActionForm
          hidden={showAdditionalEditor}
          onSubmit={(e) => {
            captureFeedback();

            e.preventDefault();
            void save();
          }}
        >
          <div className="form-grid">
            <label>
              名称
              <input
                required
                value={draft.name}
                onChange={(e) => {
                  captureFeedback();
                  return setDraft({ ...draft, name: e.target.value });
                }}
              />
            </label>
            <label>
              定义范围
              <ActionSelect
                disabled={!!old}
                value={draft.scope}
                onChange={(e) => {
                  captureFeedback();
                  return setDraft({ ...draft, scope: e.target.value });
                }}
              >
                <option value="global">Global</option>
                {project && <option value="project">当前项目</option>}
              </ActionSelect>
            </label>
            {kind === "permission_context" && (
              <JsonField
                label="执行授权配置"
                value={data}
                onChange={(data) => setDraft({ ...draft, data })}
              />
            )}
            {kind === "agent" && (
              <>
                {select("Role 精确版本", "role", "role_ref", false, true)}
                {select(
                  "Capability 精确版本",
                  "capability",
                  "capability_ref",
                  false,
                  true,
                )}
                {role && (
                  <div className="notice">
                    <strong>{role.name}</strong>
                    <MarkdownPreview text={role.data.instructions} />
                  </div>
                )}
                {capability && (
                  <div className="notice">
                    Model：
                    {records.find((r) => r.id === capability.data.model_ref)
                      ?.name || "固定修订"}{" "}
                    · Harness：
                    {records.find((r) => r.id === capability.data.harness_ref)
                      ?.name || "固定修订"}{" "}
                    · Tools{" "}
                    {
                      capability.data.tools.filter((id: string) =>
                        options("tool").some((row) => row.id === id),
                      ).length
                    }{" "}
                    · Skills {capability.data.skills.length}
                  </div>
                )}
                <p className="muted full-width">
                  职责来自 Role，方法与工具来自 Capability；本次执行授权由
                  Permission 单独管理。
                </p>
              </>
            )}
            {kind === "capability" && (
              <>
                {select("Model 精确版本", "model", "model_ref", false, true)}
                {select(
                  "Harness 精确版本",
                  "harness",
                  "harness_ref",
                  false,
                  true,
                )}
                {select("Tools", "tool", "tools", true, true)}
                {select("Skills", "skill", "skills", true)}
                {(data.skills || []).map((id: string) => {
                  const pkg = integrations.find(
                    (r) => r.kind === "skill_package" && r.definition_id === id,
                  );
                  if (!pkg) return null;
                  const missing = (pkg.data.requires_tools || []).filter(
                    (id: string) =>
                      !(data.tools || []).some(
                        (tool: string) =>
                          tool === id ||
                          records.find((r) => r.id === tool)?.definition_id ===
                            id,
                      ),
                  );
                  return (
                    <p key={id} className={missing.length ? "error" : "notice"}>
                      {records.find((r) => r.id === id)?.name}：
                      {missing.length
                        ? `缺少工具依赖 ${missing.join(", ")}`
                        : "工具依赖已满足"}
                    </p>
                  );
                })}
                {(data.tools || []).filter(
                  (id: string) =>
                    records.find((r) => r.id === id)?.data.schema_version === 2,
                ).length > 0 && (
                  <p>
                    外部工具还需要项目选择接入绑定；保存 Capability
                    不会自动授权账户。
                    <a href="?tab=components&config_tab=http">管理工具接入</a>
                  </p>
                )}

                {model && harness && (
                  <p className="notice">
                    {harness.data.model_providers?.length &&
                    !harness.data.model_providers.includes(model.data.provider)
                      ? "此 Harness 不支持选定模型后端，保存时会拒绝。"
                      : `${model.name} / ${harness.name}`}
                  </p>
                )}
              </>
            )}
            {kind === "model" && (
              <>
                <label>
                  模型后端
                  <ActionSelect
                    aria-label="模型后端"
                    value={data.provider}
                    onChange={(e) => {
                      captureFeedback();
                      return patch({
                        provider: e.target.value,
                        parameters: {},
                        endpoint: undefined,
                        context_window: undefined,
                        max_output_tokens: undefined,
                        model: "",
                      });
                    }}
                  >
                    {providers.map((p) => (
                      <option key={p.id} value={p.id}>
                        {p.label}
                      </option>
                    ))}
                  </ActionSelect>
                </label>
                <ModelFields
                  key={data.provider}
                  request={request}
                  data={{ ...data, provider: String(data.provider) }}
                  patch={patch}
                  providers={providers}
                />
                {data.provider === "openai" ? (
                  <label>
                    推理强度
                    <ActionSelect
                      value={data.parameters?.reasoning_effort || ""}
                      onChange={(e) => {
                        captureFeedback();
                        return patch({
                          parameters: e.target.value
                            ? { reasoning_effort: e.target.value }
                            : {},
                        });
                      }}
                    >
                      <option value="">模型默认</option>
                      {["minimal", "low", "medium", "high", "xhigh", "max"].map(
                        (v) => (
                          <option key={v}>{v}</option>
                        ),
                      )}
                    </ActionSelect>
                  </label>
                ) : (
                  <label>
                    最大输出 Tokens
                    <input
                      type="number"
                      min={data.provider === "deepseek" ? 1024 : 1}
                      max={data.max_output_tokens || 1000000}
                      value={data.parameters?.max_tokens || ""}
                      onChange={(e) => {
                        captureFeedback();
                        return patch({
                          parameters: e.target.value
                            ? { max_tokens: Number(e.target.value) }
                            : {},
                        });
                      }}
                    />
                  </label>
                )}
              </>
            )}
            {kind === "harness" && (
              <>
                <label>
                  运行时后端
                  <ActionSelect
                    value={data.provider}
                    onChange={(e) => {
                      captureFeedback();
                      return patch({
                        provider: e.target.value,
                        runtime_version:
                          runtimeOptions.recipes.find(
                            (r: Row) => r.provider === e.target.value,
                          )?.version || "",
                        recipe_digest: runtimeOptions.recipes.find(
                          (r: Row) => r.provider === e.target.value,
                        )?.recipe_digest,
                        model_providers: [
                          ...(e.target.value === "codex"
                            ? ["openai"]
                            : providers
                                .filter((p) => p.harness === "dsh")
                                .map((p) => p.id)),
                        ],
                      });
                    }}
                  >
                    <option value="codex">Codex</option>
                    <option value="dsh">DSH</option>
                  </ActionSelect>
                </label>
                <label>
                  固定运行版本
                  <ActionSelect
                    aria-label="固定运行版本"
                    required
                    value={data.runtime_version || ""}
                    onChange={(e) => {
                      const selected = runtimeVersions.find(
                        (r) => r.version === e.target.value,
                      );
                      patch({
                        runtime_version: e.target.value,
                        recipe_digest: selected?.recipe_digest,
                      });
                    }}
                  >
                    <option value="" disabled>
                      请选择支持的运行版本
                    </option>
                    {data.runtime_version &&
                      !runtimeVersions.some(
                        (r) => r.version === data.runtime_version,
                      ) && (
                        <option value={data.runtime_version} disabled>
                          {data.runtime_version}（历史版本，当前不支持新建）
                        </option>
                      )}
                    {runtimeVersions.map((r) => (
                      <option key={r.version} value={r.version}>
                        {r.version} ·{" "}
                        {r.installed ? "已安装的固定构建" : "受支持构建配方"}
                      </option>
                    ))}
                  </ActionSelect>
                </label>
                <fieldset className="model-provider-options">
                  <legend>支持的模型服务商</legend>
                  {providers
                    .filter((p) => p.harness === data.provider)
                    .map((p) => (
                      <label key={p.id} className="model-provider-choice">
                        <input
                          type="checkbox"
                          checked={(data.model_providers || []).includes(p.id)}
                          onChange={(event) => {
                            captureFeedback();
                            return patch({
                              model_providers: event.target.checked
                                ? [...(data.model_providers || []), p.id]
                                : (data.model_providers || []).filter(
                                    (id: string) => id !== p.id,
                                  ),
                            });
                          }}
                        />
                        <span>{p.label}</span>
                      </label>
                    ))}
                </fieldset>
                {data.recipe_digest && (
                  <p className="muted full-width">
                    构建配方摘要：<code>{data.recipe_digest}</code>
                  </p>
                )}
                <div className="full-width">
                  <h3>已登记安装</h3>
                  {projectInstallations.length === 0 && (
                    <p className="muted">尚未准备项目执行运行时</p>
                  )}
                  {projectInstallations.map((r) => (
                    <p key={r.id}>
                      {r.key.version} · {r.key.os}/{r.key.arch}
                    </p>
                  ))}
                  {old && (
                    <ActionButton
                      type="button"
                      disabled={busy}
                      onClick={async () => {
                        captureFeedback();

                        (beginFeedback(), setBusy(true));
                        try {
                          await request("runtime.prepare_harness", {
                            id: old.id,
                            project_id: project?.id,
                          });
                          setMessage("所选 Harness 已核验并准备。");
                        } catch (e) {
                          setError(String(e));
                        } finally {
                          (endFeedback(), setBusy(false));
                        }
                      }}
                    >
                      准备所选 Harness
                    </ActionButton>
                  )}
                </div>
              </>
            )}
            {kind === "skill" &&
              old &&
              (() => {
                const pkg = integrations.find(
                  (r) =>
                    r.kind === "skill_package" && r.definition_id === old.id,
                );
                return pkg ? (
                  <div className="notice">
                    <p>来源：{pkg.data.source}</p>
                    <p>
                      静态材料：{pkg.data.files.length} 个 ·{" "}
                      {pkg.data.scripts_disabled ? "脚本执行未启用" : "无脚本"}
                    </p>
                    <p>
                      必需工具：
                      {pkg.data.requires_tools
                        .map(
                          (id: string) =>
                            records.find((r) => r.id === id)?.name || id,
                        )
                        .join(", ") || "无"}
                    </p>
                    {pkg.data.files.map((file: Row) => (
                      <details key={file.path}>
                        <summary>{file.path}</summary>
                        {file.path.endsWith(".md") ? (
                          <MarkdownPreview text={file.content} />
                        ) : (
                          <pre>{file.content}</pre>
                        )}
                      </details>
                    ))}
                    <ActionButton
                      type="button"
                      onClick={async () => {
                        captureFeedback();

                        try {
                          const value = await request("skill.export", {
                            id: old.id,
                            project_id: project?.id,
                          });
                          const url = URL.createObjectURL(
                            new Blob([JSON.stringify(value, null, 2)], {
                              type: "application/json",
                            }),
                          );
                          const link = document.createElement("a");
                          link.href = url;
                          link.download = `${old.name}.skill.json`;
                          link.click();
                          setTimeout(() => URL.revokeObjectURL(url), 1000);
                        } catch (e) {
                          setError(String(e));
                        }
                      }}
                    >
                      导出 Skill 包
                    </ActionButton>
                  </div>
                ) : null;
              })()}
            {kind === "tool" && (
              <>
                {field("工具标识", "qualified_name", true)}
                <label>
                  工具实现
                  <ActionSelect
                    value={data.implementation}
                    onChange={(e) => {
                      captureFeedback();
                      return patch({ implementation: e.target.value });
                    }}
                  >
                    <option value="harness.native">Harness 原生工具</option>
                  </ActionSelect>
                </label>
                {data.implementation === "harness.native" ? (
                  <p
                    className="notice full-width"
                    role="note"
                    aria-label="原生工具组说明"
                  >
                    使用所选 Harness 自带的原生工具，无需手工配置输入或输出
                    Schema。 此处不通过 JSON 增删工具，可访问范围以实际生效的
                    Permission 为准。
                  </p>
                ) : (
                  <>
                    <label>
                      输入 JSON Schema
                      <textarea
                        value={
                          data.input_text ??
                          JSON.stringify(data.input_schema, null, 2)
                        }
                        onChange={(e) => {
                          captureFeedback();
                          return patch({ input_text: e.target.value });
                        }}
                      />
                    </label>
                    <label>
                      输出 JSON Schema
                      <textarea
                        value={
                          data.output_text ??
                          JSON.stringify(data.output_schema, null, 2)
                        }
                        onChange={(e) => {
                          captureFeedback();
                          return patch({ output_text: e.target.value });
                        }}
                      />
                    </label>
                  </>
                )}
              </>
            )}
            {kind === "delivery_type" && (
              <>
                <RevisionSelect
                  label="内容规范"
                  multiple
                  records={records.filter((r) => r.kind === "spec")}
                  value={data.spec_refs || []}
                  onChange={(spec_refs) => patch({ spec_refs })}
                />
                <label>
                  交付物格式
                  <ActionSelect
                    value={data.format}
                    onChange={(event) => patch({ format: event.target.value })}
                  >
                    <option value="markdown">文档</option>
                    <option value="diff">Diff</option>
                    <option value="test_report">Test Report</option>
                    <option value="json">JSON</option>
                    <option value="text">文本</option>
                  </ActionSelect>
                </label>
              </>
            )}
            {["skill", "spec", "standard"].includes(kind) && (
              <MarkdownField
                key={draft.selected || "new"}
                label="正文"
                value={data.body || ""}
                required
                onChange={(body) => {
                  captureFeedback();
                  return patch({ body });
                }}
              />
            )}
            {["spec", "standard", "delivery_type"].includes(kind) ? (
              <WorkflowSpecifications
                kind={kind}
                data={data}
                records={records}
                onChange={patch}
              />
            ) : null}
          </div>
          <div className="actions">
            <ActionButton
              className="primary"
              disabled={
                busy ||
                !draft.name.trim() ||
                (["skill", "spec", "standard"].includes(kind) &&
                  !data.body?.trim()) ||
                (kind === "delivery_type" && !data.spec_refs?.length)
              }
            >
              {old ? `保存 ${labels[kind]} 新版本` : `创建 ${labels[kind]}`}
            </ActionButton>
            <>
              <ActionButton
                type="button"
                disabled={busy || !old}
                onClick={() => {
                  captureFeedback();
                  return void save(true);
                }}
              >
                删除 {labels[kind]}
              </ActionButton>
              {kind === "skill" && old && (
                <ActionButton
                  type="button"
                  disabled={busy}
                  onClick={async () => {
                    captureFeedback();

                    (beginFeedback(), setBusy(true));
                    setError("");
                    try {
                      const pkg = await request("skill.export", {
                        id: old.id,
                        project_id: project?.id,
                      });
                      const row = await request("skill.import", {
                        project_id: project?.id,
                        command_id: crypto.randomUUID(),
                        payload: {
                          name: `${old.name} · 副本`,
                          scope: old.scope,
                          package: pkg,
                        },
                      });
                      await load();
                      choose(row);
                      setMessage("Skill 已复制，材料和工具依赖保留。");
                    } catch (e) {
                      setError(String(e));
                    } finally {
                      (endFeedback(), setBusy(false));
                    }
                  }}
                >
                  复制 Skill
                </ActionButton>
              )}
            </>
          </div>
        </ActionForm>
      </div>
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
    </section>
  );
}

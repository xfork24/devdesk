import { DshModelFields, type ModelProvider } from "./DshProviderSettings";
import { useCallback, useEffect, useRef, useState } from "react";
import { RoleManager } from "./RoleManager";
import {
  actionLabels,
  dutyLabels,
  latest,
  loadCatalog,
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
};
function initial(kind: string): Row {
  switch (kind) {
    case "agent":
      return {
        role_ref: "",
        capability_ref: "",
        permissions: [
          "permission-container-full-access-v1",
          "permission-container-full-access-v1",
        ],
        approval: "never",
        permission_selection: {
          sandbox_ref: "",
          harness: { mode: "follow_target" },
          sandbox: { mode: "follow_target" },
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
      return { body: "", baseline_rules: [] };
    case "spec":
      return { body: "", rules: [], applicable_actions: [] };
    default:
      return { body: "", rules: [] };
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
            <button
              key={id}
              role="tab"
              aria-selected={id === kind}
              onClick={() => setKind(id)}
            >
              {labels[id]}
            </button>
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
}: {
  kind: string;
  request: Request;
  project?: Row;
  installations: Row[];
  revision: number;
}) {
  const [providers, setProviders] = useState<ModelProvider[]>([]);
  const [records, setRecords] = useState<Row[]>([]),
    [permissions, setPermissions] = useState<Row[]>([]),
    [permissionResources, setPermissionResources] = useState<Row>({
      targets: [],
      bindings: [],
      profiles: [],
    }),
    [error, setError] = useState(""),
    [message, setMessage] = useState(""),
    [busy, setBusy] = useState(false),
    [integrations, setIntegrations] = useState<Row[]>([]),
    [legacyCapabilities, setLegacyCapabilities] = useState<
      Record<string, string>
    >({});
  const [draft, setDraft] = useDraft(
    `devdesk:v2:resource:${project?.id || "global"}:${kind}`,
    () => ({ selected: "", name: "", scope: "global", data: initial(kind) }),
  );
  const load = useCallback(async () => {
    const [
      records,
      policy,
      capabilities,
      resources,
      modelProviders,
      integrationResources,
    ] = await Promise.all([
      loadCatalog(request, project),
      request("query.permissions"),
      request("query.capabilities", {
        local: !!project,
        project_id: project?.id,
      }),
      request("query.permission_resources", { project_id: project?.id }),
      request("query.model_providers"),
      ["capability", "agent", "skill"].includes(kind)
        ? loadIntegrationRecords(request, project)
        : Promise.resolve([]),
    ]);
    setRecords(records);
    setIntegrations(integrationResources);
    setProviders(modelProviders.providers || []);
    if (kind === "tool")
      setDraft((current) =>
        current.data.implementation === "experience.lookup"
          ? { selected: "", name: "", scope: "global", data: initial(kind) }
          : current,
      );
    setPermissions(policy.permissions || []);
    setPermissionResources(resources);
    setDraft((current) =>
      !current.selected &&
      current.data.permission_selection &&
      !current.data.permission_selection.sandbox_ref
        ? {
            ...current,
            data: {
              ...current.data,
              permission_selection: {
                ...current.data.permission_selection,
                sandbox_ref: resources.default_sandbox_ref,
              },
            },
          }
        : current,
    );
    setLegacyCapabilities(capabilities.agent_bindings || {});
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
  const patch = (value: Row) =>
    setDraft((current) => ({
      ...current,
      data: { ...current.data, ...value },
    }));
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
      onChange={(value) => patch({ [key]: value })}
    />
  );
  const field = (label: string, key: string, required = false) => (
    <label>
      {label}
      <input
        required={required}
        value={data[key] || ""}
        onChange={(e) => patch({ [key]: e.target.value })}
      />
    </label>
  );
  const choose = (row: Row) => {
    let value = structuredClone(row.data);
    if (kind === "agent") {
      const role = records.find((r) => r.id === value.role_ref);
      value = {
        role_ref: value.role_ref || "",
        capability_ref:
          value.capability_ref || legacyCapabilities[row.id] || "",
        permissions: value.permissions,
        approval: value.approval,
        ...(value.permission_selection
          ? { permission_selection: value.permission_selection }
          : {}),
        instructions:
          role?.data.instructions === value.instructions
            ? ""
            : value.instructions,
      };
    }
    if (kind === "standard" && !value.baseline_rules)
      value = { body: value.body || "", baseline_rules: [] };
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
      const row = records.find((r) => r.id === selected && r.kind === kind);
      if (row) {
        routeConsumed.current = true;
        if (draft.selected !== row.id) choose(row);
      }
    }
  }, [records, kind]);
  async function save(remove = false) {
    setBusy(true);
    setError("");
    try {
      let payload = structuredClone(data);
      if (kind === "capability")
        payload.tools = (payload.tools || []).filter((id: string) =>
          options("tool").some((row) => row.id === id),
        );
      if (kind === "spec") delete payload.slots;
      if (kind === "agent" && !payload.instructions)
        delete payload.instructions;
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
      setBusy(false);
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
        <button
          onClick={() => {
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
        </button>
      </div>
      <div className="configuration-layout">
        <div className="configuration-list">
          {latest(records.filter((r) => r.kind === kind)).map((row) => (
            <button
              key={row.id}
              className={draft.selected === row.id ? "selected" : ""}
              onClick={() => choose(row)}
            >
              <strong>{row.name}</strong>
              <span>
                v{row.version} ·{" "}
                {row.scope.kind === "global" ? "Global" : "项目"}
              </span>
            </button>
          ))}
        </div>
        <form
          onSubmit={(e) => {
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
                onChange={(e) => setDraft({ ...draft, name: e.target.value })}
              />
            </label>
            <label>
              定义范围
              <select
                disabled={!!old}
                value={draft.scope}
                onChange={(e) => setDraft({ ...draft, scope: e.target.value })}
              >
                <option value="global">Global</option>
                {project && <option value="project">当前项目</option>}
              </select>
            </label>
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
                    <strong>
                      {role.name} · {dutyLabels[role.data.execution_role]}
                    </strong>
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
                <details className="full-width">
                  <summary>权限与兼容指令</summary>
                  <label>
                    权限绑定方式
                    <select
                      value={data.permission_selection ? "versioned" : "legacy"}
                      onChange={(e) => {
                        const next = { ...data };
                        if (e.target.value === "versioned")
                          next.permission_selection = {
                            sandbox_ref:
                              permissionResources.default_sandbox_ref,
                            harness: { mode: "follow_target" },
                            sandbox: { mode: "follow_target" },
                          };
                        else delete next.permission_selection;
                        patch(next);
                      }}
                    >
                      <option value="versioned">按目标版本选择权限</option>
                      <option value="legacy">保留旧固定权限</option>
                    </select>
                  </label>
                  {data.permission_selection ? (
                    <>
                      <label>
                        Sandbox 目标修订
                        <select
                          required
                          value={data.permission_selection.sandbox_ref}
                          onChange={(e) =>
                            patch({
                              permission_selection: {
                                ...data.permission_selection,
                                sandbox_ref: e.target.value,
                              },
                            })
                          }
                        >
                          <option value="">选择 Sandbox</option>
                          {permissionResources.targets
                            .filter((t: Row) => t.target.kind === "sandbox")
                            .map((t: Row) => (
                              <option
                                key={t.target.revision_id}
                                value={t.target.revision_id}
                              >
                                {t.name} · v{t.version}
                              </option>
                            ))}
                        </select>
                      </label>
                      {(["harness", "sandbox"] as const).map((layer) => {
                        const choice = data.permission_selection[layer];
                        const targetId =
                          layer === "harness"
                            ? capability?.data.harness_ref || data.harness_ref
                            : data.permission_selection.sandbox_ref;
                        const bindings = permissionResources.bindings.filter(
                          (b: Row) => b.data.target.revision_id === targetId,
                        );
                        const update = (value: Row) =>
                          patch({
                            permission_selection: {
                              ...data.permission_selection,
                              [layer]: value,
                            },
                          });
                        return (
                          <div key={layer}>
                            <label>
                              {layer} 权限来源
                              <select
                                value={choice.mode}
                                onChange={(e) =>
                                  update(
                                    e.target.value === "follow_target"
                                      ? { mode: "follow_target" }
                                      : {
                                          mode: "pinned",
                                          binding_revision_id:
                                            bindings[0]?.id || "",
                                        },
                                  )
                                }
                              >
                                <option value="follow_target">
                                  追随目标活动绑定
                                </option>
                                <option value="pinned">固定绑定修订</option>
                              </select>
                            </label>
                            {choice.mode === "pinned" ? (
                              <label>
                                {layer} 固定绑定
                                <select
                                  required
                                  value={choice.binding_revision_id}
                                  onChange={(e) =>
                                    update({
                                      mode: "pinned",
                                      binding_revision_id: e.target.value,
                                    })
                                  }
                                >
                                  <option value="">选择绑定修订</option>
                                  {bindings.map((b: Row) => (
                                    <option key={b.id} value={b.id}>
                                      {b.name} · v{b.version} · {b.id}
                                    </option>
                                  ))}
                                </select>
                              </label>
                            ) : (
                              <p className="muted">
                                新讨论采用目标活动权限；已有工程配置保持冻结。
                              </p>
                            )}
                          </div>
                        );
                      })}
                    </>
                  ) : (
                    <>
                      {[0, 1].map((index) => (
                        <RevisionSelect
                          key={index}
                          label={
                            index === 0
                              ? "Harness Permission"
                              : "Sandbox Permission"
                          }
                          records={permissions.map((p) => ({
                            ...p,
                            kind: "permission",
                            definition_id: p.identity,
                          }))}
                          value={
                            data.permissions?.[index] ||
                            data.permissions?.[0] ||
                            ""
                          }
                          required
                          onChange={(value) => {
                            const values = [...(data.permissions || ["", ""])];
                            values[index] = value;
                            patch({ permissions: values });
                          }}
                        />
                      ))}
                      <label>
                        确认策略
                        <select
                          value={data.approval || "never"}
                          onChange={(e) => patch({ approval: e.target.value })}
                        >
                          <option value="never">不额外询问</option>
                          <option value="on_request">按请求确认</option>
                        </select>
                      </label>
                    </>
                  )}
                  <label>
                    附加指令（可选）
                    <textarea
                      value={data.instructions || ""}
                      onChange={(e) => patch({ instructions: e.target.value })}
                    />
                  </label>
                </details>
              </>
            )}
            {kind === "agent" && old && (
              <div className="full-width actions">
                <button
                  type="button"
                  disabled={busy || !project}
                  onClick={async () => {
                    setBusy(true);
                    setError("");
                    setMessage("");
                    try {
                      const result = await request("agent.connection.check", {
                        project_id: project?.id,
                        agent_revision_id: old.id,
                      });
                      setMessage(
                        result.container_verified
                          ? `容器连接通过：${result.worker_endpoint}；模型${result.detail.model_present ? "已列出" : "未在目录列出"}，尚未验证推理。`
                          : "容器连接未通过；请检查地址、网络或凭据。",
                      );
                    } catch (error) {
                      setError(String(error));
                    } finally {
                      setBusy(false);
                    }
                  }}
                >
                  检查 DSH 容器连接
                </button>
                <button
                  type="button"
                  disabled={busy || !project}
                  onClick={async () => {
                    setBusy(true);
                    setError("");
                    setMessage("");
                    try {
                      const result = await request("agent.model.verify", {
                        project_id: project?.id,
                        agent_revision_id: old.id,
                      });
                      setMessage(
                        `已提交实际模型验证。请在讨论页查看 Session ${result.session_id} 的终态与工具结果；提交不代表验证通过。`,
                      );
                    } catch (error) {
                      setError(String(error));
                    } finally {
                      setBusy(false);
                    }
                  }}
                >
                  实际模型与工具验证
                </button>
                {!project && (
                  <p className="muted">
                    打开项目后可验证完整 Agent；实际验证会调用所选模型。
                  </p>
                )}
              </div>
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
                    <a href="?tab=integrations">管理工具接入</a>
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
                  <select
                    aria-label="模型后端"
                    value={data.provider}
                    onChange={(e) =>
                      patch({
                        provider: e.target.value,
                        parameters: {},
                        endpoint: undefined,
                        context_window: undefined,
                        max_output_tokens: undefined,
                        model: "",
                      })
                    }
                  >
                    {providers.map((p) => (
                      <option key={p.id} value={p.id}>
                        {p.label}
                      </option>
                    ))}
                  </select>
                </label>
                {field("模型标识", "model", true)}
                {data.provider !== "openai" && (
                  <DshModelFields
                    key={data.provider}
                    request={request}
                    data={{ ...data, provider: String(data.provider) }}
                    patch={patch}
                    providers={providers}
                  />
                )}
                {data.provider === "openai" ? (
                  <label>
                    推理强度
                    <select
                      value={data.parameters?.reasoning_effort || ""}
                      onChange={(e) =>
                        patch({
                          parameters: e.target.value
                            ? { reasoning_effort: e.target.value }
                            : {},
                        })
                      }
                    >
                      <option value="">模型默认</option>
                      {["minimal", "low", "medium", "high", "xhigh", "max"].map(
                        (v) => (
                          <option key={v}>{v}</option>
                        ),
                      )}
                    </select>
                  </label>
                ) : (
                  <label>
                    最大输出 Tokens
                    <input
                      type="number"
                      min={data.provider === "deepseek" ? 1024 : 1}
                      max={data.max_output_tokens || 1000000}
                      value={data.parameters?.max_tokens || ""}
                      onChange={(e) =>
                        patch({
                          parameters: e.target.value
                            ? { max_tokens: Number(e.target.value) }
                            : {},
                        })
                      }
                    />
                  </label>
                )}
              </>
            )}
            {kind === "harness" && (
              <>
                <label>
                  运行时后端
                  <select
                    value={data.provider}
                    onChange={(e) =>
                      patch({
                        provider: e.target.value,
                        model_providers: [
                          ...(e.target.value === "codex"
                            ? ["openai"]
                            : providers
                                .filter((p) => p.harness === "dsh")
                                .map((p) => p.id)),
                        ],
                      })
                    }
                  >
                    <option value="codex">Codex</option>
                    <option value="dsh">DSH</option>
                  </select>
                </label>
                {field("固定运行版本", "runtime_version", true)}
                <fieldset>
                  <legend>支持的模型服务商</legend>
                  {providers
                    .filter((p) => p.harness === data.provider)
                    .map((p) => (
                      <label key={p.id}>
                        <input
                          type="checkbox"
                          checked={(data.model_providers || []).includes(p.id)}
                          onChange={(event) =>
                            patch({
                              model_providers: event.target.checked
                                ? [...(data.model_providers || []), p.id]
                                : (data.model_providers || []).filter(
                                    (id: string) => id !== p.id,
                                  ),
                            })
                          }
                        />
                        {p.label}
                      </label>
                    ))}
                </fieldset>
                {field("构建配方摘要（可选）", "recipe_digest")}
                <div className="full-width">
                  <h3>已登记安装</h3>
                  {installations
                    .filter((r) => r.key?.provider === data.provider)
                    .map((r) => (
                      <p key={r.id}>
                        {r.key.version} · {r.key.os}/{r.key.arch}
                      </p>
                    ))}
                  {old && (
                    <button
                      type="button"
                      disabled={busy}
                      onClick={async () => {
                        setBusy(true);
                        try {
                          await request("runtime.prepare_harness", {
                            id: old.id,
                            project_id: project?.id,
                          });
                          setMessage("所选 Harness 已核验并准备。");
                        } catch (e) {
                          setError(String(e));
                        } finally {
                          setBusy(false);
                        }
                      }}
                    >
                      准备所选 Harness
                    </button>
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
                    <button
                      type="button"
                      onClick={async () => {
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
                    </button>
                  </div>
                ) : null;
              })()}
            {kind === "tool" && (
              <>
                {field("工具标识", "qualified_name", true)}
                <label>
                  工具实现
                  <select
                    value={data.implementation}
                    onChange={(e) => patch({ implementation: e.target.value })}
                  >
                    <option value="harness.native">Harness 原生工具</option>
                  </select>
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
                        onChange={(e) => patch({ input_text: e.target.value })}
                      />
                    </label>
                    <label>
                      输出 JSON Schema
                      <textarea
                        value={
                          data.output_text ??
                          JSON.stringify(data.output_schema, null, 2)
                        }
                        onChange={(e) => patch({ output_text: e.target.value })}
                      />
                    </label>
                  </>
                )}
              </>
            )}
            {["skill", "spec", "standard"].includes(kind) && (
              <MarkdownField
                key={draft.selected || "new"}
                label="Markdown 正文"
                value={data.body || ""}
                required
                onChange={(body) => patch({ body })}
              />
            )}
            {kind === "spec" && (
              <>
                <label>
                  适用动作
                  <select
                    multiple
                    value={(data.applicable_actions || []).map(
                      (a: Row) => a.kind,
                    )}
                    onChange={(e) =>
                      patch({
                        applicable_actions: Array.from(
                          e.target.selectedOptions,
                          (o) => ({ kind: o.value }),
                        ),
                      })
                    }
                  >
                    {Object.entries(actionLabels).map(([id, label]) => (
                      <option key={id} value={id}>
                        {label}
                      </option>
                    ))}
                  </select>
                </label>
                <label>
                  自动检查规则
                  <select
                    multiple
                    value={data.rules || []}
                    onChange={(e) =>
                      patch({
                        rules: Array.from(
                          e.target.selectedOptions,
                          (o) => o.value,
                        ),
                      })
                    }
                  >
                    {[
                      ["traceability", "需求追溯"],
                      ["failure_paths", "失败路径"],
                      ["sections", "文档章节"],
                    ].map(([id, label]) => (
                      <option key={id} value={id}>
                        {label}
                      </option>
                    ))}
                  </select>
                </label>
                <p className="muted full-width">
                  Markdown 要求可交给人或 Agent
                  评审，自动检查仅执行这里已支持的检查器。
                </p>
              </>
            )}
            {kind === "standard" && (
              <div className="full-width">
                <h3>覆盖规则</h3>
                {(data.baseline_rules || []).map((rule: Row, index: number) => (
                  <fieldset key={index}>
                    <legend>规则 {index + 1}</legend>
                    <label>
                      标准动作
                      <select
                        value={rule.selector.kind}
                        onChange={(e) => {
                          const rules = structuredClone(data.baseline_rules);
                          rules[index].selector.kind = e.target.value;
                          patch({ baseline_rules: rules });
                        }}
                      >
                        {Object.entries(actionLabels).map(([id, label]) => (
                          <option key={id} value={id}>
                            {label}
                          </option>
                        ))}
                      </select>
                    </label>
                    <label>
                      阶段限定（可选）
                      <select
                        value={rule.selector.phase || ""}
                        onChange={(e) => {
                          const rules = structuredClone(data.baseline_rules);
                          if (e.target.value)
                            rules[index].selector.phase = e.target.value;
                          else delete rules[index].selector.phase;
                          patch({ baseline_rules: rules });
                        }}
                      >
                        <option value="">全部阶段</option>
                        {[
                          "requirement_definition",
                          "solution_planning",
                          "implementation",
                          "closure",
                        ].map((value) => (
                          <option key={value}>{value}</option>
                        ))}
                      </select>
                    </label>
                    <label>
                      交付类型限定（可选）
                      <select
                        value={rule.selector.delivery_kind || ""}
                        onChange={(e) => {
                          const rules = structuredClone(data.baseline_rules);
                          if (e.target.value)
                            rules[index].selector.delivery_kind =
                              e.target.value;
                          else delete rules[index].selector.delivery_kind;
                          patch({ baseline_rules: rules });
                        }}
                      >
                        <option value="">全部交付</option>
                        {[
                          "requirement",
                          "plan",
                          "implementation",
                          "closure",
                          "merge_candidate",
                        ].map((value) => (
                          <option key={value}>{value}</option>
                        ))}
                      </select>
                    </label>
                    <RevisionSelect
                      label={`规则 ${index + 1} 必需 Spec`}
                      records={options("spec")}
                      multiple
                      required
                      value={rule.required_spec_refs}
                      onChange={(value) => {
                        const rules = structuredClone(data.baseline_rules);
                        rules[index].required_spec_refs = value;
                        patch({ baseline_rules: rules });
                      }}
                    />
                    <label>
                      最低匹配节点数
                      <input
                        type="number"
                        min={1}
                        required
                        value={rule.min_matches}
                        onChange={(e) => {
                          const rules = structuredClone(data.baseline_rules);
                          rules[index].min_matches = Number(e.target.value);
                          patch({ baseline_rules: rules });
                        }}
                      />
                    </label>
                    <button
                      type="button"
                      onClick={() =>
                        patch({
                          baseline_rules: data.baseline_rules.filter(
                            (_: Row, i: number) => i !== index,
                          ),
                        })
                      }
                    >
                      删除规则 {index + 1}
                    </button>
                  </fieldset>
                ))}
                <button
                  type="button"
                  onClick={() =>
                    patch({
                      baseline_rules: [
                        ...(data.baseline_rules || []),
                        {
                          selector: { kind: "delivery.review" },
                          required_spec_refs: [],
                          min_matches: 1,
                        },
                      ],
                    })
                  }
                >
                  添加覆盖规则
                </button>
              </div>
            )}
          </div>
          <div className="actions">
            <button
              className="primary"
              disabled={
                busy ||
                !draft.name.trim() ||
                (["skill", "spec", "standard"].includes(kind) &&
                  !data.body?.trim())
              }
            >
              {old ? `保存 ${labels[kind]} 新版本` : `创建 ${labels[kind]}`}
            </button>
            {old && (
              <>
                <button
                  type="button"
                  disabled={busy}
                  onClick={() => void save(true)}
                >
                  删除 {labels[kind]}
                </button>
                {kind === "skill" && old && (
                  <button
                    type="button"
                    disabled={busy}
                    onClick={async () => {
                      setBusy(true);
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
                        setBusy(false);
                      }
                    }}
                  >
                    复制 Skill
                  </button>
                )}
              </>
            )}
          </div>
        </form>
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

import { useCallback, useEffect, useRef, useState } from "react";
import { RoleManager } from "./RoleManager";
import {
  actionLabels,
  dutyLabels,
  latest,
  loadCatalog,
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
        ? ["model", "harness", "tool", "skill"]
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
function ResourceEditor({
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
  const [records, setRecords] = useState<Row[]>([]),
    [permissions, setPermissions] = useState<Row[]>([]),
    [error, setError] = useState(""),
    [message, setMessage] = useState(""),
    [busy, setBusy] = useState(false),
    [legacyCapabilities, setLegacyCapabilities] = useState<
      Record<string, string>
    >({});
  const [draft, setDraft] = useDraft(
    `devdesk:v2:resource:${project?.id || "global"}:${kind}`,
    () => ({ selected: "", name: "", scope: "global", data: initial(kind) }),
  );
  const load = useCallback(async () => {
    const [records, policy, capabilities] = await Promise.all([
      loadCatalog(request, project),
      request("query.permissions"),
      request("query.capabilities", {
        local: !!project,
        project_id: project?.id,
      }),
    ]);
    setRecords(records);
    if (kind === "tool")
      setDraft((current) =>
        current.data.implementation === "experience.lookup"
          ? { selected: "", name: "", scope: "global", data: initial(kind) }
          : current,
      );
    setPermissions(policy.permissions || []);
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
        if (payload.input_text !== undefined)
          payload.input_schema = JSON.parse(payload.input_text);
        if (payload.output_text !== undefined)
          payload.output_schema = JSON.parse(payload.output_text);
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
                        data.permissions?.[index] || data.permissions?.[0] || ""
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
                    value={data.provider}
                    onChange={(e) =>
                      patch({ provider: e.target.value, parameters: {} })
                    }
                  >
                    <option value="openai">OpenAI</option>
                    <option value="deepseek">DeepSeek</option>
                  </select>
                </label>
                {field("模型标识", "model", true)}
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
                      min={1024}
                      max={131072}
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
                          e.target.value === "codex" ? "openai" : "deepseek",
                        ],
                      })
                    }
                  >
                    <option value="codex">Codex</option>
                    <option value="dsh">DSH</option>
                  </select>
                </label>
                {field("固定运行版本", "runtime_version", true)}
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
              <button
                type="button"
                disabled={busy}
                onClick={() => void save(true)}
              >
                删除 {labels[kind]}
              </button>
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

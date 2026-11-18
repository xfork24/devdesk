import { displayText } from "../display-text";
import { useReadRequest, ignoreCancelledRefresh } from "./use-read-request";
import { ActionButton, ActionForm, ActionSelect } from "./ActionFeedback";
import { useActionFeedback } from "./action-feedback";
import { ModelFields, type ModelProvider } from "./DshProviderSettings";
import {
  useCallback,
  useEffect,
  useRef,
  useState,
  type ReactNode,
} from "react";
import { RoleManager } from "./RoleManager";
import { ConfigurationList } from "./ConfigurationList";
import { ConfigurationUsage } from "./ConfigurationUsage";
import {
  modelProviderLabel,
  normalizeOllamaServiceAddress,
} from "./ollama-endpoint";
import { JsonField } from "./WorkflowDeliveries";
import { StandardManager } from "./StandardManager";
import { SpecLint } from "./SpecLint";
import { WorkflowSpecifications } from "./WorkflowSpecifications";
import { DeliverySpecPreview } from "./DeliveryContent";
import { isProjectRuntimeInstallation } from "./runtime-installations";
import {
  sourceContent,
  sourcePayload,
  latest,
  isPristineConfigurationDraft,
  loadCatalog,
  loadConfigurationRecord,
  mergeConfigurationRecords,
  templateSourceLabel,
  loadIntegrationRecords,
  useDraft,
  useGroupTab,
} from "./configuration-state";
import {
  MarkdownField,
  MarkdownPreview,
  RevisionSelect,
} from "./configuration-ui";
import { type Row, type Request } from "./rpc-types";
const labels: Record<string, string> = {
  role: "Role",
  capability: "Capability",
  agent: "Agent",
  model: "Model",
  harness: "Harness",
  sandbox: "Sandbox",
  tool: "Tool",
  skill: "Skill",
  spec: "Spec",
  standard: "Standard",
  delivery_type: "Delivery",
};
function initial(kind: string): Row {
  switch (kind) {
    case "agent":
      return {
        role_ref: "",
        capability_ref: "",
      };
    case "capability":
      return {
        model_ref: "",
        harness_ref: "",
        tools: [],
        skills: [],
        spec_refs: [],
      };
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
        model_providers: [],
      };
    case "sandbox":
      return {
        provider: "docker",
        runtime_version: "1",
        enforcement: "docker-worker/v1",
        platforms: ["linux/aarch64", "linux/x86_64"],
      };
    case "tool":
      return {
        qualified_name: "",
        implementation: "harness.native",
        input_schema: { type: "object" },
        output_schema: { type: "object" },
        capabilities: [],
      };
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
            {displayText(
              group === "actors"
                ? "角色与代理"
                : group === "foundations"
                  ? "基础组件"
                  : "规范",
            )}
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
              {displayText(labels[id])}
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
      ) : kind === "standard" ? (
        <StandardManager request={request} project={project} />
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
  request: parentRequest,
  project,
  installations,
  revision,
  additionalEditor,
  detailFooter,
}: {
  kind: string;
  request: Request;
  project?: Row;
  installations: Row[];
  revision: number;
  additionalEditor?: { label: string; content: ReactNode };
  detailFooter?: (record: Row | undefined) => ReactNode;
}) {
  const request = useReadRequest(parentRequest);
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
  const [records, setRecords] = useState<Row[]>([]),
    [busy, setBusy] = useState(false),
    [integrations, setIntegrations] = useState<Row[]>([]);
  const [draft, setDraft] = useDraft(
    `devdesk:source-flat-v1:resource:${project?.id || "global"}:${kind}`,
    () => ({ selected: "", name: "", scope: "global", data: initial(kind) }),
  );
  const choice = useRef<AbortController | null>(null);
  useEffect(() => () => choice.current?.abort(), []);
  const load = useCallback(async () => {
    const [records, modelProviders, integrationResources] = await Promise.all([
      loadCatalog(request, project?.id, ["harness", "sandbox"].includes(kind)),
      request("query.model_providers"),
      ["capability", "agent", "skill"].includes(kind)
        ? loadIntegrationRecords(request, project?.id)
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
  }, [request, project?.id, kind, setDraft]);
  useEffect(() => {
    let active = true;
    void load().catch((e) => {
      if (active) setError(String(e));
    });
    return () => {
      active = false;
    };
  }, [load, revision, setError]);
  const old = records.find((r) => r.id === draft.selected),
    data = draft.data;
  const detailRefs = JSON.stringify([
    ...new Set([
      ...(draft.selected && (!old || old._index) ? [draft.selected] : []),
      ...(kind === "agent" &&
      data.role_ref &&
      (!records.find((r) => r.id === data.role_ref) ||
        records.find((r) => r.id === data.role_ref)?._index)
        ? [data.role_ref]
        : []),
      ...(kind === "capability"
        ? [data.model_ref, data.harness_ref].filter(
            (id) => id && !records.some((r) => r.id === id),
          )
        : []),
    ]),
  ]);
  useEffect(() => {
    const refs: string[] = JSON.parse(detailRefs);
    if (!refs.length) return;
    const controller = new AbortController();
    void Promise.all(
      refs.map((id) =>
        loadConfigurationRecord(
          request,
          { id, _index: true },
          project?.id,
          controller.signal,
        ),
      ),
    )
      .then((rows) => {
        if (!controller.signal.aborted)
          setRecords((old) => mergeConfigurationRecords(old, rows));
      })
      .catch((error) => {
        if (!controller.signal.aborted) setError(String(error));
      });
    return () => controller.abort();
  }, [detailRefs, request, project?.id, setError]);
  const projectInstallations = installations.filter(
    (r) => r.key?.provider === data.provider && isProjectRuntimeInstallation(r),
  );
  const patch = (value: Row) =>
    setDraft((current) => ({
      ...current,
      data: { ...current.data, ...value },
    }));
  const options = (family: string) => records.filter((r) => r.type === family);
  const select = (
    label: string,
    family: string,
    key: string,
    multiple = false,
    required = false,
  ) => {
    const props = { label, records: options(family), required };
    const value =
      family === "tool"
        ? (data[key] || []).filter((id: string) =>
            options("tool").some((row) => row.id === id),
          )
        : data[key] || (multiple ? [] : "");
    const onChange = (value: string | string[]) => {
      captureFeedback();
      patch({ [key]: value });
    };
    return multiple ? (
      <RevisionSelect {...props} multiple value={value} onChange={onChange} />
    ) : (
      <RevisionSelect {...props} value={value} onChange={onChange} />
    );
  };
  const field = (label: string, key: string, required = false) => (
    <label>
      {displayText(label)}
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
  const choose = useCallback(
    async (row: Row) => {
      choice.current?.abort();
      const controller = new AbortController();
      choice.current = controller;
      try {
        row = await loadConfigurationRecord(
          request,
          row,
          project?.id,
          controller.signal,
        );
        if (controller.signal.aborted || row.type !== kind) return;
        setRecords((rows) => mergeConfigurationRecords(rows, [row]));
        setShowAdditionalEditor(false);
        let value = sourceContent(row);
        if (kind === "agent") {
          value = {
            role_ref: value.role_ref || "",
            capability_ref: value.capability_ref || "",
          };
        }
        setDraft({
          selected: row.id,
          name: row.name,
          scope: row.scope === "global" ? "global" : "project",
          data: value,
        });
        const params = new URLSearchParams(location.search);
        params.set("config_object", row.id);
        history.replaceState(null, "", `?${params}`);
        setMessage("");
      } catch (error) {
        if (!controller.signal.aborted) setError(String(error));
      }
    },
    [kind, setDraft, setMessage, request, project?.id, setError],
  );
  const routeConsumed = useRef(false);
  useEffect(() => {
    if (routeConsumed.current || !records.length) return;
    const selected = new URLSearchParams(location.search).get("config_object");
    routeConsumed.current = true;
    if (selected) {
      const instance = records.find(
        (r) => r.id === selected && r.type === "agent_instance",
      );
      const revision =
        kind === "agent" && instance
          ? instance.data.agent_revision_id
          : selected;
      const row = records.find((r) => r.id === revision && r.type === kind) || {
        id: revision,
        type: kind,
        _index: true,
      };
      if (row) {
        if (draft.selected !== row.id) choose(row);
      }
    }
  }, [records, kind, draft, choose]);
  async function save(remove = false) {
    captureFeedback();

    beginFeedback();
    setBusy(true);
    setError("");
    try {
      const payload = structuredClone(data);
      if (kind === "model" && payload.provider === "ollama")
        payload.endpoint = normalizeOllamaServiceAddress(
          payload.endpoint ||
            providers.find((provider) => provider.id === "ollama")?.endpoint ||
            "",
        );
      if (kind === "model" && !payload.model)
        throw new Error("请先从模型目录中选择模型标识");
      if (kind === "agent" && !payload.instructions)
        delete payload.instructions;
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
                    ? `project:${project!.id}`
                    : "global",
                ...sourcePayload(kind, payload),
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
      await load().catch(ignoreCancelledRefresh);
      setMessage(
        remove ? "已停用；历史保留。" : "新修订已保存，已有使用方保持原引用。",
      );
    } catch (e) {
      setError(String(e));
    } finally {
      endFeedback();
      setBusy(false);
    }
  }
  const role = records.find((r) => r.id === data.role_ref),
    capability = records.find((r) => r.id === data.capability_ref);
  const model = records.find((r) => r.id === data.model_ref),
    harness = records.find((r) => r.id === data.harness_ref);
  return (
    <section className="panel" aria-label={displayText(`${labels[kind]} 管理`)}>
      <div className="panel-heading">
        <div>
          <h2>{displayText(labels[kind])} 管理</h2>
          {templateSourceLabel(old) && (
            <span className="muted">
              {displayText(templateSourceLabel(old))}
            </span>
          )}
          <p>
            {displayText(
              kind === "agent"
                ? "给一个 Role 与一个 Capability 的组合命名。"
                : kind === "capability"
                  ? "选择可复用的 Model、Harness、Tools 和 Skills。"
                  : "修订独立保存，修改后由使用方显式采用。",
            )}
          </p>
        </div>
      </div>
      <div className="configuration-layout">
        <ConfigurationList
          records={latest(records.filter((row) => row.type === kind))}
          selectedId={showAdditionalEditor ? undefined : draft.selected}
          ready={records.length > 0}
          autoSelectFirst={
            !showAdditionalEditor &&
            !new URLSearchParams(location.search).has("config_object") &&
            isPristineConfigurationDraft(draft, {
              selected: "",
              name: "",
              scope: "global",
              data: initial(kind),
            })
          }
          onSelect={(row) => {
            captureFeedback();
            routeConsumed.current = true;
            return choose(row);
          }}
          newAction={{
            label: `新建 ${labels[kind]}`,
            selected: !draft.selected && !showAdditionalEditor,
            onClick: () => {
              captureFeedback();

              setShowAdditionalEditor(false);
              routeConsumed.current = true;
              choice.current?.abort();
              const params = new URLSearchParams(location.search);
              params.delete("config_object");
              history.replaceState(null, "", `?${params}`);
              setDraft({
                selected: "",
                name: "",
                scope: "global",
                data: initial(kind),
              });
            },
          }}
          actions={
            additionalEditor && (
              <ActionButton
                className={showAdditionalEditor ? "selected" : ""}
                onClick={() => {
                  routeConsumed.current = true;
                  setShowAdditionalEditor(true);
                  setError("");
                  setMessage("");
                }}
              >
                {displayText(additionalEditor.label)}
              </ActionButton>
            )
          }
        />
        {additionalEditor && (
          <div hidden={!showAdditionalEditor}>
            {displayText(additionalEditor.content)}
          </div>
        )}
        <ActionForm
          hidden={showAdditionalEditor}
          className={
            ["agent", "capability"].includes(kind)
              ? "actor-configuration-form"
              : undefined
          }
          onSubmit={(e) => {
            captureFeedback();

            e.preventDefault();
            void save();
          }}
        >
          <div className="form-grid">
            {old && ["harness", "sandbox"].includes(kind) && (
              <RevisionSelect
                label={labels[kind]}
                showReferenceLinks={false}
                records={records.filter(
                  (r) =>
                    r.type === kind && r.definition_id === old.definition_id,
                )}
                value={old.id}
                onChange={(id) => {
                  const record = records.find((r) => r.id === id);
                  void choose(record || { id, type: kind, _index: true });
                }}
              />
            )}
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
                  <div className="notice actor-role-summary">
                    <strong>{displayText(role.name)}</strong>
                    <MarkdownPreview text={role.data.instructions} />
                  </div>
                )}
                {capability && (
                  <div className="notice">
                    Model：
                    {displayText(
                      records.find((r) => r.id === capability.model_ref)
                        ?.name || "固定修订",
                    )}{" "}
                    · Harness：
                    {displayText(
                      records.find((r) => r.id === capability.harness_ref)
                        ?.name || "固定修订",
                    )}{" "}
                    · Tools {displayText(capability.tools.length)} · Skills{" "}
                    {displayText(capability.skills.length)} · Specs{" "}
                    {displayText(capability.spec_refs?.length || 0)}
                  </div>
                )}
                <p className="muted full-width">
                  职责来自 Role，方法与工具来自 Capability；本次权限 Profile由
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
                {select("Tool 精确版本", "tool", "tools", true, true)}
                {select("Skill 精确版本", "skill", "skills", true)}
                {select("Spec 精确版本", "spec", "spec_refs", true)}
                {(data.skills || []).map((id: string) => {
                  const pkg = integrations.find(
                    (r) => r.type === "skill_package" && r.definition_id === id,
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
                      {displayText(records.find((r) => r.id === id)?.name)}：
                      {displayText(
                        missing.length
                          ? `缺少工具依赖 ${missing.join(", ")}`
                          : "工具依赖已满足",
                      )}
                    </p>
                  );
                })}
                {(data.tools || []).filter(
                  (id: string) =>
                    typeof records.find((r) => r.id === id)?.data
                      .integration_ref === "string",
                ).length > 0 && (
                  <p>
                    外部工具还需要项目选择接入绑定；保存 Capability
                    不会自动授权账户。
                    <a href="?tab=components&config_tab=http">管理工具接入</a>
                  </p>
                )}

                {model && harness && (
                  <p className="notice">
                    {displayText(
                      harness.model_providers?.length &&
                        !harness.model_providers.some(
                          (ref: string) =>
                            providers.find((p) => p.reference === ref)?.id ===
                            model.data.provider,
                        )
                        ? "此 Harness 不支持选定模型后端，保存时会拒绝。"
                        : `${model.name} / ${harness.name}`,
                    )}
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
                        {displayText(modelProviderLabel(p))}
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
                  parameters={
                    data.provider === "openai" ? (
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
                          {[
                            "minimal",
                            "low",
                            "medium",
                            "high",
                            "xhigh",
                            "max",
                          ].map((v) => (
                            <option key={v} value={v}>
                              {displayText(v)}
                            </option>
                          ))}
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
                    )
                  }
                />
              </>
            )}
            {kind === "sandbox" && (
              <>
                <label>
                  Sandbox 后端
                  <ActionSelect
                    aria-label="Sandbox 后端"
                    value={data.provider}
                    onChange={(e) => patch({ provider: e.target.value })}
                  >
                    <option value="docker">Docker</option>
                  </ActionSelect>
                </label>
                {field("Sandbox 运行版本", "runtime_version", true)}
                {field("Sandbox 执行合同", "enforcement", true)}
                <JsonField
                  label="Docker 基础环境与构建"
                  value={data.build || {}}
                  onChange={(build) => patch({ build })}
                />
                <label>
                  支持平台
                  <ActionSelect
                    aria-label="支持平台"
                    multiple
                    required
                    value={data.platforms || []}
                    onChange={(e) =>
                      patch({
                        platforms: Array.from(
                          e.target.selectedOptions,
                          (option) => option.value,
                        ),
                      })
                    }
                  >
                    <option value="linux/aarch64">linux/aarch64</option>
                    <option value="linux/x86_64">linux/x86_64</option>
                  </ActionSelect>
                </label>
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
                        runtime_version: "",
                        installation: {},
                        model_providers: providers
                          .filter((p) => p.harness === e.target.value)
                          .map((p) => p.reference),
                      });
                    }}
                  >
                    <option value="codex">Codex</option>
                    <option value="dsh">DSH</option>
                  </ActionSelect>
                </label>
                {field("Harness 发行版本", "runtime_version", true)}
                <JsonField
                  label="Harness 安装与登录组件"
                  value={data.installation || {}}
                  onChange={(installation) => patch({ installation })}
                />
                <fieldset className="model-provider-options">
                  <legend>支持的模型服务商</legend>
                  {providers
                    .filter((p) => p.harness === data.provider)
                    .map((p) => (
                      <label key={p.id} className="model-provider-choice">
                        <input
                          type="checkbox"
                          checked={(data.model_providers || []).includes(
                            p.reference,
                          )}
                          onChange={(event) => {
                            captureFeedback();
                            return patch({
                              model_providers: event.target.checked
                                ? [...(data.model_providers || []), p.reference]
                                : (data.model_providers || []).filter(
                                    (ref: string) => ref !== p.reference,
                                  ),
                            });
                          }}
                        />
                        <span>{displayText(p.label)}</span>
                      </label>
                    ))}
                </fieldset>
                <div className="full-width">
                  <h3>已登记安装</h3>
                  {projectInstallations.length === 0 && (
                    <p className="muted">尚未准备项目执行运行时</p>
                  )}
                  {projectInstallations.map((r) => (
                    <p key={r.id}>
                      {displayText(r.key.version)} · {displayText(r.key.os)}/
                      {displayText(r.key.arch)}
                    </p>
                  ))}
                  {old && (
                    <ActionButton
                      type="button"
                      disabled={busy}
                      onClick={async () => {
                        captureFeedback();

                        beginFeedback();
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
                          endFeedback();
                          setBusy(false);
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
                    r.type === "skill_package" && r.definition_id === old.id,
                );
                return pkg ? (
                  <div className="notice">
                    <p>来源：{displayText(pkg.data.source)}</p>
                    <p>
                      静态材料：{displayText(pkg.data.files.length)} 个 ·{" "}
                      {displayText(
                        pkg.data.scripts_disabled ? "脚本执行未启用" : "无脚本",
                      )}
                    </p>
                    <p>
                      必需工具：
                      {displayText(
                        pkg.data.requires_tools
                          .map(
                            (id: string) =>
                              records.find((r) => r.id === id)?.name || id,
                          )
                          .join(", ") || "无",
                      )}
                    </p>
                    {pkg.data.files.map((file: Row) => (
                      <details key={file.path}>
                        <summary>{displayText(file.path)}</summary>
                        {file.path.endsWith(".md") ? (
                          <MarkdownPreview text={file.content} />
                        ) : (
                          <pre>{displayText(file.content)}</pre>
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
            {["spec", "skill", "tool", "delivery_type"].includes(kind) && (
              <ConfigurationUsage
                request={request}
                projectId={project?.id}
                id={draft.selected}
                kind={kind}
              />
            )}
            {kind === "delivery_type" && (
              <>
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
                <fieldset className="full-width delivery-requirements">
                  <legend>内容与验收</legend>
                  <p className="muted">
                    每种交付必须关联内容 Spec，定义应交付内容、来源和验收标准。
                  </p>
                  <RevisionSelect
                    label="内容规范"
                    multiple
                    records={records.filter((r) => r.type === "spec")}
                    value={data.spec_refs || []}
                    onChange={(spec_refs) => patch({ spec_refs })}
                  />
                  <DeliverySpecPreview
                    references={data.spec_refs || []}
                    records={records}
                  />
                </fieldset>
                <fieldset className="full-width">
                  <legend>结构与来源</legend>
                  {data.producer ? (
                    <p>
                      产出来源：
                      {displayText(
                        {
                          agent: "Agent",
                          platform: "平台工具",
                          workflow: "Workflow",
                        }[data.producer.kind as string] || data.producer.kind,
                      )}
                      {displayText(
                        data.producer.tools
                          ? ` · ${data.producer.tools.join("、")}`
                          : "",
                      )}
                    </p>
                  ) : null}
                  <WorkflowSpecifications
                    kind={kind}
                    data={data}
                    records={records}
                    onChange={patch}
                  />
                </fieldset>
              </>
            )}
            {["skill", "spec"].includes(kind) && (
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
            {kind === "spec" ? (
              <WorkflowSpecifications
                kind={kind}
                data={data}
                records={records}
                onChange={patch}
              />
            ) : null}
            {["spec", "delivery_type"].includes(kind) && (
              <SpecLint
                request={request}
                project={project}
                kind={kind}
                data={data}
                record={old}
              />
            )}
          </div>
          <div className="actions">
            <ActionButton
              className="primary"
              disabled={
                busy ||
                !draft.name.trim() ||
                (["skill", "spec"].includes(kind) && !data.body?.trim()) ||
                (kind === "delivery_type" && !data.spec_refs?.length)
              }
            >
              {displayText(
                old ? `保存 ${labels[kind]} 新版本` : `创建 ${labels[kind]}`,
              )}
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
                删除 {displayText(labels[kind])}
              </ActionButton>
              {kind === "skill" && old && (
                <ActionButton
                  type="button"
                  disabled={busy}
                  onClick={async () => {
                    captureFeedback();

                    beginFeedback();
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
                      await load().catch(ignoreCancelledRefresh);
                      choose(row);
                      setMessage("Skill 已复制，材料和工具依赖保留。");
                    } catch (e) {
                      setError(String(e));
                    } finally {
                      endFeedback();
                      setBusy(false);
                    }
                  }}
                >
                  复制 Skill
                </ActionButton>
              )}
            </>
          </div>
        </ActionForm>
        {detailFooter && (
          <div className="resource-detail-footer">
            {displayText(detailFooter(old))}
          </div>
        )}
      </div>
      {error && (
        <p role="alert" className="error">
          {displayText(error)}
        </p>
      )}
      {message && (
        <p role="status" className="notice">
          {displayText(message)}
        </p>
      )}
    </section>
  );
}

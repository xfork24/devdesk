import { useCallback, useEffect, useRef, useState } from "react";
type Row = Record<string, any>;
type Props = {
  request: (method: string, params?: Row) => Promise<any>;
  project?: Row;
  installations: Row[];
};
const roles: Record<string, string> = {
  analyst: "需求分析师",
  architect: "架构师",
  developer: "开发",
  tester: "测试",
  reviewer: "评审",
};
const split = (s: string) =>
  s
    .split(/[,，\n]/)
    .map((s) => s.trim())
    .filter(Boolean);
export function AgentCatalog({ request, project, installations }: Props) {
  const [modelChoices, setModelChoices] = useState<Row[]>([]);
  const [records, setRecords] = useState<Row[]>([]),
    [permissions, setPermissions] = useState<Row[]>([]),
    [skills, setSkills] = useState<Row[]>([]),
    [kind, setKind] = useState("agent"),
    [scope, setScope] = useState("global"),
    [name, setName] = useState(""),
    [old, setOld] = useState<Row>(),
    [error, setError] = useState(""),
    [busy, setBusy] = useState(false),
    [message, setMessage] = useState("");
  const [form, setForm] = useState<Row>({
      role: "analyst",
      instructions: "",
      model_ref: "",
      harness_ref: "",
      skills: [],
      tools: [],
      permissions: [],
      approval: "never",
    }),
    [instanceAgent, setInstanceAgent] = useState(""),
    [instanceScope, setInstanceScope] = useState("project"),
    [instanceName, setInstanceName] = useState("");
  const [page, setPage] = useState(0),
    [total, setTotal] = useState(0);
  const generation = useRef(0);
  const load = useCallback(async () => {
    const current = ++generation.current;
    const [catalog, policy, definitions] = await Promise.all([
      request("query.configurations", {
        local: !!project,
        project_id: project?.id,
        offset: page * 100,
        limit: 100,
      }),
      request("query.permissions"),
      request("query.workspace", { project_id: project?.id, limit: 1 }),
    ]);
    if (current !== generation.current) return;
    setRecords(catalog.records);
    setTotal(catalog.total);
    setPermissions(policy.permissions);
    setSkills(
      (definitions.definitions || definitions).filter(
        (d: Row) => d.family === "skill",
      ),
    );
  }, [request, project?.id, page]);
  useEffect(() => {
    load().catch((e) => setError(String(e)));
    return () => {
      ++generation.current;
    };
  }, [load]);
  const update = (patch: Row) => setForm((old) => ({ ...old, ...patch }));
  const chooseKind = (k: string) => {
    setKind(k);
    setOld(undefined);
    setName("");
    setForm(
      k === "agent"
        ? {
            role: "analyst",
            instructions: "",
            model_ref: "",
            harness_ref: "",
            skills: [],
            tools: [],
            permissions: [],
            approval: "never",
          }
        : k === "model"
          ? {
              provider: "openai",
              model: "",
              release: "",
              parameters: {},
              capabilities: [],
            }
          : k === "harness"
            ? {
                provider: "codex",
                runtime_version: "",
                capabilities: [],
                model_providers: ["openai"],
              }
            : ["skill", "spec", "standard"].includes(k)
              ? {
                  body: "",
                  rules: k === "spec" ? ["traceability"] : [],
                  slots: [],
                }
              : {
                  qualified_name: "harness.native",
                  implementation: "harness.native",
                  input_schema: {},
                  output_schema: {},
                  capabilities: [],
                },
    );
  };
  async function act(fn: () => Promise<void>) {
    setBusy(true);
    setError("");
    try {
      await fn();
    } catch (e) {
      setError(String(e));
    } finally {
      setBusy(false);
    }
  }
  const options = (type: string) => records.filter((r) => r.kind === type);
  const selectedModel = records.find((r) => r.id === form.model_ref),
    selectedHarness = records.find((r) => r.id === form.harness_ref);
  const multi = (key: string, label: string, rows: Row[]) => (
    <label>
      {label}
      <select
        multiple
        value={form[key] || []}
        onChange={(e) =>
          update({
            [key]: Array.from(e.target.selectedOptions).map((o) => o.value),
          })
        }
      >
        {rows.map((r) => (
          <option key={r.id} value={r.id}>
            {r.name} v{r.version}
          </option>
        ))}
      </select>
    </label>
  );
  return (
    <section className="panel">
      <h2>Agent 组合与实例</h2>
      <p className="muted">
        Agent = Model + Harness + Skills / Tools / Permission。AgentID
        固定组合版本与经验范围。
      </p>
      <div className="actions">
        {[
          ["agent", "Agent"],
          ["model", "Model"],
          ["harness", "Harness"],
          ["tool", "Tool"],
          ["skill", "Skill"],
          ["spec", "Spec"],
          ["standard", "Standard"],
        ].map(([key, label]) => (
          <button
            key={key}
            className={kind === key ? "selected" : ""}
            onClick={() => chooseKind(key)}
          >
            {label}
          </button>
        ))}
      </div>
      <div className="workflow-list">
        {records
          .filter((r) => r.kind === kind)
          .map((r) => (
            <button
              key={r.id}
              onClick={() => {
                setOld(r);
                setName(r.name);
                setScope(r.scope.kind);
                setForm(structuredClone(r.data));
              }}
            >
              <strong>
                {r.name} v{r.version}
              </strong>
              <span>
                {r.scope.kind} ·{" "}
                {r.kind === "agent"
                  ? `${roles[r.data.role]} · Model ${r.data.model_ref.slice(0, 8)} · Harness ${r.data.harness_ref.slice(0, 8)}`
                  : r.data.model ||
                    r.data.runtime_version ||
                    r.data.qualified_name}
              </span>
            </button>
          ))}
      </div>
      <form
        onSubmit={(e) => {
          e.preventDefault();
          void act(async () => {
            await request(`${kind}.${old ? "revise" : "create"}`, {
              project_id: project?.id,
              command_id: crypto.randomUUID(),
              target: old?.id,
              expected_revision: old?.revision,
              payload: {
                name,
                scope:
                  scope === "project"
                    ? { kind: "project", project_id: project!.id }
                    : { kind: "global" },
                data: form,
              },
            });
            setOld(undefined);
            setMessage("已保存新的不可变修订；旧 AgentID 保持原组合。");
            await load();
          });
        }}
      >
        <div className="form-grid">
          <label>
            名称
            <input
              required
              value={name}
              onChange={(e) => setName(e.target.value)}
            />
          </label>
          <label>
            定义 Scope
            <select
              disabled={!!old}
              value={scope}
              onChange={(e) => setScope(e.target.value)}
            >
              <option value="global">Global</option>
              {project && <option value="project">当前 Project</option>}
            </select>
          </label>
          {kind === "agent" && (
            <>
              <label>
                职责
                <select
                  value={form.role}
                  onChange={(e) => update({ role: e.target.value })}
                >
                  {Object.entries(roles).map(([key, label]) => (
                    <option key={key} value={key}>
                      {label}
                    </option>
                  ))}
                </select>
              </label>
              <label>
                指令
                <textarea
                  required
                  value={form.instructions}
                  onChange={(e) => update({ instructions: e.target.value })}
                />
              </label>
              <label>
                Model 精确版本
                <select
                  required
                  value={form.model_ref}
                  onChange={(e) => update({ model_ref: e.target.value })}
                >
                  <option value="">选择 Model</option>
                  {options("model").map((r) => (
                    <option key={r.id} value={r.id}>
                      {r.name} v{r.version} · {r.data.provider}/{r.data.model}
                    </option>
                  ))}
                </select>
              </label>
              <label>
                Harness 精确版本
                <select
                  required
                  value={form.harness_ref}
                  onChange={(e) => update({ harness_ref: e.target.value })}
                >
                  <option value="">选择 Harness</option>
                  {options("harness").map((r) => (
                    <option key={r.id} value={r.id}>
                      {r.name} v{r.version} · {r.data.provider}/
                      {r.data.runtime_version}
                    </option>
                  ))}
                </select>
              </label>
              {multi("skills", "Skills", skills)}
              {multi("tools", "Tools（含显式原生工具组）", options("tool"))}
              {["Harness Permission", "Sandbox Permission"].map(
                (label, index) => (
                  <label key={label}>
                    {label}
                    <select
                      required
                      value={
                        form.permissions?.[index] || form.permissions?.[0] || ""
                      }
                      onChange={(e) => {
                        const chosen = [
                          form.permissions?.[0] || e.target.value,
                          form.permissions?.[1] ||
                            form.permissions?.[0] ||
                            e.target.value,
                        ];
                        chosen[index] = e.target.value;
                        update({ permissions: chosen });
                      }}
                    >
                      <option value="">选择精确权限版本</option>
                      {permissions.map((p) => (
                        <option key={p.id} value={p.id}>
                          {p.name} v{p.version}
                        </option>
                      ))}
                    </select>
                  </label>
                ),
              )}
              <label>
                审批策略
                <select
                  value={form.approval}
                  onChange={(e) => update({ approval: e.target.value })}
                >
                  <option value="never">不请求审批</option>
                  <option value="on_request">按需请求审批</option>
                </select>
              </label>
              <p>
                {selectedModel?.data.provider}/{selectedModel?.data.model} +{" "}
                {selectedHarness?.data.provider}/
                {selectedHarness?.data.runtime_version}
              </p>
            </>
          )}
          {kind === "model" && (
            <>
              {form.provider === "openai" && (
                <>
                  <button
                    type="button"
                    disabled={busy}
                    onClick={() =>
                      void act(async () => {
                        const result = await request("codex.auth.models");
                        setModelChoices(result.models);
                      })
                    }
                  >
                    读取可用模型目录
                  </button>
                  <datalist id="composition-models">
                    {modelChoices.map((m) => (
                      <option key={m.model} value={m.model}>
                        {m.displayName || m.model}
                      </option>
                    ))}
                  </datalist>
                </>
              )}
              <label>
                模型提供方
                <select
                  value={form.provider}
                  onChange={(e) => update({ provider: e.target.value })}
                >
                  <option value="openai">OpenAI</option>
                  <option value="deepseek">DeepSeek</option>
                </select>
              </label>
              <label>
                模型标识
                <input
                  required
                  list="composition-models"
                  value={form.model}
                  onChange={(e) => update({ model: e.target.value })}
                />
              </label>
              <label>
                发布标识
                <input
                  value={form.release || ""}
                  onChange={(e) => update({ release: e.target.value })}
                />
              </label>
              {form.provider === "openai" ? (
                <label>
                  推理强度
                  <select
                    value={form.parameters.reasoning_effort || ""}
                    onChange={(e) =>
                      update({
                        parameters: e.target.value
                          ? { reasoning_effort: e.target.value }
                          : {},
                      })
                    }
                  >
                    <option value="">模型默认</option>
                    {["low", "medium", "high", "xhigh"].map((x) => (
                      <option key={x}>{x}</option>
                    ))}
                  </select>
                </label>
              ) : (
                <label>
                  最大输出 Tokens
                  <input
                    type="number"
                    min={1024}
                    max={131072}
                    value={form.parameters.max_tokens || 8192}
                    onChange={(e) =>
                      update({
                        parameters: { max_tokens: Number(e.target.value) },
                      })
                    }
                  />
                </label>
              )}
            </>
          )}
          {kind === "harness" && (
            <>
              <button
                type="button"
                disabled={busy}
                onClick={() =>
                  void act(async () => {
                    const runtime = await request("runtime.prepare", {
                      provider: form.provider,
                    });
                    update({ runtime_version: runtime.key.version });
                    setMessage("已准备运行环境并填入精确版本。");
                  })
                }
              >
                准备 Harness 并选择精确版本
              </button>
              <label>
                运行适配器
                <select
                  value={form.provider}
                  onChange={(e) =>
                    update({
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
              <label>
                精确运行版本
                <input
                  required
                  list="agent-runtime-versions"
                  value={form.runtime_version}
                  onChange={(e) => update({ runtime_version: e.target.value })}
                />
                <datalist id="agent-runtime-versions">
                  {installations
                    .filter((r) => r.key.provider === form.provider)
                    .map((r) => (
                      <option key={r.id} value={r.version || r.key.version} />
                    ))}
                </datalist>
              </label>
              <label>
                能力（逗号分隔）
                <input
                  value={form.capabilities.join(", ")}
                  onChange={(e) =>
                    update({ capabilities: split(e.target.value) })
                  }
                />
              </label>
            </>
          )}
          {kind === "tool" && (
            <>
              <label>
                限定名称
                <input
                  required
                  value={form.qualified_name}
                  onChange={(e) => update({ qualified_name: e.target.value })}
                />
              </label>
              <label>
                执行处理器
                <select
                  value={form.implementation}
                  onChange={(e) => update({ implementation: e.target.value })}
                >
                  <option value="harness.native">Harness 原生工具组</option>
                  <option value="experience.lookup">
                    AgentID 经验检索（只读）
                  </option>
                </select>
              </label>
              <label>
                所需能力
                <input
                  value={form.capabilities.join(", ")}
                  onChange={(e) =>
                    update({ capabilities: split(e.target.value) })
                  }
                />
              </label>
              <p className="muted">
                工具契约使用协调器固定处理器；Skill 不会隐式授予工具权限。
              </p>
            </>
          )}
          {["skill", "spec", "standard"].includes(kind) && (
            <>
              <label>
                定义正文
                <textarea
                  required
                  value={form.body || ""}
                  onChange={(e) => update({ body: e.target.value })}
                />
              </label>
              {kind === "spec" && (
                <label>
                  检查规则
                  <select
                    multiple
                    value={form.rules || []}
                    onChange={(e) =>
                      update({
                        rules: Array.from(e.target.selectedOptions).map(
                          (o) => o.value,
                        ),
                      })
                    }
                  >
                    {["traceability", "failure_paths", "sections"].map(
                      (rule) => (
                        <option key={rule}>{rule}</option>
                      ),
                    )}
                  </select>
                </label>
              )}
              {kind === "standard" &&
                ["planning", "implementation", "merge"].map((purpose) => (
                  <label key={purpose}>
                    {purpose} Spec
                    <select
                      required
                      value={
                        (form.slots || []).find(
                          (s: Row) => s.purpose === purpose,
                        )?.spec || ""
                      }
                      onChange={(e) =>
                        update({
                          slots: [
                            ...(form.slots || []).filter(
                              (s: Row) => s.purpose !== purpose,
                            ),
                            { purpose, spec: e.target.value },
                          ],
                        })
                      }
                    >
                      <option value="">选择精确 Spec 版本</option>
                      {options("spec").map((r) => (
                        <option key={r.id} value={r.id}>
                          {r.name} v{r.version} · {r.scope.kind}
                        </option>
                      ))}
                    </select>
                  </label>
                ))}
            </>
          )}
        </div>
        <div className="actions">
          <button className="primary" disabled={busy}>
            {old ? "保存为新版本" : "创建定义"}
          </button>
          {old && (
            <button type="button" onClick={() => chooseKind(kind)}>
              新建另一个定义
            </button>
          )}
        </div>
      </form>
      <h3>创建 AgentID</h3>
      <form
        className="actions"
        onSubmit={(e) => {
          e.preventDefault();
          void act(async () => {
            await request("agent.instance.create", {
              project_id: project?.id,
              command_id: crypto.randomUUID(),
              payload: {
                name: instanceName,
                agent_revision_id: instanceAgent,
                scope:
                  instanceScope === "project" && project
                    ? { kind: "project", project_id: project.id }
                    : { kind: "global" },
              },
            });
            await load();
            setMessage("AgentID 已创建，组合与 Scope 已固定。");
          });
        }}
      >
        <label>
          实例名称
          <input
            required
            value={instanceName}
            onChange={(e) => setInstanceName(e.target.value)}
          />
        </label>
        <label>
          完整 AgentRevision
          <select
            required
            value={instanceAgent}
            onChange={(e) => setInstanceAgent(e.target.value)}
          >
            <option value="">选择 Agent</option>
            {options("agent").map((r) => (
              <option key={r.id} value={r.id}>
                {r.name} v{r.version} · {r.scope.kind}
              </option>
            ))}
          </select>
        </label>
        <label>
          经验范围
          <select
            value={project ? instanceScope : "global"}
            onChange={(e) => setInstanceScope(e.target.value)}
          >
            <option value="global">Global · 跨项目积累</option>
            {project && (
              <option value="project">Project · 只在本项目积累</option>
            )}
          </select>
        </label>
        <button disabled={busy}>创建实例</button>
      </form>
      <details>
        <summary>实例及冻结组成</summary>
        {options("agent_instance").map((r) => (
          <article key={r.id}>
            <strong>{r.name}</strong>
            <p>
              AgentID {r.id} · {r.scope.kind} · AgentRevision{" "}
              {r.data.agent_revision_id}
            </p>
          </article>
        ))}
      </details>
      <div className="actions">
        <button disabled={page === 0} onClick={() => setPage((p) => p - 1)}>
          上一页
        </button>
        <span>
          配置目录 {page + 1} · 共 {total} 项
        </span>
        <button
          disabled={(page + 1) * 100 >= total}
          onClick={() => setPage((p) => p + 1)}
        >
          下一页
        </button>
      </div>
      {message && (
        <p role="status" className="notice">
          {message}
        </p>
      )}
      {error && (
        <p role="alert" className="error">
          {error}
        </p>
      )}
    </section>
  );
}

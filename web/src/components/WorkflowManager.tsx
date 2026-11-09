import { useCallback, useEffect, useRef, useState } from "react";

type Row = Record<string, any>;
type Props = {
  request: (method: string, params?: Row) => Promise<any>;
  project?: Row;
  refresh: () => Promise<void>;
};
const roles: Record<string, string> = {
  analyst: "需求分析师",
  architect: "架构师",
  developer: "开发",
  tester: "测试",
  reviewer: "评审",
};
const actions: Record<string, string> = {
  "agent.interact": "角色讨论",
  "proposal.adopt": "提案采纳",
  "attempt.execute": "正式执行",
  "delivery.review": "交付评审",
  "merge.prepare": "准备候选",
  "merge.review": "候选评审",
  "merge.apply": "应用合入",
  "snapshot.capture": "上下文快照",
  "resource.cleanup": "资源释放",
  "tasks.foreach": "Task 子流程",
  branch: "条件分支",
  join: "汇合",
};
const list = (value: string) =>
  value
    .split(/[,，\n]/)
    .map((x) => x.trim())
    .filter(Boolean);

export function WorkflowManager({ request, project, refresh }: Props) {
  const [rows, setRows] = useState<Row[]>([]),
    [agents, setAgents] = useState<Row[]>([]),
    [binding, setBinding] = useState<Row>(),
    [standards, setStandards] = useState<Row[]>([]);
  const [selected, setSelected] = useState<Row>(),
    [document, setDocument] = useState<Row>(),
    [stepKey, setStepKey] = useState("");
  const [name, setName] = useState(""),
    [scope, setScope] = useState("global"),
    [template, setTemplate] = useState("builtin-workflow-feature-sol-high-v1"),
    [processKind, setProcessKind] = useState("feature");
  const [status, setStatus] = useState(""),
    [error, setError] = useState(""),
    [busy, setBusy] = useState(false),
    [dirty, setDirty] = useState(false),
    [validation, setValidation] = useState<Row>();
  const [workflowRef, setWorkflowRef] = useState(""),
    [standardRef, setStandardRef] = useState(""),
    [agentRefs, setAgentRefs] = useState<Record<string, string>>({}),
    [page, setPage] = useState(0),
    [total, setTotal] = useState(0);
  const [preview, setPreview] = useState<Row>(),
    [facts, setFacts] = useState<string[]>([]),
    [acceptedFacts, setAcceptedFacts] = useState<string[]>([]),
    [diff, setDiff] = useState<Row>();
  const generation = useRef(0);
  const initialWorkflow = useRef(
    new URLSearchParams(location.search).get("workflow_id"),
  );
  const load = useCallback(async () => {
    const current = ++generation.current;
    const [workflows, compositions, configuration] = await Promise.all([
      request("query.workflows", {
        local: !!project,
        project_id: project?.id,
        offset: page * 50,
        limit: 50,
      }),
      request("query.configurations", {
        local: !!project,
        project_id: project?.id,
        kind: "agent",
        limit: 100,
      }),
      project
        ? request("query.project_configuration", { project_id: project.id })
        : Promise.resolve(undefined),
    ]);
    if (current !== generation.current) return;
    setRows(workflows.records);
    setTotal(workflows.total);
    setAgents(compositions.records);
    setBinding(configuration?.configuration);
    setStandards(configuration?.standards || []);
    if (configuration) {
      setWorkflowRef(
        configuration.configuration?.data.workflow_ref ||
          "builtin-workflow-feature-sol-high-v1",
      );
      setStandardRef(
        configuration.configuration?.data.standard_ref ||
          configuration.standards[0]?.id ||
          "",
      );
      setAgentRefs(configuration.configuration?.data.agent_refs || {});
    }
  }, [request, project?.id, page]);
  useEffect(() => {
    load().catch((e) => setError(String(e)));
    return () => {
      ++generation.current;
    };
  }, [load]);
  useEffect(() => {
    if (!initialWorkflow.current) return;
    const id = initialWorkflow.current;
    initialWorkflow.current = null;
    let current = true;
    request("query.workflow", { id })
      .then((row) => {
        if (current) choose(row);
      })
      .catch((e) => {
        if (current) setError(String(e));
      });
    return () => {
      current = false;
    };
  }, [request]);
  const mutate = (
    method: string,
    target: string | undefined,
    payload: Row = {},
    revision?: number,
  ) =>
    request(method, {
      project_id: project?.id,
      command_id: crypto.randomUUID(),
      target,
      expected_revision: revision,
      payload,
    });
  async function action(fn: () => Promise<void>) {
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
  function choose(row: Row) {
    const params = new URLSearchParams(location.search);
    params.set("workflow_id", row.id);
    window.history.replaceState(null, "", `?${params}`);
    setSelected(row);
    setDocument(
      structuredClone(
        row.kind === "workflow_draft" ? row.data.document : row.data,
      ),
    );
    setStepKey((row.data.document || row.data).steps[0]?.key || "");
    setDirty(false);
    setValidation(undefined);
    setDiff(undefined);
    setPreview(undefined);
    setStatus("");
  }
  const editable =
    selected?.kind === "workflow_draft" && !selected.data.published_revision_id;
  const step = document?.steps.find((s: Row) => s.key === stepKey);
  const update = (patch: Row) => {
    setDocument((old) => ({ ...old, ...patch }));
    setDirty(true);
    setValidation(undefined);
    setPreview(undefined);
    setStatus("未保存");
  };
  const updateStep = (patch: Row) =>
    update({
      steps: document!.steps.map((s: Row) =>
        s.key === stepKey ? { ...s, ...patch } : s,
      ),
    });
  async function save() {
    if (!selected || !document) return;
    setStatus("保存中…");
    const saved = await mutate(
      "workflow.draft.update",
      selected.id,
      { document, name: selected.name },
      selected.revision,
    );
    setSelected(saved);
    setDirty(false);
    setStatus("已保存");
    await load();
  }
  const published = rows.filter((r) => r.kind === "workflow");
  const targetWorkflow = published.find((r) => r.id === workflowRef);
  const validate = async (previewMode = false) => {
    const result = await request(
      previewMode ? "query.workflow_preview" : "query.workflow_validation",
      {
        document,
        sample: { outputs: facts, accepted: acceptedFacts },
        local: !!project,
      },
    );
    setValidation(result);
    if (previewMode) setPreview(result);
  };
  return (
    <>
      <section className="panel">
        <div className="panel-heading">
          <div>
            <h2>Workflow 定义与版本</h2>
            <p className="muted">
              定义交互、交付和推进机制。已发布版本不可修改；Project
              换绑只影响之后创建的 Feature。
            </p>
          </div>
        </div>
        <form
          className="actions"
          onSubmit={(e) => {
            e.preventDefault();
            void action(async () => {
              const scopeValue =
                scope === "global"
                  ? { kind: "global" }
                  : { kind: "project", project_id: project!.id };
              const row = await mutate("workflow.create", undefined, {
                name,
                scope: scopeValue,
                ...(template
                  ? { template_ref: template }
                  : {
                      document: {
                        schema_version: 1,
                        process_kind: processKind,
                        stages: [{ key: "work", name: "工作" }],
                        steps: [],
                        binding_slots: {},
                        subworkflow_refs: {},
                      },
                    }),
              });
              choose(row);
              setName("");
              await load();
            });
          }}
        >
          <label>
            流程名称
            <input
              required
              value={name}
              onChange={(e) => setName(e.target.value)}
            />
          </label>
          <label>
            定义 Scope
            <select value={scope} onChange={(e) => setScope(e.target.value)}>
              <option value="global">Global</option>
              {project && <option value="project">当前 Project</option>}
            </select>
          </label>
          <label>
            模板
            <select
              value={template}
              onChange={(e) => setTemplate(e.target.value)}
            >
              <option value="">空白草稿</option>
              {published.map((r) => (
                <option key={r.id} value={r.id}>
                  {r.name} v{r.version}
                </option>
              ))}
            </select>
          </label>
          {!template && (
            <label>
              用途
              <select
                value={processKind}
                onChange={(e) => setProcessKind(e.target.value)}
              >
                <option value="feature">Feature</option>
                <option value="task">Task</option>
                <option value="project_intake">Project 需求入口</option>
              </select>
            </label>
          )}
          <button disabled={busy || !name.trim()}>新建 Workflow</button>
        </form>
        <div className="workflow-list">
          {rows.map((row) => (
            <button
              key={row.id}
              className={selected?.id === row.id ? "selected" : ""}
              onClick={() => choose(row)}
            >
              <strong>{row.name}</strong>
              <span>
                {row.scope.kind} ·{" "}
                {row.kind === "workflow_draft"
                  ? row.data.published_revision_id
                    ? "已发布草稿"
                    : "草稿"
                  : `v${row.version}`}{" "}
                {binding?.data.workflow_ref === row.id
                  ? " · Project 正在使用"
                  : ""}
              </span>
            </button>
          ))}
        </div>
        <div className="actions">
          <button disabled={page === 0} onClick={() => setPage((p) => p - 1)}>
            上一页
          </button>
          <span>
            {page + 1} / {Math.max(1, Math.ceil(total / 50))}
          </span>
          <button
            disabled={(page + 1) * 50 >= total}
            onClick={() => setPage((p) => p + 1)}
          >
            下一页
          </button>
        </div>
      </section>
      {selected && document && (
        <section className="panel">
          <div className="panel-heading">
            <div>
              <h2>{selected.name}</h2>
              <p className="muted">
                {selected.scope.kind} ·{" "}
                {editable
                  ? `草稿 revision ${selected.revision}`
                  : `发布版本 v${selected.version}`}{" "}
                · {document.process_kind}
              </p>
            </div>
            <span role="status">{status}</span>
          </div>
          <div className="actions">
            {editable ? (
              <>
                <button
                  disabled={busy || !dirty}
                  onClick={() => void action(save)}
                >
                  保存草稿
                </button>
                <button
                  disabled={busy}
                  onClick={() => void action(() => validate())}
                >
                  校验流程
                </button>
                <button
                  disabled={busy}
                  onClick={() => void action(() => validate(true))}
                >
                  预演
                </button>
                <button
                  className="primary"
                  disabled={busy}
                  onClick={() =>
                    void action(async () => {
                      if (dirty) await save();
                      const current = await request("query.workflow_draft", {
                        id: selected.id,
                      });
                      const checked = await request(
                        "query.workflow_validation",
                        { document: current.data.document },
                      );
                      setValidation(checked);
                      if (!checked.valid) return;
                      const row = await mutate(
                        "workflow.publish",
                        current.id,
                        {},
                        current.revision,
                      );
                      choose(row);
                      await load();
                    })
                  }
                >
                  发布版本
                </button>
              </>
            ) : (
              <>
                <button
                  disabled={busy}
                  onClick={() =>
                    void action(async () => {
                      const row = await mutate(
                        "workflow.draft.create",
                        undefined,
                        { base_revision_id: selected.id },
                      );
                      choose(row);
                      await load();
                    })
                  }
                >
                  基于此版本编辑
                </button>
                {project && selected.scope.kind === "global" && (
                  <button
                    disabled={busy}
                    onClick={() =>
                      void action(async () => {
                        choose(
                          await mutate("workflow.draft.create", undefined, {
                            base_revision_id: selected.id,
                            scope: { kind: "project", project_id: project.id },
                            name: `${selected.name} · ${project.title}`,
                          }),
                        );
                        await load();
                      })
                    }
                  >
                    派生为项目流程
                  </button>
                )}
                <button onClick={() => void action(() => validate(true))}>
                  预演
                </button>
              </>
            )}
            {!editable && selected.kind === "workflow" && (
              <button
                disabled={busy}
                onClick={() =>
                  void action(async () => {
                    choose(
                      await mutate("workflow.create", undefined, {
                        name: selected.name + " 副本",
                        scope: selected.scope,
                        document,
                      }),
                    );
                    await load();
                  })
                }
              >
                复制为新流程
              </button>
            )}
            {selected.data.base_revision_id && (
              <button
                onClick={() =>
                  void action(async () => {
                    const base = await request("query.workflow", {
                      id: selected.data.base_revision_id,
                    });
                    setDiff({ base: base.data, current: document });
                  })
                }
              >
                与基线比较
              </button>
            )}
          </div>
          <div className="workflow-editor">
            <aside className="workflow-stages">
              <h3>阶段与子流程</h3>
              {document.stages.map((stage: Row) => (
                <div key={stage.key}>
                  {editable ? (
                    <label>
                      阶段名称
                      <input
                        value={stage.name}
                        onChange={(e) =>
                          update({
                            stages: document.stages.map((s: Row) =>
                              s.key === stage.key
                                ? { ...s, name: e.target.value }
                                : s,
                            ),
                          })
                        }
                      />
                    </label>
                  ) : (
                    <strong>{stage.name}</strong>
                  )}
                  {document.steps
                    .filter((s: Row) => s.stage === stage.key)
                    .map((s: Row) => (
                      <button
                        key={s.key}
                        className={s.key === stepKey ? "selected" : ""}
                        onClick={() => setStepKey(s.key)}
                      >
                        {s.name}
                      </button>
                    ))}
                </div>
              ))}
              {editable && (
                <>
                  <button
                    onClick={() => {
                      const key = `stage_${crypto.randomUUID().slice(0, 8)}`;
                      update({
                        stages: [...document.stages, { key, name: "新阶段" }],
                      });
                    }}
                  >
                    添加阶段
                  </button>
                  <label>
                    Task 子流程版本
                    <select
                      value={document.subworkflow_refs.task || ""}
                      onChange={(e) =>
                        update({
                          subworkflow_refs: {
                            ...document.subworkflow_refs,
                            task: e.target.value,
                          },
                        })
                      }
                    >
                      <option value="">未选择</option>
                      {published
                        .filter((r) => r.data.process_kind === "task")
                        .map((r) => (
                          <option key={r.id} value={r.id}>
                            {r.name} v{r.version}
                          </option>
                        ))}
                    </select>
                  </label>
                </>
              )}
            </aside>
            <div className="workflow-preview" aria-label="流程预览">
              {document.steps.map((s: Row, i: number) => (
                <button
                  key={s.key}
                  onClick={() => setStepKey(s.key)}
                  className={s.key === stepKey ? "selected" : ""}
                >
                  <span className="eyebrow">
                    {i + 1} · {actions[s.kind]}
                  </span>
                  <strong>{s.name}</strong>
                  <small>
                    前置：{s.dependencies.join("、") || "起点"} ·{" "}
                    {s.advance_mode === "automatic" ? "自动推进" : "待人确认"}
                  </small>
                </button>
              ))}
              {editable && (
                <button
                  onClick={() => {
                    const key = `step_${crypto.randomUUID().slice(0, 8)}`;
                    update({
                      steps: [
                        ...document.steps,
                        {
                          key,
                          name: "新步骤",
                          stage: document.stages[0].key,
                          kind: "snapshot.capture",
                          contract_version: 1,
                          dependencies: [],
                          participant_slot: null,
                          interaction: {
                            mode: "discussion",
                            goal: "",
                            required_topics: [],
                            output_kind: "",
                            max_turns: 30,
                          },
                          advance_mode: "automatic",
                          condition: { op: "always" },
                          parameters: {},
                          execution_recipe: [],
                          rework_scope: [],
                          retry_limit: 3,
                        },
                      ],
                    });
                    setStepKey(key);
                  }}
                >
                  添加步骤
                </button>
              )}
            </div>
            {step && (
              <fieldset
                disabled={!editable || busy}
                className="workflow-step-form"
              >
                <legend>步骤配置 · {step.key}</legend>
                <label>
                  步骤名称
                  <input
                    value={step.name}
                    onChange={(e) => updateStep({ name: e.target.value })}
                  />
                </label>
                <label>
                  动作
                  <select
                    value={step.kind}
                    onChange={(e) => updateStep({ kind: e.target.value })}
                  >
                    {Object.entries(actions).map(([key, name]) => (
                      <option key={key} value={key}>
                        {name}
                      </option>
                    ))}
                  </select>
                </label>
                <label>
                  阶段
                  <select
                    value={step.stage}
                    onChange={(e) => updateStep({ stage: e.target.value })}
                  >
                    {document.stages.map((s: Row) => (
                      <option key={s.key} value={s.key}>
                        {s.name}
                      </option>
                    ))}
                  </select>
                </label>
                <label>
                  前置步骤
                  <select
                    multiple
                    value={step.dependencies}
                    onChange={(e) =>
                      updateStep({
                        dependencies: Array.from(e.target.selectedOptions).map(
                          (o) => o.value,
                        ),
                      })
                    }
                  >
                    {document.steps
                      .filter((s: Row) => s.key !== step.key)
                      .map((s: Row) => (
                        <option key={s.key} value={s.key}>
                          {s.name}
                        </option>
                      ))}
                  </select>
                </label>
                <label>
                  参与角色
                  <select
                    value={step.participant_slot || ""}
                    onChange={(e) =>
                      updateStep({ participant_slot: e.target.value || null })
                    }
                  >
                    <option value="">协调器</option>
                    {Object.entries(document.binding_slots).map(
                      ([key, slot]: [string, any]) => (
                        <option key={key} value={key}>
                          {roles[slot.role] || key}
                        </option>
                      ),
                    )}
                  </select>
                </label>
                <details>
                  <summary>输入端口与上游产物</summary>
                  {(step.kind === "proposal.adopt"
                    ? ["proposal"]
                    : ["delivery.review", "merge.review"].includes(step.kind)
                      ? ["delivery"]
                      : step.kind === "merge.apply"
                        ? ["merge"]
                        : step.kind === "tasks.foreach"
                          ? ["proposal"]
                          : []
                  ).map((port) => (
                    <label key={port}>
                      {port} 来源
                      <select
                        value={step.inputs?.[port]?.step || ""}
                        onChange={(e) => {
                          const inputs = { ...(step.inputs || {}) };
                          if (e.target.value)
                            inputs[port] = { step: e.target.value, port };
                          else delete inputs[port];
                          updateStep({
                            inputs,
                            dependencies: e.target.value
                              ? Array.from(
                                  new Set([
                                    ...step.dependencies,
                                    e.target.value,
                                  ]),
                                )
                              : step.dependencies,
                          });
                        }}
                      >
                        <option value="">按前置步骤契约解析</option>
                        {document.steps
                          .filter(
                            (s: Row) =>
                              s.key !== step.key &&
                              (port === "proposal"
                                ? ["agent.interact", "proposal.adopt"].includes(
                                    s.kind,
                                  )
                                : port === "delivery"
                                  ? [
                                      "attempt.execute",
                                      "delivery.review",
                                      "merge.prepare",
                                      "merge.review",
                                    ].includes(s.kind)
                                  : s.kind === "merge.review"),
                          )
                          .map((s: Row) => (
                            <option key={s.key} value={s.key}>
                              {s.name} · {port}
                            </option>
                          ))}
                      </select>
                    </label>
                  ))}
                </details>
                {step.kind === "attempt.execute" && (
                  <label>
                    执行阶段
                    <select
                      value={step.parameters.phase || ""}
                      onChange={(e) =>
                        updateStep({
                          parameters: {
                            ...step.parameters,
                            phase: e.target.value,
                          },
                        })
                      }
                    >
                      {[
                        "requirement_definition",
                        "solution_planning",
                        "implementation",
                        "closure",
                      ].map((p) => (
                        <option key={p}>{p}</option>
                      ))}
                    </select>
                  </label>
                )}
                {step.kind === "agent.interact" && (
                  <>
                    <label>
                      交互方式
                      <select
                        value={step.interaction.mode}
                        onChange={(e) =>
                          updateStep({
                            interaction: {
                              ...step.interaction,
                              mode: e.target.value,
                            },
                          })
                        }
                      >
                        {["brainstorm", "clarification", "discussion"].map(
                          (p) => (
                            <option key={p}>{p}</option>
                          ),
                        )}
                      </select>
                    </label>
                    <label>
                      讨论目标
                      <textarea
                        aria-label="讨论目标"
                        value={step.interaction.goal}
                        onChange={(e) =>
                          updateStep({
                            interaction: {
                              ...step.interaction,
                              goal: e.target.value,
                            },
                          })
                        }
                      />
                    </label>
                    <label>
                      必答主题（每行一个）
                      <textarea
                        value={step.interaction.required_topics.join("\n")}
                        onChange={(e) =>
                          updateStep({
                            interaction: {
                              ...step.interaction,
                              required_topics: list(e.target.value),
                            },
                          })
                        }
                      />
                    </label>
                    <label>
                      期望产物
                      <select
                        value={step.interaction.output_kind}
                        onChange={(e) =>
                          updateStep({
                            interaction: {
                              ...step.interaction,
                              output_kind: e.target.value,
                            },
                          })
                        }
                      >
                        {[
                          "requirement",
                          "design",
                          "task_plan",
                          "test_plan",
                          "feature",
                        ].map((p) => (
                          <option key={p}>{p}</option>
                        ))}
                      </select>
                    </label>
                    <label>
                      讨论轮数上限
                      <input
                        type="number"
                        min={1}
                        max={100}
                        value={step.interaction.max_turns}
                        onChange={(e) =>
                          updateStep({
                            interaction: {
                              ...step.interaction,
                              max_turns: Number(e.target.value),
                            },
                          })
                        }
                      />
                    </label>
                  </>
                )}
                <label>
                  推进策略
                  <select
                    aria-label="推进策略"
                    value={step.advance_mode}
                    onChange={(e) =>
                      updateStep({ advance_mode: e.target.value })
                    }
                  >
                    <option value="automatic">条件满足后自动推进</option>
                    <option value="human_confirmation">
                      结果完成后待人确认
                    </option>
                  </select>
                </label>
                <details>
                  <summary>条件、技术重试与返工</summary>
                  <label>
                    条件
                    <select
                      value={step.condition.op}
                      onChange={(e) =>
                        updateStep({
                          condition:
                            e.target.value === "always"
                              ? { op: "always" }
                              : e.target.value === "accepted"
                                ? { op: "accepted", kind: "requirement" }
                                : {
                                    op: "output_present",
                                    step: step.dependencies[0] || "",
                                  },
                        })
                      }
                    >
                      <option value="always">始终</option>
                      <option value="accepted">指定交付被接受</option>
                      <option value="output_present">前置步骤有产物</option>
                    </select>
                  </label>
                  {step.condition.op === "accepted" && (
                    <label>
                      交付类型
                      <select
                        value={step.condition.kind}
                        onChange={(e) =>
                          updateStep({
                            condition: {
                              ...step.condition,
                              kind: e.target.value,
                            },
                          })
                        }
                      >
                        {[
                          "requirement",
                          "plan",
                          "implementation",
                          "closure",
                          "merge_candidate",
                        ].map((k) => (
                          <option key={k}>{k}</option>
                        ))}
                      </select>
                    </label>
                  )}
                  {step.condition.op === "output_present" && (
                    <label>
                      产物来源
                      <select
                        value={step.condition.step}
                        onChange={(e) =>
                          updateStep({
                            condition: {
                              ...step.condition,
                              step: e.target.value,
                            },
                          })
                        }
                      >
                        {document.steps
                          .filter((s: Row) => s.key !== step.key)
                          .map((s: Row) => (
                            <option key={s.key} value={s.key}>
                              {s.name}
                            </option>
                          ))}
                      </select>
                    </label>
                  )}
                  <label>
                    技术重试上限
                    <input
                      type="number"
                      min={0}
                      max={10}
                      value={step.retry_limit}
                      onChange={(e) =>
                        updateStep({ retry_limit: Number(e.target.value) })
                      }
                    />
                  </label>
                  <label>
                    返工入口
                    <select
                      multiple
                      value={step.rework_scope}
                      onChange={(e) =>
                        updateStep({
                          rework_scope: Array.from(
                            e.target.selectedOptions,
                          ).map((o) => o.value),
                        })
                      }
                    >
                      {document.steps.map((s: Row) => (
                        <option key={s.key} value={s.key}>
                          {s.name}
                        </option>
                      ))}
                    </select>
                  </label>
                </details>
                {step.execution_recipe.length > 0 && (
                  <details>
                    <summary>执行配方与 TDD 依赖</summary>
                    {step.execution_recipe.map((node: Row, index: number) => (
                      <div key={node.key}>
                        <strong>{node.key}</strong>
                        <label>
                          职责
                          <select
                            value={node.role}
                            onChange={(e) =>
                              updateStep({
                                execution_recipe: step.execution_recipe.map(
                                  (n: Row, i: number) =>
                                    i === index
                                      ? { ...n, role: e.target.value }
                                      : n,
                                ),
                              })
                            }
                          >
                            {Object.entries(roles).map(([r, name]) => (
                              <option key={r} value={r}>
                                {name}
                              </option>
                            ))}
                          </select>
                        </label>
                        <label>
                          节点依赖
                          <select
                            multiple
                            value={node.dependencies}
                            onChange={(e) =>
                              updateStep({
                                execution_recipe: step.execution_recipe.map(
                                  (n: Row, i: number) =>
                                    i === index
                                      ? {
                                          ...n,
                                          dependencies: Array.from(
                                            e.target.selectedOptions,
                                          ).map((o) => o.value),
                                        }
                                      : n,
                                ),
                              })
                            }
                          >
                            {step.execution_recipe
                              .filter((n: Row) => n.key !== node.key)
                              .map((n: Row) => (
                                <option key={n.key}>{n.key}</option>
                              ))}
                          </select>
                        </label>
                      </div>
                    ))}
                  </details>
                )}
                <div className="actions">
                  <button
                    onClick={() => {
                      const steps = [...document.steps],
                        i = steps.findIndex((s) => s.key === stepKey);
                      if (i > 0) {
                        [steps[i - 1], steps[i]] = [steps[i], steps[i - 1]];
                        update({ steps });
                      }
                    }}
                  >
                    上移
                  </button>
                  <button
                    onClick={() => {
                      const copy = structuredClone(step);
                      copy.key = `step_${crypto.randomUUID().slice(0, 8)}`;
                      copy.name += " 副本";
                      update({ steps: [...document.steps, copy] });
                      setStepKey(copy.key);
                    }}
                  >
                    复制步骤
                  </button>
                  <button
                    onClick={() => {
                      update({
                        steps: document.steps.filter(
                          (s: Row) => s.key !== stepKey,
                        ),
                      });
                      setStepKey("");
                    }}
                  >
                    删除步骤
                  </button>
                </div>
              </fieldset>
            )}
          </div>
          <details>
            <summary>角色 slot 的默认完整 Agent</summary>
            {editable && (
              <button
                onClick={() =>
                  update({
                    binding_slots: Object.fromEntries(
                      Object.keys(roles).map((key) => [
                        key,
                        document.binding_slots[key] || {
                          role: key,
                          agent_ref: null,
                          capabilities: [],
                        },
                      ]),
                    ),
                  })
                }
              >
                补齐五种工程职责
              </button>
            )}
            {Object.entries(document.binding_slots).map(
              ([key, slot]: [string, any]) => (
                <label key={key}>
                  {roles[slot.role] || key}
                  <select
                    disabled={!editable}
                    value={slot.agent_ref || ""}
                    onChange={(e) =>
                      update({
                        binding_slots: {
                          ...document.binding_slots,
                          [key]: { ...slot, agent_ref: e.target.value || null },
                        },
                      })
                    }
                  >
                    <option value="">Project 绑定时选择</option>
                    {agents
                      .filter((a) => a.data.role === slot.role)
                      .map((a) => (
                        <option key={a.id} value={a.id}>
                          {a.name} v{a.version} · {a.data.model_ref.slice(0, 8)}
                        </option>
                      ))}
                  </select>
                </label>
              ),
            )}
          </details>
          {validation && (
            <div
              className={validation.valid ? "notice" : "error"}
              role="status"
            >
              {validation.valid
                ? "校验通过"
                : validation.errors?.map((e: Row) => e.message).join("；")}
            </div>
          )}
          {preview && (
            <div className="notice">
              <strong>演练 · 不产生工程完成事实</strong>
              <label>
                样本已产生输出的步骤
                <select
                  multiple
                  value={facts}
                  onChange={(e) =>
                    setFacts(
                      Array.from(e.target.selectedOptions).map((o) => o.value),
                    )
                  }
                >
                  {document.steps.map((s: Row) => (
                    <option key={s.key} value={s.key}>
                      {s.name}
                    </option>
                  ))}
                </select>
              </label>
              <label>
                样本已接受的交付
                <select
                  multiple
                  value={acceptedFacts}
                  onChange={(e) =>
                    setAcceptedFacts(
                      Array.from(e.target.selectedOptions).map((o) => o.value),
                    )
                  }
                >
                  {[
                    "requirement",
                    "plan",
                    "implementation",
                    "closure",
                    "merge_candidate",
                  ].map((kind) => (
                    <option key={kind}>{kind}</option>
                  ))}
                </select>
              </label>
              <button
                disabled={busy}
                onClick={() => void action(() => validate(true))}
              >
                按样本重新演练
              </button>
              <ol>
                {preview.simulation?.map((result: Row) => {
                  const key = result.key;
                  const s = document.steps.find((s: Row) => s.key === key);
                  return (
                    <li key={key}>
                      {s.name}：
                      {(
                        {
                          completed: "已完成",
                          skipped: "已跳过",
                          waiting_dependencies: "等待前置",
                          waiting_confirmation: "等待人确认",
                          waiting_condition: "等待条件",
                          waiting_input: "等待交互",
                          ready: "可执行",
                        } as Record<string, string>
                      )[result.state] || result.state}{" "}
                      · {result.reason}；前置{" "}
                      {s.dependencies.join("、") || "无"}；
                      {s.advance_mode === "human_confirmation"
                        ? "结果完成后等人确认"
                        : "满足条件后自动推进"}
                      ；职责 {s.participant_slot || "协调器"}
                      {s.kind === "tasks.foreach"
                        ? "，按采纳计划冻结成员后等待全部 Task"
                        : ""}
                      {s.condition.op !== "always"
                        ? `，条件 ${s.condition.op}`
                        : ""}
                    </li>
                  );
                })}
              </ol>
            </div>
          )}
          {diff && (
            <details open>
              <summary>基线与当前内容差异</summary>
              <div className="split">
                <pre>{JSON.stringify(diff.base, null, 2)}</pre>
                <pre>{JSON.stringify(diff.current, null, 2)}</pre>
              </div>
            </details>
          )}
          <details>
            <summary>只读定义与内容摘要</summary>
            <p>{selected.content_hash}</p>
            <pre>{JSON.stringify(document, null, 2)}</pre>
          </details>
        </section>
      )}
      {project && (
        <section className="panel">
          <h2>用于当前 Project</h2>
          <p>
            当前配置：
            {binding
              ? `v${binding.version} · ${binding.id.slice(0, 8)}`
              : "尚未配置"}
            。新配置用于之后创建的 Feature，已有 Feature 保持自己的配置锁。
          </p>
          <form
            onSubmit={(e) => {
              e.preventDefault();
              void action(async () => {
                await mutate(
                  "project.configuration.update",
                  project.id,
                  {
                    workflow_ref: workflowRef,
                    standard_ref: standardRef,
                    agent_refs: agentRefs,
                    source_configuration_id: binding?.id,
                  },
                  project.revision,
                );
                await load();
                await refresh();
                setStatus("Project 默认配置已保存");
              });
            }}
          >
            <div className="form-grid">
              <label>
                Workflow 精确版本
                <select
                  value={workflowRef}
                  onChange={(e) => setWorkflowRef(e.target.value)}
                >
                  {published
                    .filter((r) => r.data.process_kind === "feature")
                    .map((r) => (
                      <option key={r.id} value={r.id}>
                        {r.name} v{r.version} · {r.scope.kind}
                      </option>
                    ))}
                </select>
              </label>
              <label>
                Standard 精确版本
                <select
                  value={standardRef}
                  onChange={(e) => setStandardRef(e.target.value)}
                >
                  {standards.map((r) => (
                    <option key={r.id} value={r.id}>
                      {r.name} v{r.version}
                    </option>
                  ))}
                </select>
              </label>
              {Object.entries(targetWorkflow?.data.binding_slots || {}).map(
                ([key, slot]: [string, any]) => (
                  <label key={key}>
                    {roles[slot.role] || key}
                    <select
                      required
                      value={agentRefs[key] || slot.agent_ref || ""}
                      onChange={(e) =>
                        setAgentRefs((old) => ({
                          ...old,
                          [key]: e.target.value,
                        }))
                      }
                    >
                      <option value="">选择完整 AgentRevision</option>
                      {agents
                        .filter((a) => a.data.role === slot.role)
                        .map((a) => (
                          <option key={a.id} value={a.id}>
                            {a.name} v{a.version} · {a.scope.kind}
                          </option>
                        ))}
                    </select>
                  </label>
                ),
              )}
            </div>
            <button className="primary" disabled={busy}>
              保存 Project 默认配置
            </button>
          </form>
        </section>
      )}
      {error && (
        <div className="error" role="alert">
          {error}（本地草稿保留；版本冲突请重读后合并）
        </div>
      )}
    </>
  );
}

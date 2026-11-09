import { WorkflowTaskPreview } from "./WorkflowTaskPreview";
import { useCallback, useEffect, useId, useRef, useState } from "react";
import {
  actionLabels,
  dutyLabels,
  latest,
  loadCatalog,
  MarkdownField,
  MarkdownTopicsField,
  RevisionSelect,
  useDraft,
  type Row,
  type Request,
} from "./configuration-ui";
type Props = {
  request: Request;
  project?: Row;
  refresh: () => Promise<unknown>;
};
const nodeLabels: Record<string, string> = {
  test_author: "编写独立测试",
  red: "断言 Red",
  implement: "实现",
  green: "Green",
  docs: "文档",
  verify: "验证",
  feature_tests: "Feature 测试",
  requirement_validate: "需求验证",
  plan_validate: "方案验证",
  test_plan_validate: "测试计划验证",
};
const phases: Record<string, string> = {
  requirement_definition: "需求定义",
  solution_planning: "方案规划",
  implementation: "Task 实现",
  closure: "Feature 收尾",
};
const split = (text: string) =>
  text
    .split(/[,，\n]/)
    .map((v) => v.trim())
    .filter(Boolean);
const emptyDocument = () => ({
  schema_version: 3,
  process_kind: "feature",
  stages: [{ key: "work", name: "工作" }],
  steps: [],
  binding_slots: {},
  subworkflow_refs: {},
  input_ports: [],
  output_ports: {},
});
function outputNames(step: Row, records: Row[]): string[] {
  switch (step.kind) {
    case "agent.interact":
      return ["session", "proposal", "document"];
    case "proposal.adopt":
      return ["proposal", "document"];
    case "attempt.execute":
      return ["attempt", "delivery"];
    case "delivery.review":
      return ["delivery"];
    case "merge.prepare":
    case "merge.review":
      return ["merge", "delivery"];
    case "merge.apply":
      return ["merge"];
    case "snapshot.capture":
      return ["snapshot"];
    case "tasks.foreach":
      return ["group"];
    case "workflow.call":
      return Object.keys(
        records.find((r) => r.id === step.subworkflow_ref)?.data.output_ports ||
          {},
      );
    default:
      return [];
  }
}
function specificationContext(
  step: Row,
  steps: Row[],
  seen = new Set<string>(),
): { phase?: string; delivery?: string } {
  const phase =
    step.kind === "agent.interact"
      ? step.interaction?.output_kind
      : step.parameters?.phase;
  if (step.kind.startsWith("merge."))
    return { phase, delivery: "merge_candidate" };
  if (phase)
    return {
      phase,
      delivery:
        phase === "requirement_definition"
          ? "requirement"
          : phase === "solution_planning"
            ? "plan"
            : phase,
    };
  if (seen.has(step.key)) return {};
  seen.add(step.key);
  for (const key of step.dependencies || []) {
    const parent = steps.find((s) => s.key === key);
    if (parent) {
      const context = specificationContext(parent, steps, seen);
      if (context.delivery) return { delivery: context.delivery };
    }
  }
  return {};
}
export function WorkflowManager({ request, project, refresh }: Props) {
  const treeId = useId();
  const [rows, setRows] = useState<Row[]>([]),
    [records, setRecords] = useState<Row[]>([]),
    [contracts, setContracts] = useState<Row>({
      actions: [],
      recipe_nodes: [],
    }),
    [error, setError] = useState(""),
    [message, setMessage] = useState(""),
    [busy, setBusy] = useState(false),
    [validation, setValidation] = useState<Row>(),
    [moveStage, setMoveStage] = useState(""),
    [search, setSearch] = useState("");
  const [draft, setDraft] = useDraft(
    `devdesk:v2:workflow:${project?.id || "global"}`,
    () => ({
      selected: "",
      view: "list",
      name: "",
      scope: "global",
      document: emptyDocument() as Row,
      revision: 1,
      editing: false,
      node: "workflow",
      collapsedStages: [] as string[],
    }),
  );
  const initialRoute = useRef(false);
  const load = useCallback(async () => {
    const catalog = await loadCatalog(request, project);
    const workflows: Row[] = [];
    let offset = 0;
    for (;;) {
      const page = await request("query.workflows", {
        local: !!project,
        project_id: project?.id,
        offset,
        limit: 100,
        include_history: true,
      });
      workflows.push(...page.records);
      if (page.action_contracts) setContracts(page.action_contracts);
      offset += page.records.length;
      if (offset >= page.total || !page.records.length) break;
    }
    setRows(workflows);
    setRecords(catalog);
  }, [request, project?.id]);
  const choose = (row: Row) => {
    const doc = structuredClone(
      row.kind === "workflow_draft" ? row.data.document : row.data,
    );
    setDraft({
      selected: row.id,
      view: "editor",
      name: row.name,
      scope: row.scope.kind,
      document: doc,
      revision: row.revision,
      editing: row.kind === "workflow_draft" && !row.data.published_revision_id,
      node: "workflow",
      collapsedStages: [],
    });
    setValidation(undefined);
    setMessage("");
    const params = new URLSearchParams(location.search);
    params.set("workflow_id", row!.id);
    history.replaceState(null, "", `?${params}`);
  };
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
    if (initialRoute.current || !rows.length) return;
    initialRoute.current = true;
    const id = new URLSearchParams(location.search).get("workflow_id");
    if (id === draft.selected) return;
    if (id) {
      const row = rows.find((r) => r.id === id);
      if (row) choose(row);
      else
        void request("query.workflow", { id, project_id: project?.id })
          .then(choose)
          .catch((e) => setError(String(e)));
    }
  }, [rows]);
  const document = draft.document;
  const collapsedStages = draft.collapsedStages || [];
  const selected = rows.find((r) => r.id === draft.selected);
  const editable = draft.editing;
  const workflows = latest(
    rows.filter((r) => r.kind === "workflow" && r.data.schema_version >= 2),
  );
  const isTaskVersion = document.schema_version >= 3;
  const requiresTask = (kind: string) =>
    contracts.actions.some((a: Row) => a.kind === kind && a.requires_task);
  const requiresRole = (kind: string) =>
    contracts.actions.some((a: Row) => a.kind === kind && a.requires_role);
  const agentNode = (key: string) =>
    contracts.recipe_nodes.some((n: Row) => n.key === key && n.requires_task);
  const actionNames = Object.fromEntries(
    contracts.actions.map((a: Row) => [a.kind, a.label]),
  );
  const roles = latest(records.filter((r) => r.kind === "role"));
  const specs = records.filter((r) => r.kind === "spec");
  const stage = draft.node.startsWith("stage:")
    ? document.stages.find((s: Row) => s.key === draft.node.slice(6))
    : undefined;
  const step = draft.node.startsWith("step:")
    ? document.steps.find((s: Row) => s.key === draft.node.slice(5))
    : undefined;
  const update = (value: Row) =>
    setDraft((current) => ({
      ...current,
      document: { ...current.document, ...value },
    }));
  const updateStep = (value: Row) =>
    setDraft((current) => ({
      ...current,
      document: {
        ...current.document,
        steps: current.document.steps.map((s: Row) =>
          s.key === step?.key ? { ...s, ...value } : s,
        ),
      },
    }));
  const mutate = (
    method: string,
    target: string | undefined,
    payload: Row,
    revision?: number,
  ) =>
    request(method, {
      project_id: project?.id,
      command_id: crypto.randomUUID(),
      target,
      expected_revision: revision,
      payload,
    });
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
  async function save() {
    let row = selected;
    if (!row) {
      row = await mutate("workflow.create", undefined, {
        name: draft.name,
        scope:
          draft.scope === "project"
            ? { kind: "project", project_id: project!.id }
            : { kind: "global" },
        document,
      });
    } else {
      row = await mutate(
        "workflow.draft.update",
        row.id,
        { name: draft.name, document },
        draft.revision,
      );
    }
    await load();
    setDraft((current) => ({
      ...current,
      selected: row!.id,
      revision: row!.revision,
      editing: true,
    }));
    const params = new URLSearchParams(location.search);
    params.set("workflow_id", row!.id);
    history.replaceState(null, "", `?${params}`);
    setMessage("流程草稿已保存。");
    return row!;
  }
  async function validate() {
    const row = await request("query.workflow_requirements", {
      id: draft.selected,
      project_id: project?.id,
      document,
    });
    setValidation(row);
    setMessage("结构、Role、Spec 与子流程引用校验通过。");
  }
  function addStep(kind = "join") {
    const key = `step_${crypto.randomUUID().slice(0, 8)}`;
    const item = {
      key,
      name: kind === "workflow.call" ? "新子流程" : "新步骤",
      stage: stage?.key || step?.stage || document.stages[0]?.key,
      kind,
      contract_version: isTaskVersion && requiresTask(kind) ? 2 : 1,
      ...(isTaskVersion && requiresTask(kind) ? { task_instructions: "" } : {}),
      dependencies: step ? [step.key] : [],
      role_ref: null,
      spec_refs: [],
      inputs: {},
      interaction: {
        mode: "discussion",
        goal: "",
        required_topics: [],
        output_kind: "requirement",
        max_turns: 8,
      },
      advance_mode: "automatic",
      condition: { op: "always" },
      parameters: {},
      execution_recipe: [],
      rework_scope: [],
      retry_limit: 3,
      ...(kind === "workflow.call" ? { subworkflow_ref: "" } : {}),
    };
    update({ steps: [...document.steps, item] });
    setDraft((current) => ({
      ...current,
      node: `step:${key}`,
      collapsedStages: (current.collapsedStages || []).filter(
        (stage) => stage !== item.stage,
      ),
    }));
  }
  function sourceOptions(current?: Row) {
    const allowed = new Set<string>();
    function visit(key: string) {
      if (allowed.has(key)) return;
      allowed.add(key);
      const row = document.steps.find((s: Row) => s.key === key);
      for (const dep of row?.dependencies || []) visit(dep);
    }
    if (current) for (const dep of current.dependencies || []) visit(dep);
    else for (const s of document.steps) allowed.add(s.key);
    return document.steps
      .filter((s: Row) => allowed.has(s.key))
      .flatMap((s: Row) =>
        outputNames(s, records).map((port) => ({
          value: `${s.key}:${port}`,
          label: `${s.name} / ${port}`,
        })),
      );
  }
  function inputSelect(
    label: string,
    value: Row | undefined,
    onChange: (v: Row | undefined) => void,
    current?: Row,
  ) {
    return (
      <label key={label}>
        {label}
        <select
          aria-label="{label}"
          value={value ? `${value.step}:${value.port}` : ""}
          disabled={!editable}
          onChange={(e) => {
            if (!e.target.value) onChange(undefined);
            else {
              const [step, port] = e.target.value.split(":");
              onChange({ step, port });
            }
          }}
        >
          <option value="">请选择上游输出</option>
          {(document.input_ports || []).map((port: string) => (
            <option key={port} value={`$input:${port}`}>
              流程输入 / {port}
            </option>
          ))}
          {sourceOptions(current).map((option: Row) => (
            <option key={option.value} value={option.value}>
              {option.label}
            </option>
          ))}
        </select>
      </label>
    );
  }
  const child =
    step?.kind === "workflow.call"
      ? records.find((r) => r.id === step.subworkflow_ref)
      : undefined;
  const requiredInputs =
    step?.kind === "workflow.call"
      ? child?.data.input_ports || []
      : step && ["delivery.review"].includes(step.kind)
        ? ["delivery"]
        : step && ["merge.review", "merge.apply"].includes(step.kind)
          ? ["merge"]
          : [];
  return (
    <>
      {(draft.view === "list" || (!draft.selected && !editable)) && (
        <section className="panel">
          <div className="panel-heading">
            <div>
              <h2>工作流程</h2>
              <p>Step 选择 Role 和 Spec；项目管理为 Role 绑定具体 Agent。</p>
            </div>
            <button
              onClick={() => {
                const params = new URLSearchParams(location.search);
                params.delete("workflow_id");
                history.replaceState(null, "", `?${params}`);
                setDraft({
                  selected: "",
                  view: "editor",
                  name: "",
                  scope: "global",
                  document: emptyDocument(),
                  revision: 1,
                  editing: true,
                  node: "workflow",
                  collapsedStages: [],
                });
              }}
            >
              新建 Workflow
            </button>
          </div>
          <label>
            检索 Workflow
            <input
              type="search"
              value={search}
              onChange={(e) => setSearch(e.target.value)}
            />
          </label>
          <div className="workflow-list">
            {[
              ...workflows,
              ...rows.filter(
                (r) =>
                  r.kind === "workflow_draft" && !r.data.published_revision_id,
              ),
            ]
              .filter((row) =>
                row.name.toLowerCase().includes(search.toLowerCase()),
              )
              .map((row) => (
                <button
                  key={row.id}
                  className={draft.selected === row.id ? "selected" : ""}
                  onClick={() => choose(row)}
                >
                  <strong>{row.name}</strong>
                  <span>
                    {row.kind === "workflow_draft" ? "草稿" : `v${row.version}`}{" "}
                    · {row.scope.kind}
                  </span>
                </button>
              ))}
          </div>
        </section>
      )}
      {draft.view !== "list" && (draft.selected || editable) && (
        <section className="panel" aria-label="Workflow 编辑器">
          <div className="panel-heading">
            <div>
              <h2>{draft.name || "未命名流程"}</h2>
              <p>
                {editable
                  ? "编辑中的草稿 · 表单内容在本机自动保存"
                  : "已保存版本 · 修改时建立新修订"}
              </p>
            </div>
            <div className="actions">
              <button
                onClick={() => {
                  setDraft((current) => ({ ...current, view: "list" }));
                  const params = new URLSearchParams(location.search);
                  params.delete("workflow_id");
                  history.replaceState(null, "", `?${params}`);
                }}
              >
                返回流程列表
              </button>
              {selected && !isTaskVersion ? (
                <button
                  disabled={busy}
                  onClick={() =>
                    void act(async () => {
                      const row = await mutate(
                        "workflow.task_upgrade",
                        selected.id,
                        {},
                        selected.revision,
                      );
                      await load();
                      choose(row);
                      setMessage(
                        row.data.task_upgrade_diagnostics?.join("\n") ||
                          "已建立任务指令版草稿，请检查任务并显式选择升级后的子流程，再发布。",
                      );
                    })
                  }
                >
                  创建任务指令版草稿
                </button>
              ) : null}
              {selected && (
                <button
                  disabled={busy}
                  onClick={() =>
                    void act(async () => {
                      const row = await mutate("workflow.create", undefined, {
                        name: `${draft.name} · 副本`,
                        scope: selected.scope,
                        document,
                      });
                      await load();
                      choose(row);
                    })
                  }
                >
                  复制流程
                </button>
              )}
              {selected && (
                <label>
                  历史修订
                  <select
                    aria-label="历史修订"
                    value={selected.id}
                    onChange={(e) => {
                      const row = rows.find((r) => r.id === e.target.value);
                      if (row) choose(row);
                    }}
                  >
                    {rows
                      .filter((r) => r.definition_id === selected.definition_id)
                      .map((r) => (
                        <option key={r.id} value={r.id}>
                          {r.kind === "workflow_draft" ? "草稿" : "可用版本"} ·
                          v{r.version}
                        </option>
                      ))}
                  </select>
                </label>
              )}
              <button
                onClick={() =>
                  setDraft((current) => ({ ...current, node: "workflow" }))
                }
              >
                流程配置
              </button>
              {!editable && selected && (
                <button
                  disabled={busy}
                  onClick={() =>
                    void act(async () => {
                      const row = await mutate(
                        "workflow.draft.create",
                        selected.id,
                        {
                          base_revision_id:
                            selected.data.published_revision_id || selected.id,
                        },
                        selected.revision,
                      );
                      await load();
                      choose(row);
                      setDraft((current) => ({
                        ...current,
                        node: draft.node,
                        collapsedStages,
                      }));
                    })
                  }
                >
                  编辑流程
                </button>
              )}
              {editable && (
                <>
                  <button
                    disabled={busy || !draft.name.trim()}
                    onClick={() =>
                      void act(async () => {
                        await save();
                      })
                    }
                  >
                    保存草稿
                  </button>
                  <button
                    disabled={busy || !draft.name.trim()}
                    onClick={() =>
                      void act(async () => {
                        const row = await save();
                        await request("query.workflow_requirements", {
                          id: row.id,
                          project_id: project?.id,
                        });
                        const published = await mutate(
                          "workflow.publish",
                          row.id,
                          {},
                          row.revision,
                        );
                        await load();
                        choose(published);
                        await refresh();
                        setMessage("流程可用版本已保存。");
                      })
                    }
                  >
                    保存可用版本
                  </button>
                </>
              )}
              <button disabled={busy} onClick={() => void act(validate)}>
                校验与规范预览
              </button>
              {selected?.kind === "workflow" && (
                <button
                  disabled={busy}
                  onClick={() =>
                    void act(async () => {
                      await mutate(
                        "configuration.retire",
                        selected.id,
                        { reason: "用户删除流程" },
                        selected.revision,
                      );
                      await load();
                      setDraft((current) => ({
                        ...current,
                        selected: "",
                        editing: false,
                      }));
                      setMessage("流程已停用，历史保留。");
                    })
                  }
                >
                  删除流程
                </button>
              )}
            </div>
          </div>
          <div className="workflow-editor workflow-editor-v2">
            <aside className="workflow-preview">
              <h3>阶段与子流程</h3>
              {document.stages.map((item: Row, index: number) => (
                <div className="workflow-stage-group" key={item.key}>
                  <button
                    type="button"
                    aria-label={`${collapsedStages.includes(item.key) ? "展开" : "折叠"}阶段：${item.name}`}
                    aria-expanded={!collapsedStages.includes(item.key)}
                    aria-controls={`${treeId}-${item.key}`}
                    className={
                      draft.node === `stage:${item.key}` ? "selected" : ""
                    }
                    onClick={() => {
                      setMoveStage("");
                      setDraft((current) => ({
                        ...current,
                        node: `stage:${item.key}`,
                        collapsedStages: (
                          current.collapsedStages || []
                        ).includes(item.key)
                          ? (current.collapsedStages || []).filter(
                              (key) => key !== item.key,
                            )
                          : [...(current.collapsedStages || []), item.key],
                      }));
                    }}
                  >
                    <strong>
                      <span aria-hidden="true">
                        {collapsedStages.includes(item.key) ? "▸ " : "▾ "}
                      </span>
                      {index + 1}. {item.name}
                    </strong>
                  </button>
                  <div
                    id={`${treeId}-${item.key}`}
                    hidden={collapsedStages.includes(item.key)}
                  >
                    {document.steps
                      .filter((s: Row) => s.stage === item.key)
                      .map((item: Row) => (
                        <button
                          key={item.key}
                          className={
                            step?.key === item.key
                              ? "selected workflow-node"
                              : "workflow-node"
                          }
                          onClick={() =>
                            setDraft((current) => ({
                              ...current,
                              node: `step:${item.key}`,
                            }))
                          }
                        >
                          {item.kind === "workflow.call" ? "↳ " : ""}
                          {item.name}
                          <small>{actionLabels[item.kind]}</small>
                        </button>
                      ))}
                  </div>
                </div>
              ))}
              {editable && (
                <div className="actions">
                  <button
                    onClick={() => {
                      const key = `stage_${crypto.randomUUID().slice(0, 8)}`;
                      update({
                        stages: [...document.stages, { key, name: "新阶段" }],
                      });
                      setDraft((current) => ({
                        ...current,
                        node: `stage:${key}`,
                      }));
                    }}
                  >
                    添加阶段
                  </button>
                  <button
                    disabled={!document.stages.length}
                    onClick={() => addStep()}
                  >
                    添加步骤
                  </button>
                  <button
                    disabled={!document.stages.length}
                    onClick={() => addStep("workflow.call")}
                  >
                    添加子流程
                  </button>
                </div>
              )}
            </aside>
            <div className="workflow-step-form">
              <fieldset disabled={!editable}>
                {draft.node === "workflow" && (
                  <>
                    <legend>流程配置</legend>
                    <label>
                      Workflow 名称
                      <input
                        required
                        value={draft.name}
                        onChange={(e) =>
                          setDraft({ ...draft, name: e.target.value })
                        }
                      />
                    </label>
                    <label>
                      流程用途
                      <select
                        aria-label="流程用途"
                        value={document.process_kind}
                        onChange={(e) =>
                          update({ process_kind: e.target.value })
                        }
                      >
                        <option value="feature">Feature 流程</option>
                        <option value="task">Task 流程</option>
                        <option value="project_intake">项目讨论</option>
                        <option value="subworkflow">可复用子流程</option>
                      </select>
                    </label>
                    <label>
                      定义范围
                      <select
                        aria-label="定义范围"
                        disabled={!!selected}
                        value={draft.scope}
                        onChange={(e) =>
                          setDraft({ ...draft, scope: e.target.value })
                        }
                      >
                        <option value="global">Global</option>
                        {project && <option value="project">当前项目</option>}
                      </select>
                    </label>
                    {document.steps.some(
                      (s: Row) => s.kind === "tasks.foreach",
                    ) && (
                      <RevisionSelect
                        label="Task 子流程版本"
                        records={workflows.filter(
                          (r) => r.data.process_kind === "task",
                        )}
                        value={document.subworkflow_refs?.task || ""}
                        required
                        onChange={(value) =>
                          update({
                            subworkflow_refs: {
                              ...document.subworkflow_refs,
                              task: value,
                            },
                          })
                        }
                      />
                    )}
                    <details>
                      <summary>子流程输入输出接口</summary>
                      <label>
                        输入端口名称（逗号或换行分隔）
                        <textarea
                          value={(document.input_ports || []).join("\n")}
                          onChange={(e) =>
                            update({ input_ports: split(e.target.value) })
                          }
                        />
                      </label>
                      {Object.entries(document.output_ports || {}).map(
                        ([name, value]) => (
                          <div key={name}>
                            <label>
                              输出端口名称
                              <input
                                aria-label={`输出端口名称 ${name}`}
                                defaultValue={name}
                                onBlur={(e) => {
                                  const key = e.target.value.trim();
                                  if (
                                    key &&
                                    key !== name &&
                                    !document.output_ports[key]
                                  ) {
                                    const ports = { ...document.output_ports };
                                    delete ports[name];
                                    ports[key] = value;
                                    update({ output_ports: ports });
                                  }
                                }}
                              />
                            </label>
                            {inputSelect(`输出 ${name}`, value as Row, (v) => {
                              const ports = { ...document.output_ports };
                              if (v) ports[name] = v;
                              else delete ports[name];
                              update({ output_ports: ports });
                            })}
                            <button
                              type="button"
                              onClick={() => {
                                const ports = { ...document.output_ports };
                                delete ports[name];
                                update({ output_ports: ports });
                              }}
                            >
                              删除输出 {name}
                            </button>
                          </div>
                        ),
                      )}
                      <button
                        type="button"
                        onClick={() =>
                          update({
                            output_ports: {
                              ...document.output_ports,
                              [`output_${Object.keys(document.output_ports || {}).length + 1}`]:
                                { step: "", port: "" },
                            },
                          })
                        }
                      >
                        添加输出端口
                      </button>
                    </details>
                  </>
                )}
                {stage && (
                  <>
                    <legend>阶段配置</legend>
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
                    <MarkdownField
                      key={`${draft.selected}:${stage.key}:description`}
                      label="阶段意图与交付物（Markdown）"
                      rows={8}
                      readOnly={!editable}
                      value={stage.description || ""}
                      onChange={(description) =>
                        update({
                          stages: document.stages.map((s: Row) =>
                            s.key === stage.key ? { ...s, description } : s,
                          ),
                        })
                      }
                    />
                    <div className="actions">
                      {[-1, 1].map((direction) => (
                        <button
                          key={direction}
                          type="button"
                          onClick={() => {
                            const stages = [...document.stages];
                            const i = stages.findIndex(
                              (s) => s.key === stage.key,
                            );
                            const target = i + direction;
                            if (target >= 0 && target < stages.length) {
                              [stages[i], stages[target]] = [
                                stages[target],
                                stages[i],
                              ];
                              update({ stages });
                            }
                          }}
                        >
                          {direction < 0 ? "阶段上移" : "阶段下移"}
                        </button>
                      ))}
                    </div>
                    {document.steps.some((s: Row) => s.stage === stage.key) && (
                      <label>
                        删除阶段前将节点迁移到
                        <select
                          aria-label="删除阶段前将节点迁移到"
                          value={moveStage}
                          onChange={(e) => setMoveStage(e.target.value)}
                        >
                          <option value="">请选择目标阶段</option>
                          {document.stages
                            .filter((s: Row) => s.key !== stage.key)
                            .map((s: Row) => (
                              <option key={s.key} value={s.key}>
                                {s.name}
                              </option>
                            ))}
                        </select>
                      </label>
                    )}
                    <button
                      type="button"
                      disabled={
                        document.steps.some(
                          (s: Row) => s.stage === stage.key,
                        ) && !moveStage
                      }
                      onClick={() => {
                        update({
                          stages: document.stages.filter(
                            (s: Row) => s.key !== stage.key,
                          ),
                          steps: document.steps.map((s: Row) =>
                            s.stage === stage.key
                              ? { ...s, stage: moveStage }
                              : s,
                          ),
                        });
                        setDraft((current) => ({
                          ...current,
                          node: "workflow",
                        }));
                      }}
                    >
                      删除阶段
                    </button>
                  </>
                )}
                {step && (
                  <>
                    <legend>步骤配置</legend>
                    <label>
                      步骤名称
                      <input
                        value={step.name}
                        onChange={(e) => updateStep({ name: e.target.value })}
                      />
                    </label>
                    <label>
                      所属阶段
                      <select
                        aria-label="所属阶段"
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
                      标准动作
                      <select
                        aria-label="标准动作"
                        value={step.kind}
                        onChange={(e) =>
                          updateStep({
                            kind: e.target.value,
                            contract_version:
                              isTaskVersion && requiresTask(e.target.value)
                                ? 2
                                : 1,
                            task_instructions:
                              isTaskVersion && requiresTask(e.target.value)
                                ? ""
                                : undefined,
                            participant_slot: null,
                            spec_refs: requiresRole(e.target.value)
                              ? step.spec_refs || []
                              : [],
                            execution_recipe:
                              e.target.value === "attempt.execute"
                                ? step.execution_recipe || []
                                : [],
                            role_ref: requiresRole(e.target.value)
                              ? step.role_ref
                              : null,
                            subworkflow_ref:
                              e.target.value === "workflow.call"
                                ? step.subworkflow_ref || ""
                                : null,
                          })
                        }
                      >
                        {Object.entries(actionNames).map(([id, label]) => (
                          <option key={id} value={id}>
                            {label}
                          </option>
                        ))}
                      </select>
                    </label>
                    {isTaskVersion && requiresTask(step.kind) ? (
                      <>
                        <MarkdownField
                          key={`${draft.selected}:${step.key}:task`}
                          label={
                            step.kind === "attempt.execute"
                              ? "公共任务指令（Markdown）"
                              : "任务指令（Markdown）"
                          }
                          required
                          rows={10}
                          readOnly={!editable}
                          value={step.task_instructions || ""}
                          onChange={(task_instructions) =>
                            updateStep({ task_instructions })
                          }
                        />
                        <p className="muted">
                          此任务与 Role、所选 Skill、步骤 Spec 一起传给
                          Agent。发布产生新修订，已有工程保持原冻结任务。
                        </p>
                      </>
                    ) : null}
                    {requiresRole(step.kind) && (
                      <>
                        <RevisionSelect
                          label="执行 Role"
                          records={roles.map((r) => ({
                            ...r,
                            id: r.definition_id,
                          }))}
                          required
                          value={step.role_ref || ""}
                          onChange={(value) =>
                            updateStep({
                              role_ref: value,
                              participant_slot: null,
                            })
                          }
                        />
                        <RevisionSelect
                          label="步骤 Spec"
                          records={specs.filter(
                            (r) =>
                              !r.data.applicable_actions?.length ||
                              r.data.applicable_actions.some((a: Row) => {
                                const context = specificationContext(
                                  step,
                                  document.steps,
                                );
                                return (
                                  a.kind === step.kind &&
                                  (!a.phase ||
                                    !context.phase ||
                                    a.phase === context.phase) &&
                                  (!a.delivery_kind ||
                                    !context.delivery ||
                                    a.delivery_kind === context.delivery)
                                );
                              }),
                          )}
                          multiple
                          required
                          value={step.spec_refs || []}
                          onChange={(value) => updateStep({ spec_refs: value })}
                        />
                      </>
                    )}
                    <label>
                      前置步骤
                      <select
                        aria-label="前置步骤"
                        multiple
                        value={step.dependencies || []}
                        onChange={(e) =>
                          updateStep({
                            dependencies: Array.from(
                              e.target.selectedOptions,
                              (o) => o.value,
                            ),
                          })
                        }
                      >
                        {document.steps
                          .filter((s: Row) => s.key !== step.key)
                          .map((s: Row) => (
                            <option value={s.key} key={s.key}>
                              {s.name}
                            </option>
                          ))}
                      </select>
                    </label>
                    {step.kind === "workflow.call" && (
                      <>
                        <RevisionSelect
                          label="调用子流程"
                          records={workflows.filter(
                            (r) => r.id !== draft.selected,
                          )}
                          required
                          value={step.subworkflow_ref || ""}
                          onChange={(value) =>
                            updateStep({ subworkflow_ref: value, inputs: {} })
                          }
                        />
                        {child && (
                          <p>
                            <a
                              href={`?tab=workflows&workflow_id=${encodeURIComponent(child.id)}${project ? `&project_id=${encodeURIComponent(project.id)}` : ""}`}
                            >
                              编辑子流程：{child.name} v{child.version}
                            </a>
                          </p>
                        )}
                      </>
                    )}
                    {[
                      ...new Set([
                        ...requiredInputs,
                        ...Object.keys(step.inputs || {}),
                      ]),
                    ].map((port) =>
                      inputSelect(
                        `输入 ${port}`,
                        step.inputs?.[port],
                        (value) => {
                          const inputs = { ...step.inputs };
                          if (value) inputs[port] = value;
                          else delete inputs[port];
                          updateStep({ inputs });
                        },
                        step,
                      ),
                    )}
                    {step.kind === "agent.interact" && (
                      <>
                        {!isTaskVersion ? (
                          <MarkdownField
                            key={`${draft.selected}:${step.key}:goal`}
                            label="讨论目标"
                            required
                            readOnly={!editable}
                            value={step.interaction?.goal || ""}
                            onChange={(goal) =>
                              updateStep({
                                interaction: {
                                  ...step.interaction,
                                  goal,
                                },
                              })
                            }
                          />
                        ) : null}
                        <MarkdownTopicsField
                          key={`${draft.selected}:${step.key}:topics`}
                          value={step.interaction?.required_topics || []}
                          readOnly={!editable}
                          onChange={(required_topics) =>
                            updateStep({
                              interaction: {
                                ...step.interaction,
                                required_topics,
                              },
                            })
                          }
                        />
                        <label>
                          输出类型
                          <select
                            aria-label="输出类型"
                            value={
                              step.interaction?.output_kind || "requirement"
                            }
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
                              "feature",
                              "design",
                              "task_plan",
                              "test_plan",
                            ].map((id) => (
                              <option key={id}>{id}</option>
                            ))}
                          </select>
                        </label>
                        <label>
                          最大轮数
                          <input
                            type="number"
                            min={1}
                            max={100}
                            value={step.interaction?.max_turns || 8}
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
                    {step.kind === "attempt.execute" && (
                      <>
                        <label>
                          执行阶段
                          <select
                            aria-label="执行阶段"
                            value={step.parameters?.phase || ""}
                            onChange={(e) =>
                              updateStep({
                                parameters: {
                                  ...step.parameters,
                                  phase: e.target.value,
                                },
                                ...(isTaskVersion
                                  ? {
                                      task_instructions: "",
                                      execution_recipe: [],
                                    }
                                  : {}),
                              })
                            }
                          >
                            <option value="">请选择</option>
                            {Object.entries(phases).map(([id, label]) => (
                              <option key={id} value={id}>
                                {label}
                              </option>
                            ))}
                          </select>
                        </label>
                        <h3>执行节点与 Role</h3>
                        {(step.execution_recipe || []).map(
                          (node: Row, index: number) => (
                            <fieldset key={node.key}>
                              <legend>
                                {nodeLabels[node.key] || node.key}
                              </legend>
                              {isTaskVersion ? (
                                agentNode(node.key) ? (
                                  <MarkdownField
                                    key={`${draft.selected}:${step.key}:${node.key}:task`}
                                    label={`${nodeLabels[node.key] || node.key} 节点任务指令（Markdown）`}
                                    required
                                    rows={8}
                                    readOnly={!editable}
                                    value={node.task_instructions || ""}
                                    onChange={(task_instructions) => {
                                      const recipe = structuredClone(
                                        step.execution_recipe,
                                      );
                                      recipe[index].task_instructions =
                                        task_instructions;
                                      updateStep({ execution_recipe: recipe });
                                    }}
                                  />
                                ) : (
                                  <p className="muted">
                                    系统测试执行器按冻结 TestRecipe 执行，不调用
                                    Agent。
                                  </p>
                                )
                              ) : null}
                              <RevisionSelect
                                label={`${nodeLabels[node.key] || node.key} Role`}
                                records={roles
                                  .filter(
                                    (r) => r.data.execution_role === node.role,
                                  )
                                  .map((r) => ({ ...r, id: r.definition_id }))}
                                required
                                value={node.role_ref || ""}
                                onChange={(value) => {
                                  const recipe = structuredClone(
                                    step.execution_recipe,
                                  );
                                  recipe[index].role_ref = value;
                                  updateStep({ execution_recipe: recipe });
                                }}
                              />
                              <label>
                                节点前置依赖
                                <select
                                  aria-label="节点前置依赖"
                                  multiple
                                  value={node.dependencies || []}
                                  onChange={(e) => {
                                    const recipe = structuredClone(
                                      step.execution_recipe,
                                    );
                                    recipe[index].dependencies = Array.from(
                                      e.target.selectedOptions,
                                      (o) => o.value,
                                    );
                                    updateStep({ execution_recipe: recipe });
                                  }}
                                >
                                  {step.execution_recipe
                                    .filter((n: Row) => n.key !== node.key)
                                    .map((n: Row) => (
                                      <option key={n.key} value={n.key}>
                                        {nodeLabels[n.key] || n.key}
                                      </option>
                                    ))}
                                </select>
                              </label>
                              <span className="muted">
                                {dutyLabels[node.role]} ·{" "}
                                {node.writes ? "受限写入" : "只读"}
                              </span>
                            </fieldset>
                          ),
                        )}
                        <button
                          type="button"
                          onClick={() => {
                            const phase = step.parameters?.phase;
                            const nodes: Row[] =
                              phase === "implementation"
                                ? [
                                    {
                                      key: "test_author",
                                      role: "tester",
                                      dependencies: [],
                                      writes: true,
                                    },
                                    {
                                      key: "red",
                                      role: "tester",
                                      dependencies: ["test_author"],
                                      writes: false,
                                    },
                                    {
                                      key: "implement",
                                      role: "developer",
                                      dependencies: ["red"],
                                      writes: true,
                                    },
                                    {
                                      key: "green",
                                      role: "tester",
                                      dependencies: ["implement"],
                                      writes: false,
                                    },
                                    {
                                      key: "docs",
                                      role: "developer",
                                      dependencies: ["green"],
                                      writes: false,
                                    },
                                    {
                                      key: "verify",
                                      role: "tester",
                                      dependencies: ["green", "docs"],
                                      writes: false,
                                    },
                                  ]
                                : phase === "solution_planning"
                                  ? [
                                      {
                                        key: "plan_validate",
                                        role: "architect",
                                        dependencies: [],
                                        writes: false,
                                      },
                                      {
                                        key: "test_plan_validate",
                                        role: "tester",
                                        dependencies: ["plan_validate"],
                                        writes: false,
                                      },
                                    ]
                                  : phase === "closure"
                                    ? [
                                        {
                                          key: "test_author",
                                          role: "tester",
                                          dependencies: [],
                                          writes: true,
                                        },
                                        {
                                          key: "feature_tests",
                                          role: "tester",
                                          dependencies: ["test_author"],
                                          writes: false,
                                        },
                                        {
                                          key: "docs",
                                          role: "developer",
                                          dependencies: ["feature_tests"],
                                          writes: false,
                                        },
                                        {
                                          key: "verify",
                                          role: "tester",
                                          dependencies: [
                                            "feature_tests",
                                            "docs",
                                          ],
                                          writes: false,
                                        },
                                      ]
                                    : [
                                        {
                                          key: "requirement_validate",
                                          role: "analyst",
                                          dependencies: [],
                                          writes: false,
                                        },
                                      ];
                            updateStep({
                              execution_recipe: nodes.map((node) => ({
                                ...(isTaskVersion && agentNode(node.key)
                                  ? { task_instructions: "" }
                                  : {}),
                                ...node,
                                role_ref: null,
                              })),
                            });
                          }}
                        >
                          设置该阶段执行节点
                        </button>
                      </>
                    )}
                    <label>
                      推进方式
                      <select
                        aria-label="推进方式"
                        value={step.advance_mode || "automatic"}
                        onChange={(e) =>
                          updateStep({ advance_mode: e.target.value })
                        }
                      >
                        <option value="automatic">自动</option>
                        <option value="human_confirmation">人工确认</option>
                      </select>
                    </label>
                    <ConditionEditor
                      condition={step.condition || { op: "always" }}
                      steps={document.steps}
                      onChange={(condition) => updateStep({ condition })}
                    />
                    <label>
                      重试上限
                      <input
                        type="number"
                        min={0}
                        max={10}
                        value={step.retry_limit ?? 3}
                        onChange={(e) =>
                          updateStep({ retry_limit: Number(e.target.value) })
                        }
                      />
                    </label>
                    <label>
                      返工范围
                      <select
                        aria-label="返工范围"
                        multiple
                        value={step.rework_scope || []}
                        onChange={(e) =>
                          updateStep({
                            rework_scope: Array.from(
                              e.target.selectedOptions,
                              (o) => o.value,
                            ),
                          })
                        }
                      >
                        {document.steps.map((s: Row) => (
                          <option value={s.key} key={s.key}>
                            {s.name}
                          </option>
                        ))}
                      </select>
                    </label>
                    <div className="actions">
                      {[-1, 1].map((direction) => (
                        <button
                          type="button"
                          key={direction}
                          onClick={() => {
                            const steps = [...document.steps];
                            const index = steps.findIndex(
                              (s) => s.key === step.key,
                            );
                            const target = index + direction;
                            if (target >= 0 && target < steps.length) {
                              [steps[index], steps[target]] = [
                                steps[target],
                                steps[index],
                              ];
                              update({ steps });
                            }
                          }}
                        >
                          {direction < 0 ? "步骤上移" : "步骤下移"}
                        </button>
                      ))}
                      <button
                        type="button"
                        disabled={document.steps.some(
                          (s: Row) =>
                            s.dependencies?.includes(step.key) ||
                            Object.values(s.inputs || {}).some(
                              (input: any) => input.step === step.key,
                            ),
                        )}
                        onClick={() => {
                          update({
                            steps: document.steps.filter(
                              (s: Row) => s.key !== step.key,
                            ),
                          });
                          setDraft((current) => ({
                            ...current,
                            node: "workflow",
                          }));
                        }}
                      >
                        删除步骤
                      </button>
                    </div>
                    <p className="muted">
                      仍被其他步骤依赖时，先调整前置和输入引用再删除。
                    </p>
                  </>
                )}
              </fieldset>
              {step && isTaskVersion && requiresTask(step.kind) ? (
                <WorkflowTaskPreview
                  key={`${draft.selected}:${step.key}`}
                  request={request}
                  projectId={project?.id}
                  document={document}
                  step={step}
                  agents={records.filter((r) => r.kind === "agent")}
                  roles={records.filter((r) => r.kind === "role")}
                  nodeKeys={(step.execution_recipe || [])
                    .filter((n: Row) => agentNode(n.key))
                    .map((n: Row) => n.key)}
                />
              ) : null}
            </div>
          </div>
          {validation && (
            <details open>
              <summary>所需角色与规范预览</summary>
              <p>
                {validation.roles?.map((r: Row) => r.role.name).join("、") ||
                  "系统流程，无需 Agent"}
              </p>
              {validation.specification_snapshot?.map((binding: Row) => (
                <div className="document-row" key={binding.path}>
                  <strong>{binding.path}</strong>
                  <span>
                    {binding.spec_refs
                      .map(
                        (id: string) =>
                          specs.find((r) => r.id === id)?.name || "固定 Spec",
                      )
                      .join("、") || "系统动作"}
                  </span>
                </div>
              ))}
            </details>
          )}
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
    </>
  );
}
function ConditionEditor({
  condition,
  steps,
  onChange,
}: {
  condition: Row;
  steps: Row[];
  onChange: (value: Row) => void;
}) {
  return (
    <fieldset>
      <legend>推进条件</legend>
      <label>
        条件操作
        <select
          aria-label="条件操作"
          value={condition.op}
          onChange={(e) =>
            onChange(
              e.target.value === "all" || e.target.value === "any"
                ? { op: e.target.value, conditions: [{ op: "always" }] }
                : e.target.value === "output_present"
                  ? { op: e.target.value, step: "" }
                  : e.target.value === "accepted"
                    ? { op: e.target.value, kind: "requirement" }
                    : { op: "always" },
            )
          }
        >
          <option value="always">总是</option>
          <option value="output_present">前置步骤已有输出</option>
          <option value="accepted">交付已接受</option>
          <option value="all">全部条件</option>
          <option value="any">任一条件</option>
        </select>
      </label>
      {condition.op === "output_present" && (
        <label>
          引用步骤
          <select
            aria-label="引用步骤"
            value={condition.step}
            onChange={(e) => onChange({ ...condition, step: e.target.value })}
          >
            <option value="">请选择</option>
            {steps.map((s) => (
              <option key={s.key} value={s.key}>
                {s.name}
              </option>
            ))}
          </select>
        </label>
      )}
      {condition.op === "accepted" && (
        <label>
          接受的交付类型
          <select
            aria-label="接受的交付类型"
            value={condition.kind}
            onChange={(e) => onChange({ ...condition, kind: e.target.value })}
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
      )}
      {["all", "any"].includes(condition.op) && (
        <>
          {condition.conditions.map((c: Row, i: number) => (
            <div key={i}>
              <ConditionEditor
                condition={c}
                steps={steps}
                onChange={(value) =>
                  onChange({
                    ...condition,
                    conditions: condition.conditions.map(
                      (c: Row, index: number) => (index === i ? value : c),
                    ),
                  })
                }
              />
              <button
                type="button"
                onClick={() =>
                  onChange({
                    ...condition,
                    conditions: condition.conditions.filter(
                      (_: Row, index: number) => i !== index,
                    ),
                  })
                }
              >
                删除子条件
              </button>
            </div>
          ))}
          <button
            type="button"
            onClick={() =>
              onChange({
                ...condition,
                conditions: [...condition.conditions, { op: "always" }],
              })
            }
          >
            添加子条件
          </button>
        </>
      )}
    </fieldset>
  );
}

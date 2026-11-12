import { useCallback, useEffect, useRef, useState } from "react";
import { ChevronRight } from "lucide-react";
import {
  ActionButton,
  ActionSelect,
  useActionFeedback,
} from "./ActionFeedback";
import {
  latest,
  loadCatalog,
  MarkdownField,
  RevisionSelect,
  useDraft,
  type Request,
  type Row,
} from "./configuration-ui";
import {
  JsonField,
  SourceSelect,
  WorkflowDeliveries,
} from "./WorkflowDeliveries";
import { WorkflowTaskPreview } from "./WorkflowTaskPreview";

type Props = {
  request: Request;
  project?: Row;
  refresh: () => Promise<unknown>;
};
const emptyDocument = () => ({
  schema_version: 6,
  stages: [
    { key: "work", name: "工作", description: "", default_role_ref: null },
  ],
  steps: [] as Row[],
  inputs: {},
  outputs: {},
  completion: { op: "always" },
});
const executorNames: Record<string, string> = {
  agent: "Agent 工作",
  tool: "调用工具",
  workflow: "调用子流程",
  control: "控制与确认",
};
const splitPaths = (value: string) =>
  value
    .split(/[\n,，]/)
    .map((value) => value.trim())
    .filter(Boolean);
function replacePort(
  value: any,
  step: string,
  previous: string,
  next: string,
): any {
  if (Array.isArray(value))
    return value.map((value) => replacePort(value, step, previous, next));
  if (!value || typeof value !== "object") return value;
  if (value.type === "constant") return value;
  if (value.step === step && value.port === previous)
    return { ...value, port: next };
  return Object.fromEntries(
    Object.entries(value).map(([key, item]) => [
      key,
      value.op === "equals" && key === "value"
        ? item
        : replacePort(item, step, previous, next),
    ]),
  );
}

export function WorkflowManager({ request, project, refresh }: Props) {
  const {
    error,
    setError,
    message,
    setMessage,
    capture,
    beginFeedback,
    endFeedback,
  } = useActionFeedback();
  const [rows, setRows] = useState<Row[]>([]),
    [records, setRecords] = useState<Row[]>([]),
    [busy, setBusy] = useState(false),
    [validation, setValidation] = useState<Row>();
  const [authoring, setAuthoring] = useState<Row>();
  const [importText, setImportText] = useState("");
  const [draft, setDraft] = useDraft(
    `devdesk:v6:workflow:${project?.id || "global"}`,
    () => ({
      selected: "",
      revision: 1,
      name: "",
      scope: "global",
      view: "list",
      editing: true,
      node: "workflow",
      document: emptyDocument() as Row,
      expandedStages: [] as string[],
    }),
  );
  const editorRef = useRef<HTMLElement>(null);
  const epoch = useRef(0),
    routeSelected = useRef(false);
  const load = useCallback(async () => {
    const current = ++epoch.current;
    const [catalog, workflows, defaults] = await Promise.all([
      loadCatalog(request, project),
      (async () => {
        const records: Row[] = [];
        for (let offset = 0; ;) {
          const page = await request("query.workflows", {
            local: !!project,
            project_id: project?.id,
            offset,
            limit: 100,
            include_history: true,
          });
          records.push(...page.records);
          offset += page.records.length;
          if (offset >= page.total || !page.records.length) return { records };
        }
      })(),
      request("query.configuration_defaults", { project_id: project?.id }),
    ]);
    if (epoch.current !== current) return;
    setAuthoring(defaults.authoring);
    setRecords(catalog);
    setRows(workflows.records || []);
  }, [request, project?.id]);
  useEffect(() => {
    void load().catch((error) => setError(String(error)));
    return () => {
      epoch.current++;
    };
  }, [load]);
  const choose = useCallback(
    (row: Row) => {
      const document =
        row.kind === "workflow_draft" ? row.data.document : row.data;
      if (document.schema_version !== 6) {
        setError("此流程使用过时的开发合同，请重建配置。");
        return;
      }
      setDraft({
        ...draft,
        selected: row.id,
        revision: row.revision,
        name: row.name,
        scope: row.scope?.kind || "global",
        view: "editor",
        editing: row.kind === "workflow_draft",
        node: "workflow",
        document: structuredClone(document),
        expandedStages: [],
      });
      setValidation(undefined);
      const params = new URLSearchParams(location.search);
      params.set("workflow_id", row.id);
      history.replaceState(null, "", `?${params}`);
    },
    [draft, setDraft, setError],
  );
  useEffect(() => {
    if (!rows.length || routeSelected.current) return;
    routeSelected.current = true;
    const selected = new URLSearchParams(location.search).get("workflow_id");
    const row = rows.find((row) => row.id === selected);
    if (row && (draft.selected !== row.id || draft.view !== "editor"))
      choose(row);
  }, [rows, choose, draft.selected, draft.view]);
  const document = draft.document;
  const roles = latest(records.filter((row) => row.kind === "role")).map(
    (row) => ({ ...row, id: row.id }),
  );
  const stage = draft.node.startsWith("stage:")
    ? document.stages.find((stage: Row) => stage.key === draft.node.slice(6))
    : undefined;
  const step = draft.node.startsWith("step:")
    ? document.steps.find((step: Row) => step.key === draft.node.slice(5))
    : undefined;
  const workflows = rows.filter(
    (row) => row.kind === "workflow" && row.data.schema_version === 6,
  );
  const update = (patch: Row) => {
    setValidation(undefined);
    setDraft((current) => ({
      ...current,
      document: { ...current.document, ...patch },
    }));
  };
  const updateStep = (patch: Row) =>
    update({
      steps: document.steps.map((item: Row) =>
        item.key === step?.key ? { ...item, ...patch } : item,
      ),
    });
  async function mutate(
    method: string,
    target: string | undefined,
    payload: Row,
    revision?: number,
  ) {
    return request(method, {
      project_id: project?.id,
      command_id: crypto.randomUUID(),
      target,
      expected_revision: revision,
      payload,
    });
  }
  async function run(work: () => Promise<void>) {
    capture();
    beginFeedback();
    setBusy(true);
    setError("");
    try {
      await work();
    } catch (error) {
      setError(String(error));
    } finally {
      setBusy(false);
      endFeedback();
    }
  }
  async function save() {
    for (const field of editorRef.current?.querySelectorAll<
      HTMLInputElement | HTMLTextAreaElement | HTMLSelectElement
    >("input,textarea,select") || []) {
      if (!field.reportValidity())
        throw new Error("请先修正表单中的无效内容。");
    }

    const row = draft.selected && rows.find((row) => row.id === draft.selected);
    const saved = row
      ? await mutate(
          "workflow.draft.update",
          row.id,
          { name: draft.name, document },
          draft.revision,
        )
      : await mutate("workflow.create", undefined, {
          name: draft.name,
          scope:
            draft.scope === "project"
              ? { kind: "project", project_id: project!.id }
              : { kind: "global" },
          document,
        });
    setDraft((current) => ({
      ...current,
      selected: saved.id,
      revision: saved.revision,
      editing: true,
    }));
    const params = new URLSearchParams(location.search);
    params.set("workflow_id", saved.id);
    history.replaceState(null, "", `?${params}`);
    await load();
    setMessage("流程草稿已保存。");
    return saved;
  }
  function addStep(type = "agent") {
    const key = `step_${crypto.randomUUID().slice(0, 8)}`;
    const executor =
      type === "agent"
        ? { type, role_ref: null }
        : type === "workflow"
          ? { type, workflow_ref: "", foreach: null }
          : type === "tool"
            ? { type, tool_ref: "", arguments: {} }
            : { type };
    const item = {
      key,
      name: type === "workflow" ? "新子流程调用" : "新步骤",
      stage: stage?.key || step?.stage || document.stages[0]?.key,
      executor,
      task_instructions: type === "agent" ? "" : null,
      required_skill_refs: [],
      spec_refs: [],
      tags: [],
      dependencies: step ? [step.key] : [],
      inputs: {},
      outputs: {},
      access: {
        workspace_input: null,
      },
      budget: structuredClone(
        authoring?.execution_budget || {
          max_turns: 8,
          timeout_seconds: 300,
          retry_limit: 3,
        },
      ),
      condition: { op: "always" },
      completion: { op: "always" },
      confirmation: "automatic",
    };
    update({ steps: [...document.steps, item] });
    setDraft((current) => ({
      ...current,
      node: `step:${key}`,
      expandedStages: current.expandedStages.includes(item.stage)
        ? current.expandedStages
        : [...current.expandedStages, item.stage],
    }));
  }
  function renameOutput(previous: string, next: string) {
    if (!step || previous === next) return;
    if (!/^[A-Za-z0-9_-]{1,80}$/.test(next) || next in step.outputs) {
      setError("输出名称需唯一，使用字母、数字、下划线或连字符。");
      return;
    }
    const steps = document.steps.map((item: Row) =>
      item.key === step.key
        ? {
            ...item,
            outputs: Object.fromEntries(
              Object.entries(item.outputs).map(([name, value]) => [
                name === previous ? next : name,
                value,
              ]),
            ),
          }
        : item,
    );
    update(replacePort({ ...document, steps }, step.key, previous, next));
  }
  const visibleRows = rows.filter(
    (row) =>
      (row.kind === "workflow_draft"
        ? row.data.document?.schema_version
        : row.data.schema_version) === 6,
  );
  return (
    <section
      ref={editorRef}
      className="panel workflow-manager"
      aria-label="工作流程管理"
    >
      <div className="panel-heading">
        <div>
          <h2>工作流程</h2>
          <p>通过步骤任务、技能、材料与完成要求定义工作。</p>
        </div>
        <ActionButton
          type="button"
          onClick={() => {
            capture();
            setDraft({
              selected: "",
              revision: 1,
              name: "",
              scope: project ? "project" : "global",
              view: "editor",
              editing: true,
              node: "workflow",
              document: emptyDocument(),
              expandedStages: [],
            });
            setValidation(undefined);
          }}
        >
          新建流程
        </ActionButton>
      </div>
      {draft.view === "list" ? (
        <div className="configuration-list">
          {latest(visibleRows).map((row) => (
            <ActionButton
              key={row.id}
              onClick={() => {
                capture();
                choose(row);
              }}
            >
              <strong>{row.name}</strong>
              <small>
                {row.kind === "workflow_draft" ? "草稿" : `v${row.version}`}
              </small>
            </ActionButton>
          ))}
          {!visibleRows.length ? <p>暂无流程，可创建新的工作流程。</p> : null}
        </div>
      ) : (
        <>
          <div className="actions">
            <ActionButton
              disabled={busy}
              onClick={() => {
                setDraft((current) => ({
                  ...current,
                  selected: "",
                  revision: 1,
                  name: `${current.name} 副本`,
                  editing: true,
                  view: "editor",
                  node: "workflow",
                }));
              }}
            >
              复制为新流程
            </ActionButton>
            <ActionButton
              onClick={() => {
                const url = URL.createObjectURL(
                  new Blob(
                    [JSON.stringify({ name: draft.name, document }, null, 2)],
                    { type: "application/json" },
                  ),
                );
                const link = window.document.createElement("a");
                link.href = url;
                link.download = `workflow-${draft.selected || "draft"}.json`;
                link.click();
                URL.revokeObjectURL(url);
              }}
            >
              导出流程
            </ActionButton>
            <details>
              <summary>导入流程</summary>
              <label>
                Workflow JSON
                <textarea
                  value={importText}
                  onChange={(event) => setImportText(event.target.value)}
                />
              </label>
              <ActionButton
                disabled={busy}
                onClick={() => {
                  try {
                    const value = JSON.parse(importText);
                    const imported = value.document || value;
                    if (!Array.isArray(imported.steps) || !Array.isArray(imported.stages))
                      throw new Error("请选择完整流程定义（需要 stages 与 steps）。");
                    run(async () => {
                      // Structural validation stays backend-side; the frontend only parses JSON.
                      await request("query.workflow_requirements", {
                        document: imported,
                        project_id: project?.id,
                      });
                      setDraft((current) => ({
                        ...current,
                        selected: "",
                        revision: 1,
                        name: value.name || "导入流程",
                        editing: true,
                        view: "editor",
                        node: "workflow",
                        document: imported,
                        expandedStages: [],
                      }));
                      setImportText("");
                    });
                  } catch (error) {
                    setError(String(error));
                  }
                }}
              >
                载入为新草稿
              </ActionButton>
            </details>

            <ActionButton
              type="button"
              onClick={() => {
                capture();
                setDraft((current) => ({ ...current, view: "list" }));
              }}
            >
              返回流程列表
            </ActionButton>
            {!draft.editing && draft.selected ? (
              <ActionButton
                disabled={busy}
                onClick={() =>
                  run(async () => {
                    const row = await mutate(
                      "workflow.draft.create",
                      undefined,
                      { base_revision_id: draft.selected },
                    );
                    await load();
                    choose(row);
                  })
                }
              >
                编辑流程
              </ActionButton>
            ) : null}
            {draft.editing ? (
              <>
                <ActionButton
                  disabled={busy || !draft.name.trim()}
                  onClick={() =>
                    run(async () => {
                      await save();
                    })
                  }
                >
                  保存草稿
                </ActionButton>
                <ActionButton
                  disabled={busy || !draft.name.trim()}
                  onClick={() =>
                    run(async () => {
                      const row = await save();
                      const published = await mutate(
                        "workflow.publish",
                        row.id,
                        {},
                        row.revision,
                      );
                      await load();
                      choose(published);
                      await refresh();
                      setMessage("流程已发布。");
                    })
                  }
                >
                  发布流程
                </ActionButton>
              </>
            ) : null}
            <ActionButton
              disabled={busy}
              onClick={() =>
                run(async () => {
                  const value = await request("query.workflow_requirements", {
                    id: draft.selected || undefined,
                    document,
                    project_id: project?.id,
                  });
                  setValidation(value);
                  setMessage("流程结构、材料和配置引用校验通过。");
                })
              }
            >
              校验流程
            </ActionButton>
          </div>
          <div className="workflow-editor workflow-editor-v2">
            <aside className="workflow-stages">
              <ActionButton
                type="button"
                onClick={() => {
                  capture();
                  setDraft((current) => ({ ...current, node: "workflow" }));
                }}
              >
                {draft.name || "流程配置"}
              </ActionButton>
              {document.stages.map((item: Row) => {
                const expanded = draft.expandedStages.includes(item.key);
                const steps = document.steps.filter(
                  (step: Row) => step.stage === item.key,
                );
                return (
                  <div className="workflow-stage-node" key={item.key}>
                    <div className="workflow-stage-head">
                      <ActionButton
                        type="button"
                        className="tree-toggle"
                        aria-label={`${expanded ? "折叠" : "展开"}阶段：${item.name}`}
                        aria-expanded={expanded}
                        onClick={() =>
                          setDraft((current) => ({
                            ...current,
                            expandedStages: expanded
                              ? current.expandedStages.filter(
                                  (key: string) => key !== item.key,
                                )
                              : [...current.expandedStages, item.key],
                          }))
                        }
                      >
                        <ChevronRight aria-hidden="true" />
                      </ActionButton>
                      <ActionButton
                        type="button"
                        className="workflow-stage-name"
                        onClick={() => {
                          capture();
                          setDraft((current) => ({
                            ...current,
                            node: `stage:${item.key}`,
                            expandedStages: current.expandedStages.includes(
                              item.key,
                            )
                              ? current.expandedStages
                              : [...current.expandedStages, item.key],
                          }));
                        }}
                      >
                        {item.name}
                      </ActionButton>
                      <small>{steps.length} 个步骤</small>
                    </div>
                    {expanded ? (
                      <div className="workflow-stage-children">
                        {steps.map((step: Row) => (
                          <ActionButton
                            key={step.key}
                            className={
                              draft.node === `step:${step.key}`
                                ? "selected"
                                : ""
                            }
                            type="button"
                            onClick={() => {
                              capture();
                              setDraft((current) => ({
                                ...current,
                                node: `step:${step.key}`,
                              }));
                            }}
                          >
                            {step.name}
                            <small>{executorNames[step.executor.type]}</small>
                          </ActionButton>
                        ))}
                        {!steps.length ? <small>暂无步骤</small> : null}
                      </div>
                    ) : null}
                  </div>
                );
              })}
              {draft.editing ? (
                <div className="actions">
                  <ActionButton
                    type="button"
                    onClick={() => {
                      const key = `stage_${crypto.randomUUID().slice(0, 8)}`;
                      update({
                        stages: [
                          ...document.stages,
                          {
                            key,
                            name: "新阶段",
                            description: "",
                            default_role_ref: null,
                          },
                        ],
                      });
                      setDraft((current) => ({
                        ...current,
                        node: `stage:${key}`,
                        expandedStages: [...current.expandedStages, key],
                      }));
                    }}
                  >
                    添加阶段
                  </ActionButton>
                  <ActionButton
                    type="button"
                    disabled={!document.stages.length}
                    onClick={() => addStep()}
                  >
                    添加步骤
                  </ActionButton>
                  <ActionButton
                    type="button"
                    disabled={!document.stages.length}
                    onClick={() => addStep("workflow")}
                  >
                    添加子流程调用
                  </ActionButton>
                </div>
              ) : null}
            </aside>
            <div className="workflow-step-form">
              <fieldset disabled={!draft.editing || busy}>
                {draft.node === "workflow" ? (
                  <>
                    <legend>流程配置</legend>
                    <label>
                      流程名称
                      <input
                        required
                        value={draft.name}
                        onChange={(event) =>
                          setDraft((current) => ({
                            ...current,
                            name: event.target.value,
                          }))
                        }
                      />
                    </label>
                    {!draft.selected ? (
                      <label>
                        定义范围
                        <ActionSelect
                          value={draft.scope}
                          onChange={(event) =>
                            setDraft((current) => ({
                              ...current,
                              scope: event.target.value,
                            }))
                          }
                        >
                          <option value="global">全局</option>
                          {project ? (
                            <option value="project">当前项目</option>
                          ) : null}
                        </ActionSelect>
                      </label>
                    ) : null}
                    <details>
                      <summary>流程输入输出与完成要求</summary>
                      <JsonField
                        label="流程输入端口"
                        value={document.inputs}
                        onChange={(inputs) => update({ inputs })}
                      />
                      <JsonField
                        label="流程输出引用"
                        value={document.outputs}
                        onChange={(outputs) => update({ outputs })}
                      />
                      <JsonField
                        label="流程完成条件"
                        value={document.completion}
                        onChange={(completion) => update({ completion })}
                      />
                    </details>
                  </>
                ) : null}
                {stage ? (
                  <>
                    <legend>阶段配置</legend>
                    <label>
                      阶段名称
                      <input
                        value={stage.name}
                        onChange={(event) =>
                          update({
                            stages: document.stages.map((item: Row) =>
                              item.key === stage.key
                                ? { ...item, name: event.target.value }
                                : item,
                            ),
                          })
                        }
                      />
                    </label>

                    <div className="actions">
                      {[-1, 1].map((delta) => (
                        <ActionButton
                          key={delta}
                          type="button"
                          disabled={
                            document.stages.findIndex(
                              (s: Row) => s.key === stage.key,
                            ) +
                              delta <
                              0 ||
                            document.stages.findIndex(
                              (s: Row) => s.key === stage.key,
                            ) +
                              delta >=
                              document.stages.length
                          }
                          onClick={() => {
                            const stages = [...document.stages];
                            const index = stages.findIndex(
                              (s) => s.key === stage.key,
                            );
                            [stages[index], stages[index + delta]] = [
                              stages[index + delta],
                              stages[index],
                            ];
                            update({ stages });
                          }}
                        >
                          {delta < 0 ? "上移阶段" : "下移阶段"}
                        </ActionButton>
                      ))}
                    </div>
                    <RevisionSelect
                      label="阶段默认 Role"
                      records={roles}
                      value={stage.default_role_ref || ""}
                      onChange={(default_role_ref) =>
                        update({
                          stages: document.stages.map((item: Row) =>
                            item.key === stage.key
                              ? {
                                  ...item,
                                  default_role_ref: default_role_ref || null,
                                }
                              : item,
                          ),
                        })
                      }
                    />
                    <MarkdownField
                      label="阶段背景"
                      value={stage.description || ""}
                      onChange={(description) =>
                        update({
                          stages: document.stages.map((item: Row) =>
                            item.key === stage.key
                              ? { ...item, description }
                              : item,
                          ),
                        })
                      }
                    />
                    <ActionButton
                      type="button"
                      disabled={document.steps.some(
                        (item: Row) => item.stage === stage.key,
                      )}
                      onClick={() => {
                        update({
                          stages: document.stages.filter(
                            (item: Row) => item.key !== stage.key,
                          ),
                        });
                        setDraft((current) => ({
                          ...current,
                          node: "workflow",
                        }));
                      }}
                    >
                      删除空阶段
                    </ActionButton>
                  </>
                ) : null}
                {step ? (
                  <>
                    <legend>步骤配置</legend>
                    <label>
                      步骤名称
                      <input
                        value={step.name}
                        onChange={(event) =>
                          updateStep({ name: event.target.value })
                        }
                      />
                    </label>
                    <label>
                      所属阶段
                      <ActionSelect
                        value={step.stage}
                        onChange={(event) =>
                          updateStep({ stage: event.target.value })
                        }
                      >
                        {document.stages.map((stage: Row) => (
                          <option key={stage.key} value={stage.key}>
                            {stage.name}
                          </option>
                        ))}
                      </ActionSelect>
                    </label>
                    <label>
                      执行方式
                      <ActionSelect
                        value={step.executor.type}
                        onChange={(event) => {
                          const type = event.target.value;
                          updateStep({
                            executor:
                              type === "agent"
                                ? { type, role_ref: null }
                                : type === "tool"
                                  ? { type, tool_ref: "", arguments: {} }
                                  : type === "workflow"
                                    ? { type, workflow_ref: "", foreach: null }
                                    : { type },
                            task_instructions:
                              type === "agent"
                                ? step.task_instructions || ""
                                : null,
                            required_skill_refs:
                              type === "agent"
                                ? step.required_skill_refs || []
                                : [],
                          });
                        }}
                      >
                        {Object.entries(executorNames).map(([key, name]) => (
                          <option key={key} value={key}>
                            {name}
                          </option>
                        ))}
                      </ActionSelect>
                    </label>
                    {step.executor.type === "agent" ? (
                      <>
                        <RevisionSelect
                          label="执行 Role（留空继承阶段）"
                          records={roles}
                          value={step.executor.role_ref || ""}
                          onChange={(role_ref) =>
                            updateStep({
                              executor: {
                                ...step.executor,
                                role_ref: role_ref || null,
                              },
                            })
                          }
                        />
                        <MarkdownField
                          key={`${draft.selected}:${step.key}:task`}
                          label="任务指令"
                          required
                          value={step.task_instructions || ""}
                          onChange={(task_instructions) =>
                            updateStep({ task_instructions })
                          }
                          rows={12}
                        />
                        <p className="muted">
                          描述本次工作目标；工作方法引用 Skill，验收依据引用
                          Spec。
                        </p>
                        <RevisionSelect
                          label="本次必需 Skills"
                          records={records.filter(
                            (row) => row.kind === "skill",
                          )}
                          multiple
                          value={step.required_skill_refs || []}
                          onChange={(required_skill_refs) =>
                            updateStep({ required_skill_refs })
                          }
                        />
                      </>
                    ) : null}
                    {step.executor.type === "tool" ? (
                      <>
                        <RevisionSelect
                          label="执行工具"
                          records={records.filter(
                            (row) =>
                              row.kind === "tool" &&
                              (row.data.schema_version === 2 ||
                                row.data.implementation === "platform"),
                          )}
                          required
                          value={step.executor.tool_ref || ""}
                          onChange={(tool_ref) =>
                            updateStep({
                              executor: {
                                ...step.executor,
                                tool_ref,
                                arguments: {},
                              },
                            })
                          }
                        />
                        <JsonField
                          key={`${step.key}:arguments`}
                          label="工具参数映射"
                          value={step.executor.arguments || {}}
                          onChange={(arguments_) =>
                            updateStep({
                              executor: {
                                ...step.executor,
                                arguments: arguments_,
                              },
                            })
                          }
                        />
                        <JsonField
                          label="工具输出映射（端口名 → JSON Pointer）"
                          value={step.executor.output_mapping || {}}
                          readOnly={!draft.editing}
                          onChange={(output_mapping) =>
                            updateStep({
                              executor: { ...step.executor, output_mapping },
                            })
                          }
                        />
                      </>
                    ) : null}
                    {step.executor.type === "workflow" ? (
                      <>
                        <RevisionSelect
                          label="调用子流程"
                          records={workflows.filter(
                            (row) => row.id !== draft.selected,
                          )}
                          required
                          value={step.executor.workflow_ref || ""}
                          onChange={(workflow_ref) =>
                            updateStep({
                              executor: { ...step.executor, workflow_ref },
                            })
                          }
                        />
                        {step.executor.workflow_ref ? (
                          <a
                            href={`?tab=workflows&workflow_id=${encodeURIComponent(step.executor.workflow_ref)}${project ? `&project_id=${encodeURIComponent(project.id)}` : ""}`}
                          >
                            编辑所引用的子流程
                          </a>
                        ) : null}
                        <label>
                          <input
                            type="checkbox"
                            checked={!!step.executor.work_binding}
                            onChange={(event) =>
                              updateStep({
                                executor: {
                                  ...step.executor,
                                  work_binding: event.target.checked
                                    ? {
                                        input:
                                          step.executor.foreach?.item_input ||
                                          "work",
                                        pointer: "/work_ref",
                                      }
                                    : null,
                                },
                              })
                            }
                          />
                          按工作已采用的规范调用
                        </label>
                        {step.executor.work_binding ? (
                          <>
                            <label>
                              工作引用输入名
                              <input
                                value={step.executor.work_binding.input}
                                onChange={(event) =>
                                  updateStep({
                                    executor: {
                                      ...step.executor,
                                      work_binding: {
                                        ...step.executor.work_binding,
                                        input: event.target.value,
                                      },
                                    },
                                  })
                                }
                              />
                            </label>
                            <label>
                              工作引用 JSON Pointer
                              <input
                                value={step.executor.work_binding.pointer}
                                onChange={(event) =>
                                  updateStep({
                                    executor: {
                                      ...step.executor,
                                      work_binding: {
                                        ...step.executor.work_binding,
                                        pointer: event.target.value,
                                      },
                                    },
                                  })
                                }
                              />
                            </label>
                          </>
                        ) : null}
                        <label>
                          <input
                            type="checkbox"
                            checked={!!step.executor.foreach}
                            onChange={(event) =>
                              updateStep({
                                executor: {
                                  ...step.executor,
                                  foreach: event.target.checked
                                    ? {
                                        source: { step: "", port: "" },
                                        item_input: "item",
                                        max_concurrency: 1,
                                        min_items: 1,
                                      }
                                    : null,
                                },
                              })
                            }
                          />
                          对集合逐项执行
                        </label>
                        {step.executor.foreach ? (
                          <>
                            <SourceSelect
                              document={document}
                              step={step}
                              value={step.executor.foreach.source}
                              label="集合来源"
                              onChange={(source) =>
                                updateStep({
                                  executor: {
                                    ...step.executor,
                                    foreach: {
                                      ...step.executor.foreach,
                                      source,
                                    },
                                  },
                                  dependencies:
                                    source.step === "$input"
                                      ? step.dependencies
                                      : [
                                          ...new Set([
                                            ...step.dependencies,
                                            source.step,
                                          ]),
                                        ],
                                })
                              }
                            />
                            <label>
                              子流程条目输入名
                              <input
                                value={step.executor.foreach.item_input}
                                onChange={(event) =>
                                  updateStep({
                                    executor: {
                                      ...step.executor,
                                      foreach: {
                                        ...step.executor.foreach,
                                        item_input: event.target.value,
                                      },
                                    },
                                  })
                                }
                              />
                            </label>
                            <label>
                              集合并发上限
                              <input
                                type="number"
                                min={1}
                                max={32}
                                value={step.executor.foreach.max_concurrency}
                                onChange={(event) =>
                                  updateStep({
                                    executor: {
                                      ...step.executor,
                                      foreach: {
                                        ...step.executor.foreach,
                                        max_concurrency: Number(
                                          event.target.value,
                                        ),
                                      },
                                    },
                                  })
                                }
                              />
                            </label>
                          </>
                        ) : null}
                      </>
                    ) : null}
                    <RevisionSelect
                      label="验收 Spec"
                      records={records.filter((row) => row.kind === "spec")}
                      multiple
                      value={step.spec_refs || []}
                      onChange={(spec_refs) => updateStep({ spec_refs })}
                    />
                    <label>
                      前置步骤
                      <ActionSelect
                        multiple
                        value={step.dependencies || []}
                        onChange={(event) =>
                          updateStep({
                            dependencies: Array.from(
                              event.target.selectedOptions,
                              (option) => option.value,
                            ),
                          })
                        }
                      >
                        {document.steps
                          .filter((item: Row) => item.key !== step.key)
                          .map((item: Row) => (
                            <option key={item.key} value={item.key}>
                              {item.name}
                            </option>
                          ))}
                      </ActionSelect>
                    </label>
                    <WorkflowDeliveries
                      document={document}
                      step={step}
                      records={records}
                      onChange={updateStep}
                      onRenameOutput={renameOutput}
                    />
                    <label>
                      完成确认
                      <ActionSelect
                        value={step.confirmation || "automatic"}
                        onChange={(event) =>
                          updateStep({ confirmation: event.target.value })
                        }
                      >
                        <option value="automatic">满足条件后自动完成</option>
                        <option value="human">等待人工确认</option>
                      </ActionSelect>
                    </label>
                    <details>
                      <summary>入口与完成条件</summary>
                      <JsonField
                        key={`${step.key}:condition`}
                        label="入口条件"
                        value={step.condition || { op: "always" }}
                        onChange={(condition) => updateStep({ condition })}
                      />
                      <label>
                        <input
                          type="checkbox"
                          checked={!!step.allow_completion_override}
                          disabled={!draft.editing}
                          onChange={(event) =>
                            updateStep({
                              allow_completion_override: event.target.checked,
                            })
                          }
                        />
                        允许人工说明理由后覆盖完成条件
                      </label>
                      <JsonField
                        key={`${step.key}:completion`}
                        label="完成条件"
                        value={step.completion || { op: "always" }}
                        onChange={(completion) => updateStep({ completion })}
                      />
                      <label>
                        步骤标签
                        <input
                          value={(step.tags || []).join(", ")}
                          onChange={(event) =>
                            updateStep({ tags: splitPaths(event.target.value) })
                          }
                        />
                      </label>
                    </details>
                    <details>
                      <summary>执行范围与预算</summary>
                      <label>
                        工作空间输入名
                        <input
                          value={step.access?.workspace_input || ""}
                          onChange={(event) =>
                            updateStep({
                              access: {
                                ...step.access,
                                workspace_input: event.target.value || null,
                              },
                            })
                          }
                        />
                      </label>
                      <label>
                        工作目录（相对工作空间）
                        <input
                          value={step.access?.working_directory || ""}
                          onChange={(event) =>
                            updateStep({
                              access: {
                                ...step.access,
                                working_directory: event.target.value || null,
                              },
                            })
                          }
                        />
                      </label>
                      <p className="muted">
                        工作空间读写范围及工具授权由项目选定的 Permission
                        配置管理。
                      </p>
                      <JsonField
                        key={`${step.key}:budget`}
                        label="执行预算"
                        value={
                          step.budget || {
                            max_turns: 8,
                            timeout_seconds: 300,
                            retry_limit: 3,
                          }
                        }
                        onChange={(budget) => updateStep({ budget })}
                      />
                    </details>
                    <ActionButton
                      type="button"
                      onClick={() => {
                        const dependents = document.steps.filter((item: Row) =>
                          item.dependencies?.includes(step.key),
                        );
                        if (dependents.length) {
                          setError(
                            "请先移除后续步骤对此步骤的依赖和材料引用。",
                          );
                          return;
                        }
                        update({
                          steps: document.steps.filter(
                            (item: Row) => item.key !== step.key,
                          ),
                        });
                        setDraft((current) => ({
                          ...current,
                          node: "workflow",
                        }));
                      }}
                    >
                      删除步骤
                    </ActionButton>
                  </>
                ) : null}
              </fieldset>
              {step?.executor.type === "agent" ? (
                <WorkflowTaskPreview
                  request={request}
                  document={document}
                  step={step}
                  records={records}
                  project={project}
                  workflowId={draft.selected}
                />
              ) : null}
            </div>
          </div>
          {validation ? (
            <p role="status">
              已解析 {validation.roles?.length || 0} 个 Role，
              {validation.specification_view?.length || 0} 个步骤。
            </p>
          ) : null}
        </>
      )}
      {error ? (
        <p role="alert" className="error">
          {error}
        </p>
      ) : null}
      {message ? (
        <p role="status" className="notice">
          {message}
        </p>
      ) : null}
    </section>
  );
}

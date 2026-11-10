import { useEffect, useRef, useState } from "react";
import { ActionButton, useActionFeedback } from "./ActionFeedback";
import { MarkdownPreview, type Request, type Row } from "./configuration-ui";

type Props = {
  nested?: boolean;
  request: Request;
  record: Row;
  projectId: string;
  refresh: () => Promise<void>;
  onDiscussion: (id: string, owner: string) => void;
};
const stateLabels: Record<string, string> = {
  queued: "等待前置",
  running: "执行中",
  waiting_input: "等待输入",
  waiting_confirmation: "等待确认",
  completed: "已完成",
  failed: "失败",
  cancelled: "已取消",
  skipped: "已跳过",
  needs_reconciliation: "需要核对",
};

export function WorkflowInstanceView({
  request,
  record,
  nested = false,
  projectId,
  refresh,
  onDiscussion,
}: Props) {
  const { error, setError, capture, beginFeedback, endFeedback } =
    useActionFeedback();
  const [data, setData] = useState<Row>(),
    [busy, setBusy] = useState(false),
    [selected, setSelected] = useState("");
  const [input, setInput] = useState(""),
    [reason, setReason] = useState(""),
    [override, setOverride] = useState(false);
  const storageKey = `devdesk:v4:runtime:${projectId}:${record.id}`;
  const [expanded, setExpanded] = useState<Record<string, boolean>>(() => {
    try {
      return JSON.parse(localStorage.getItem(storageKey) || "{}");
    } catch {
      return {};
    }
  });
  const epoch = useRef(0);
  async function load() {
    const current = ++epoch.current;
    const result = await request("query.workflow_instance", {
      id: record.id,
      project_id: projectId,
    });
    if (epoch.current === current) setData(result);
  }
  useEffect(() => {
    void load().catch((error) => setError(String(error)));
    return () => {
      epoch.current++;
    };
  }, [record, projectId, request]);
  useEffect(() => {
    try {
      localStorage.setItem(storageKey, JSON.stringify(expanded));
    } catch {}
  }, [storageKey, expanded]);
  const runtime = data?.runtime || record.data,
    current = data?.record || record,
    plan = data?.plan || runtime.plan?.document;
  async function act(method: string, payload: Row = {}) {
    capture();
    beginFeedback();
    setBusy(true);
    setError("");
    try {
      await request(method, {
        project_id: projectId,
        command_id: crypto.randomUUID(),
        target: current.id,
        expected_revision: current.revision,
        payload,
      });
      await load();
      await refresh();
      setInput("");
      setReason("");
      setOverride(false);
    } catch (error) {
      setError(String(error));
    } finally {
      setBusy(false);
      endFeedback();
    }
  }
  const toggle = (key: string) =>
    setExpanded((previous) => ({ ...previous, [key]: !previous[key] }));
  return (
    <section
      className={`panel workflow-instance${nested ? " workflow-instance-child" : ""}`}
      aria-label={nested ? "子流程进度" : "当前工作流"}
    >
      <div className="panel-heading">
        <div>
          <h3>{current.name || "工作流程"}</h3>
          <p>
            {stateLabels[runtime.status] || runtime.status}
            {runtime.paused ? " · 已暂停" : ""}
          </p>
        </div>
        <div className="actions">
          <ActionButton
            type="button"
            disabled={busy}
            onClick={() =>
              void load().catch((error) => setError(String(error)))
            }
          >
            刷新进度
          </ActionButton>
          {!["completed", "cancelled"].includes(runtime.status) ? (
            <>
              <ActionButton
                type="button"
                disabled={busy}
                onClick={() =>
                  void act(
                    runtime.paused ? "workflow.resume" : "workflow.pause",
                  )
                }
              >
                {runtime.paused ? "恢复流程" : "暂停流程"}
              </ActionButton>
              <ActionButton
                type="button"
                disabled={busy || runtime.cancel_requested}
                onClick={() => void act("workflow.cancel")}
              >
                停止流程
              </ActionButton>
            </>
          ) : null}
        </div>
      </div>
      {(plan?.stages || []).map((stage: Row) => {
        const definitions = plan.steps.filter(
          (step: Row) => step.stage === stage.key,
        );
        const executions = definitions
          .map((definition: Row) => ({
            definition,
            execution: runtime.steps?.find(
              (execution: Row) => execution.step === definition.key,
            ),
          }))
          .filter((row: Row) => row.execution);
        const completed = executions.filter((row: Row) =>
          ["completed", "skipped"].includes(row.execution.status),
        ).length;
        return (
          <section className="workflow-stage-row" key={stage.key}>
            <div className="panel-heading">
              <div>
                <h4>{stage.name}</h4>
                <p>
                  {completed} / {executions.length}
                </p>
              </div>
              <ActionButton
                type="button"
                onClick={() => toggle(`stage:${stage.key}`)}
              >
                {expanded[`stage:${stage.key}`] ? "展开阶段" : "折叠阶段"}
              </ActionButton>
            </div>
            {!expanded[`stage:${stage.key}`] ? (
              <>
                {stage.description ? (
                  <MarkdownPreview text={stage.description} />
                ) : null}
                {executions.map(({ definition, execution }: Row) => (
                  <article
                    key={execution.id}
                    className={`workflow-step-runtime status-${execution.status}`}
                  >
                    <ActionButton
                      type="button"
                      onClick={() => {
                        setSelected((previous) =>
                          previous === execution.id ? "" : execution.id,
                        );
                        setReason("");
                        setOverride(false);
                      }}
                    >
                      {definition.name} ·{" "}
                      {stateLabels[execution.status] || execution.status}
                    </ActionButton>
                    {execution.reason ? <p>{execution.reason}</p> : null}
                    <div className="actions">
                      {execution.session_id ? (
                        <ActionButton
                          type="button"
                          onClick={() =>
                            onDiscussion(execution.session_id, runtime.owner_id)
                          }
                        >
                          打开 Agent 讨论
                        </ActionButton>
                      ) : null}
                      {["failed", "cancelled"].includes(execution.status) ? (
                        <ActionButton
                          type="button"
                          disabled={busy}
                          onClick={() =>
                            void act("workflow.step.retry", {
                              step: execution.step,
                            })
                          }
                        >
                          技术重试
                        </ActionButton>
                      ) : null}
                      {["completed", "failed", "cancelled"].includes(
                        execution.status,
                      ) ? (
                        <ActionButton
                          type="button"
                          disabled={busy}
                          onClick={() =>
                            void act("workflow.step.rework", {
                              step: execution.step,
                            })
                          }
                        >
                          从此步骤返工
                        </ActionButton>
                      ) : null}
                    </div>
                    {execution.status === "needs_reconciliation" ? (
                      <ActionButton
                        disabled={busy}
                        onClick={() =>
                          void act("workflow.step.reconcile", {
                            step: execution.step,
                          })
                        }
                      >
                        核对执行结果
                      </ActionButton>
                    ) : null}
                    {execution.status === "waiting_input" ? (
                      <label>
                        补充本次任务
                        <textarea
                          value={selected === execution.id ? input : ""}
                          onFocus={() => setSelected(execution.id)}
                          onChange={(event) => setInput(event.target.value)}
                        />
                        <ActionButton
                          type="button"
                          disabled={
                            busy || selected !== execution.id || !input.trim()
                          }
                          onClick={() =>
                            void act("workflow.step.input", {
                              step: execution.step,
                              input,
                            })
                          }
                        >
                          提交补充输入
                        </ActionButton>
                      </label>
                    ) : null}
                    {execution.status === "waiting_confirmation" &&
                    (definition.confirmation === "human" ||
                      definition.allow_completion_override) ? (
                      <div>
                        {definition.allow_completion_override ? (
                          <label>
                            <input
                              type="checkbox"
                              checked={selected === execution.id && override}
                              onChange={(event) => {
                                setSelected(execution.id);
                                setOverride(event.target.checked);
                              }}
                            />
                            明确覆盖未满足的完成条件
                          </label>
                        ) : null}
                        <label>
                          确认说明
                          <textarea
                            value={selected === execution.id ? reason : ""}
                            onFocus={() => setSelected(execution.id)}
                            onChange={(event) => setReason(event.target.value)}
                          />
                        </label>
                        <ActionButton
                          type="button"
                          disabled={busy || (override && !reason.trim())}
                          onClick={() =>
                            void act("workflow.step.confirm", {
                              step: execution.step,
                              input_digest: execution.input_digest,
                              outputs: execution.outputs,
                              reason,
                              override_completion: override,
                            })
                          }
                        >
                          确认此版本结果
                        </ActionButton>
                      </div>
                    ) : null}
                    {Object.entries(execution.outputs || {}).map(
                      ([port, id]) => {
                        const output = data?.outputs?.[id as string];
                        return output ? (
                          <details key={port}>
                            <summary>产物 · {port}</summary>
                            {typeof output.value === "string" ? (
                              <MarkdownPreview text={output.value} />
                            ) : (
                              <pre>{JSON.stringify(output.value, null, 2)}</pre>
                            )}
                          </details>
                        ) : null;
                      },
                    )}
                    {selected === execution.id ? (
                      <>
                        <MarkdownPreview
                          text={definition.task_instructions || ""}
                        />
                        <details>
                          <summary>本轮任务来源与执行记录</summary>
                          <pre>{JSON.stringify(execution, null, 2)}</pre>
                        </details>
                      </>
                    ) : null}
                    {(data?.children || [])
                      .filter(
                        (child: Row) =>
                          child.data.parent_execution_id === execution.id,
                      )
                      .map((child: Row) => (
                        <div key={child.id}>
                          <ActionButton
                            type="button"
                            onClick={() => toggle(child.id)}
                          >
                            子流程 · {child.name} ·{" "}
                            {stateLabels[child.data.status] ||
                              child.data.status}{" "}
                            · {expanded[child.id] ? "折叠" : "展开"}
                          </ActionButton>
                          {expanded[child.id] ? (
                            <WorkflowInstanceView
                              nested
                              request={request}
                              record={child}
                              projectId={projectId}
                              refresh={refresh}
                              onDiscussion={onDiscussion}
                            />
                          ) : null}
                        </div>
                      ))}
                  </article>
                ))}
              </>
            ) : null}
          </section>
        );
      })}
      {error ? (
        <p role="alert" className="error">
          {error}
        </p>
      ) : null}
    </section>
  );
}

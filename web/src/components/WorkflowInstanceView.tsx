import { displayText, truncateText } from "../display-text";
import { useCallback, useEffect, useRef, useState } from "react";
import { ChevronRight } from "lucide-react";
import { ActionButton } from "./ActionFeedback";
import { useActionFeedback } from "./action-feedback";
import { type Request, type Row } from "./rpc-types";
import { ReadingLink, ReadState } from "./reading";
import { WorkflowReferences } from "./WorkflowReferences";
import { useReading } from "./reading-state";

type Props = {
  nested?: boolean;
  request: Request;
  record: Row;
  projectId: string;
  refresh: () => Promise<void>;
  onDiscussion: (id: string, owner: string, turnId?: string) => void;
  target?: Row;
  ownerId?: string;
};
const stateLabels: Record<string, string> = {
  queued: "等待前置",
  running: "执行中",
  waiting_input: "等待输入",
  waiting_confirmation: "等待人类确认",
  awaiting_acceptance: "等待接纳",
  completed: "已完成",
  failed: "失败",
  cancelled: "已取消",
  skipped: "已跳过",
  needs_reconciliation: "需要核对",
};
const waitingStatuses = [
  "waiting_input",
  "waiting_confirmation",
  "awaiting_acceptance",
];
// Collapsed stage rows still report where the work currently stands.
function stageStatus(executions: Row[]): { key: string; label: string } {
  const statuses = executions.map((row: Row) => row.execution.status);
  const pick = (key: string) => ({ key, label: stateLabels[key] || key });
  if (statuses.includes("failed")) return pick("failed");
  if (statuses.includes("needs_reconciliation"))
    return pick("needs_reconciliation");
  if (statuses.includes("running")) return pick("running");
  const waiting = statuses.find((status) => waitingStatuses.includes(status));
  if (waiting) return pick(waiting);
  if (
    statuses.length &&
    statuses.every((status) => ["completed", "skipped"].includes(status))
  )
    return pick("completed");
  return pick("queued");
}

export function WorkflowInstanceView({
  request,
  record,
  nested = false,
  projectId,
  refresh,
  onDiscussion,
  target = {},
  ownerId,
}: Props) {
  const { error, setError, capture, beginFeedback, endFeedback } =
    useActionFeedback();
  const [data, setData] = useState<Row>(),
    [busy, setBusy] = useState(false),
    [selected, setSelected] = useState("");
  const [input, setInput] = useState("");
  const [reasons, setReasons] = useState<Record<string, string>>({});
  const storageKey = `devdesk:v5:runtime:${projectId}:${record.id}`;
  const [open, setOpen] = useState<Record<string, boolean>>(() => {
    try {
      return JSON.parse(localStorage.getItem(storageKey) || "{}");
    } catch {
      return {};
    }
  });
  const epoch = useRef(0);
  const focusedTarget = useRef("");
  const invalidate = useCallback(() => {
    epoch.current += 1;
  }, []);
  const load = useCallback(async () => {
    const current = ++epoch.current;
    const result = await request("query.workflow_instance", {
      id: record.id,
      project_id: projectId,
    });
    if (epoch.current === current) setData(result);
  }, [request, record.id, projectId]);
  useEffect(() => {
    let active = true;
    let timer: ReturnType<typeof setTimeout>;
    async function poll() {
      try {
        await load();
      } catch (error) {
        if (active) setError(String(error));
      } finally {
        if (active) timer = setTimeout(() => void poll(), 2500);
      }
    }
    void poll();
    return () => {
      active = false;
      clearTimeout(timer);
      invalidate();
    };
  }, [record, projectId, request, load, setError, invalidate]);
  useEffect(() => {
    try {
      localStorage.setItem(storageKey, JSON.stringify(open));
    } catch {}
  }, [storageKey, open]);
  const runtime = data?.runtime || record.data,
    current = data?.record || record,
    plan = data?.plan || runtime.plan?.document;
  const historical = useReading(
    request,
    "query.workflow_executions",
    {
      project_id: projectId,
      owner_id: ownerId || runtime.owner_id,
      instance_id: record.id,
      execution_id: target.execution_id,
    },
    Boolean(
      data &&
      target.execution_id &&
      target.instance_id === record.id &&
      !runtime.steps.some((step: Row) => step.id === target.execution_id),
    ),
  );
  useEffect(() => {
    if (!target.execution_id) {
      focusedTarget.current = "";
      return;
    }
    if (!data) return;
    const focus = `${record.id}:${target.instance_id}:${target.execution_id}`;
    if (focusedTarget.current === focus) return;
    const selected = data.runtime.steps.find(
      (step: Row) => step.id === target.execution_id,
    );
    const next: Record<string, boolean> = {};
    if (selected) {
      const definition = data.plan?.steps?.find(
        (step: Row) => step.key === selected.step,
      );
      if (definition) next[`stage:${definition.stage}`] = true;
      next[`step:${selected.id}`] = true;
    } else if (target.instance_id && target.instance_id !== record.id) {
      for (const child of data.children || []) {
        const parent = data.runtime.steps.find(
          (step: Row) => step.id === child.data.parent_execution_id,
        );
        const definition = data.plan?.steps?.find(
          (step: Row) => step.key === parent?.step,
        );
        if (definition) next[`stage:${definition.stage}`] = true;
        if (parent) next[`step:${parent.id}`] = true;
        next[child.id] = true;
      }
    }
    if (!Object.keys(next).length) return;
    focusedTarget.current = focus;
    setOpen((current) =>
      Object.keys(next).some((key) => !current[key])
        ? { ...current, ...next }
        : current,
    );
  }, [target.execution_id, target.instance_id, record.id, data]);
  async function act(method: string, payload: Row = {}) {
    capture();
    beginFeedback();
    setBusy(true);
    setError("");
    try {
      const fresh = await request("query.workflow_instance", {
        id: current.id,
        project_id: projectId,
      });
      await request(method, {
        project_id: projectId,
        command_id: crypto.randomUUID(),
        target: current.id,
        expected_revision: fresh.record.revision,
        payload: {
          plan_revision_id: runtime.active_plan_revision_id,
          control_version: fresh.runtime.control_version,
          ...payload,
        },
      });
      await load();
      await refresh();
      setInput("");
      setReasons((previous) => {
        const next = { ...previous };
        delete next[payload.execution_id];
        return next;
      });
    } catch (error) {
      setError(String(error));
    } finally {
      setBusy(false);
      endFeedback();
    }
  }
  const toggle = (key: string) =>
    setOpen((previous) => ({ ...previous, [key]: !previous[key] }));
  return (
    <section
      className={`panel workflow-instance${nested ? " workflow-instance-child" : ""}`}
      aria-label={displayText(nested ? "子流程进度" : "当前工作流")}
    >
      <div className="panel-heading">
        <div>
          <h3>{displayText(current.name || "工作流程")}</h3>
          <p>
            {displayText(stateLabels[runtime.status] || runtime.status)}
            {displayText(runtime.paused ? " · 已暂停" : "")}
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
                {displayText(runtime.paused ? "恢复流程" : "暂停流程")}
              </ActionButton>
              <ActionButton
                type="button"
                disabled={
                  busy ||
                  !runtime.steps.every((s: Row) =>
                    ["completed", "skipped"].includes(s.status),
                  )
                }
                onClick={() =>
                  void act("workflow.complete", {
                    reason: "用户确认完成当前流程",
                  })
                }
              >
                完成流程
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
      {historical.loading && <ReadState {...historical} />}
      {historical.error && <ReadState {...historical} />}
      {historical.value?.record && (
        <article className="workflow-step-runtime" aria-label="历史流程步骤">
          <h4>
            {displayText(historical.value.record.step_name)} · 历史执行第{" "}
            {displayText(historical.value.record.round)} 轮
          </h4>
          <p>
            {displayText(
              historical.value.record.reason || historical.value.record.status,
            )}
          </p>
          <p className="muted">
            此步骤属于原计划{" "}
            {displayText(historical.value.record.plan_revision_id)}
            ，下方展示当前流程。
          </p>
          <WorkflowReferences
            source={{
              ...historical.value.record,
              execution_id: historical.value.record.id,
            }}
            projectId={projectId}
            ownerId={ownerId || runtime.owner_id}
            current="overview"
          />
        </article>
      )}
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
        const status = stageStatus(executions);
        const stageKey = `stage:${stage.key}`;
        const stageOpen = !!open[stageKey];
        return (
          <section
            className={`workflow-stage-row status-${status.key}`}
            key={stage.key}
          >
            <ActionButton
              type="button"
              className="workflow-stage-toggle"
              aria-expanded={stageOpen}
              onClick={() => toggle(stageKey)}
            >
              <ChevronRight aria-hidden="true" />
              <strong>{displayText(stage.name)}</strong>
              <span>
                {displayText(completed)} / {displayText(executions.length)} ·{" "}
                {displayText(status.label)}
              </span>
            </ActionButton>
            {stageOpen ? (
              <div className="workflow-stage-body">
                {stage.description ? (
                  <p className="muted">
                    {displayText(truncateText(stage.description, 180))}
                    {displayText(stage.description.length > 180 ? "…" : "")}
                  </p>
                ) : null}
                {executions.map(({ definition, execution }: Row) => {
                  const completion = data?.completion_status?.[execution.id];
                  const unmet = completion?.satisfied === false;
                  const confirmation = completion?.confirmation;
                  const humanSatisfied =
                    completion?.human_satisfied ??
                    (definition.confirmation !== "human" || !!confirmation);
                  const stepKey = `step:${execution.id}`;
                  const stepOpen = !!open[stepKey];
                  const children = (data?.children || []).filter(
                    (child: Row) =>
                      child.data.parent_execution_id === execution.id,
                  );
                  return (
                    <article
                      key={execution.id}
                      id={`workflow-execution-${execution.id}`}
                      aria-current={
                        target.execution_id === execution.id
                          ? "step"
                          : undefined
                      }
                      aria-label={displayText(`步骤 · ${definition.name}`)}
                      className={`workflow-step-runtime status-${execution.status}`}
                    >
                      <ActionButton
                        type="button"
                        className="workflow-step-toggle"
                        aria-expanded={stepOpen}
                        onClick={() => toggle(stepKey)}
                      >
                        <ChevronRight aria-hidden="true" />
                        <strong>{displayText(definition.name)}</strong>
                        <span className={`badge status-${execution.status}`}>
                          {displayText(
                            stateLabels[execution.status] || execution.status,
                          )}
                        </span>
                      </ActionButton>
                      <p className="workflow-step-summary">
                        {displayText(
                          data?.actors?.[execution.id]?.name ||
                            definition.executor?.type ||
                            "执行方式未记录",
                        )}{" "}
                        · 第 {displayText(execution.round)} 轮
                        {displayText(
                          execution.reason ? ` · ${execution.reason}` : "",
                        )}
                      </p>
                      {stepOpen ? (
                        <div className="workflow-step-body">
                          {definition.confirmation === "human" &&
                            completion &&
                            execution.status === "waiting_confirmation" &&
                            !humanSatisfied && (
                              <div role="status">
                                <p>
                                  {displayText(
                                    unmet
                                      ? "本步骤验收未通过，需要先返工。"
                                      : "本步骤验收已通过，请核对产物并确认此版本，后续步骤将等待你的确认。",
                                  )}
                                </p>
                                <div className="reading-links">
                                  {Object.entries(definition.inputs || {}).map(
                                    ([name, binding]) => {
                                      const source = (binding as Row).source;
                                      const producer =
                                        source &&
                                        runtime.steps?.find(
                                          (s: Row) => s.step === source.step,
                                        );
                                      const outputId =
                                        producer?.outputs?.[source?.port];
                                      return outputId ? (
                                        <ReadingLink
                                          key={name}
                                          detail="documents"
                                          projectId={projectId}
                                          ownerId={ownerId || runtime.owner_id}
                                          target={{ output_id: outputId }}
                                        >
                                          核对产物 ·{" "}
                                          {displayText(
                                            plan.steps.find(
                                              (s: Row) => s.key === source.step,
                                            )?.name || name,
                                          )}
                                        </ReadingLink>
                                      ) : null;
                                    },
                                  )}
                                </div>
                              </div>
                            )}
                          <WorkflowReferences
                            source={{
                              owner_kind: record._reading?.owner_kind,
                              feature_id: record._reading?.feature_id,
                              ...execution,
                              execution_id: execution.id,
                              instance_id: current.id,
                              owner_id: runtime.owner_id,
                            }}
                            projectId={projectId}
                            ownerId={ownerId || runtime.owner_id}
                            current="overview"
                          />
                          <div className="actions">
                            {execution.status === "queued" && (
                              <ActionButton
                                type="button"
                                disabled={busy || runtime.paused}
                                onClick={() =>
                                  void act("workflow.step.dispatch", {
                                    step: execution.step,
                                  })
                                }
                              >
                                执行步骤
                              </ActionButton>
                            )}
                            {[
                              "awaiting_acceptance",
                              "waiting_confirmation",
                            ].includes(execution.status) && (
                              <ActionButton
                                type="button"
                                disabled={busy || !humanSatisfied}
                                onClick={() =>
                                  void act("workflow.step.accept", {
                                    step: execution.step,
                                    execution_id: execution.id,
                                    input_digest: execution.input_digest,
                                    outputs: execution.outputs,
                                    reason: "用户核对并接纳结果",
                                  })
                                }
                              >
                                接纳结果
                              </ActionButton>
                            )}
                            {execution.session_id ? (
                              <ActionButton
                                type="button"
                                onClick={() =>
                                  onDiscussion(
                                    execution.session_id,
                                    runtime.owner_id,
                                    execution.turn_id,
                                  )
                                }
                              >
                                打开 Agent 讨论
                              </ActionButton>
                            ) : null}
                            {["failed", "cancelled"].includes(
                              execution.status,
                            ) ? (
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
                            {[
                              "completed",
                              "failed",
                              "cancelled",
                              "waiting_confirmation",
                              "awaiting_acceptance",
                            ].includes(execution.status) ? (
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
                            {execution.status === "needs_reconciliation" ? (
                              <ActionButton
                                type="button"
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
                          </div>
                          {execution.status === "waiting_input" ? (
                            <div className="workflow-step-inline-input">
                              <label>
                                补充本次任务
                                <textarea
                                  value={selected === execution.id ? input : ""}
                                  onFocus={() => setSelected(execution.id)}
                                  onChange={(event) =>
                                    setInput(event.target.value)
                                  }
                                />
                              </label>
                              <ActionButton
                                type="button"
                                disabled={
                                  busy ||
                                  selected !== execution.id ||
                                  !input.trim()
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
                            </div>
                          ) : null}
                          {[
                            "waiting_confirmation",
                            "awaiting_acceptance",
                          ].includes(execution.status) ? (
                            <div role="group" aria-label="步骤审批">
                              {unmet ? (
                                <p role="status">
                                  校验未通过，完成条件尚未满足。可以返工，或填写理由后手动
                                  Approve；原校验报告会保留。
                                </p>
                              ) : null}
                              {!unmet &&
                                definition.confirmation !== "human" && (
                                  <p className="muted">
                                    此步骤由项目经理自动接纳，无需人工确认；也可在此手动接纳当前结果。
                                  </p>
                                )}
                              <div className="workflow-step-inline-input">
                                <label>
                                  {displayText(
                                    unmet
                                      ? "手动批准理由（必填）"
                                      : definition.confirmation === "human"
                                        ? "确认说明"
                                        : "接纳说明",
                                  )}
                                  <textarea
                                    value={reasons[execution.id] || ""}
                                    onChange={(event) =>
                                      setReasons((previous) => ({
                                        ...previous,
                                        [execution.id]: event.target.value,
                                      }))
                                    }
                                  />
                                </label>
                                <ActionButton
                                  type="button"
                                  disabled={
                                    busy ||
                                    runtime.cancel_requested ||
                                    (unmet && !reasons[execution.id]?.trim())
                                  }
                                  onClick={() =>
                                    void act(
                                      unmet
                                        ? "workflow.step.approve"
                                        : definition.confirmation === "human"
                                          ? "workflow.step.confirm"
                                          : "workflow.step.accept",
                                      {
                                        step: execution.step,
                                        execution_id: execution.id,
                                        input_digest: execution.input_digest,
                                        outputs: execution.outputs,
                                        accept_result: true,
                                        reason: reasons[execution.id] || "",
                                      },
                                    )
                                  }
                                >
                                  {displayText(
                                    unmet
                                      ? "手动 Approve 此版本"
                                      : definition.confirmation === "human"
                                        ? "确认此版本结果"
                                        : "接纳此版本结果",
                                  )}
                                </ActionButton>
                              </div>
                            </div>
                          ) : null}
                          {confirmation ? (
                            <p role="status">
                              {displayText(
                                confirmation.override_completion
                                  ? "已手动批准，原完成条件未满足"
                                  : "已人工确认",
                              )}
                              {displayText(
                                confirmation.reason
                                  ? `：${confirmation.reason}`
                                  : "",
                              )}
                            </p>
                          ) : null}
                          {children.map((child: Row) => (
                            <div className="workflow-child" key={child.id}>
                              <ActionButton
                                type="button"
                                className="workflow-child-toggle"
                                aria-expanded={!!open[child.id]}
                                aria-label={displayText(
                                  `子流程 · ${child.name} · ${stateLabels[child.data.status] || child.data.status} · ${open[child.id] ? "折叠" : "展开"}`,
                                )}
                                onClick={() => toggle(child.id)}
                              >
                                <ChevronRight aria-hidden="true" />
                                <strong>
                                  子流程 · {displayText(child.name)}
                                </strong>
                                <span
                                  className={`badge status-${child.data.status}`}
                                >
                                  {displayText(
                                    stateLabels[child.data.status] ||
                                      child.data.status,
                                  )}
                                </span>
                              </ActionButton>
                              {open[child.id] ? (
                                <div className="workflow-inline-subflow">
                                  <WorkflowInstanceView
                                    nested
                                    target={target}
                                    request={request}
                                    record={{
                                      ...child,
                                      _reading: record._reading,
                                    }}
                                    ownerId={ownerId}
                                    projectId={projectId}
                                    refresh={refresh}
                                    onDiscussion={onDiscussion}
                                  />
                                </div>
                              ) : null}
                            </div>
                          ))}
                        </div>
                      ) : null}
                    </article>
                  );
                })}
              </div>
            ) : null}
          </section>
        );
      })}
      {error ? (
        <p role="alert" className="error">
          {displayText(error)}
        </p>
      ) : null}
    </section>
  );
}

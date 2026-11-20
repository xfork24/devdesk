import { displayText, truncateText } from "../display-text";
import { useCallback, useEffect, useRef, useState } from "react";
import { useRuntimeRefresh } from "./useRuntimeRefresh";
import { ChevronRight } from "lucide-react";
import { ActionButton } from "./ActionFeedback";
import { useActionFeedback } from "./action-feedback";
import { type Request, type Row } from "./rpc-types";
import { ReadingLink, ReadState } from "./reading";
import { WorkflowStepInput } from "./WorkflowStepInput";
import { WorkflowReferences } from "./WorkflowReferences";
import { WorkflowStageProgress } from "./WorkflowStageProgress";
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
type ActionBaseline = {
  target: string;
  revision: number;
  planRevision: string;
  controlVersion: number;
  stepRevision?: number;
  execution?: Row;
};
type StageNavigation = {
  key: string;
  hasSteps: boolean;
  visitedPages: number[];
};
const stepPageSize = 20;
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
    [busy, setBusy] = useState(false);
  const [reasons, setReasons] = useState<Record<string, string>>({});
  const [childOffset, setChildOffset] = useState(0);
  const [stepOffset, setStepOffset] = useState(0);
  const [stageNavigation, setStageNavigation] = useState<StageNavigation>();
  const stageElements = useRef(new Map<string, HTMLElement>());
  const drafts = useRef(new Map<string, ActionBaseline>());
  const attempts = useRef(
    new Map<string, { fingerprint: string; id: string }>(),
  );
  const storageKey = `devdesk:v5:runtime:${projectId}:${record.id}`;
  const [open, setOpen] = useState<Record<string, boolean>>(() => {
    try {
      return JSON.parse(localStorage.getItem(storageKey) || "{}");
    } catch {
      return {};
    }
  });
  const opened = useRef(open);
  opened.current = open;
  const epoch = useRef(0);
  const focusedTarget = useRef("");
  const locatedTarget = useRef("");
  const invalidate = useCallback(() => {
    epoch.current += 1;
  }, []);
  const load = useCallback(
    async (fresh = false) => {
      const current = ++epoch.current;
      const focusKey = `${target.instance_id || ""}:${target.execution_id || ""}`;
      const locate =
        target.instance_id &&
        target.execution_id &&
        locatedTarget.current !== focusKey;
      const result = await request(
        "query.workflow_instance",
        {
          id: record.id,
          project_id: projectId,
          view: "status",
          child_offset: childOffset,
          step_offset: stepOffset,
          ...(locate
            ? {
                focus_instance_id: target.instance_id,
                focus_execution_id: target.execution_id,
              }
            : {}),
        },
        { fresh },
      );
      const completionStatus = { ...result.completion_status };
      await Promise.all(
        (result.runtime?.steps || [])
          .filter(
            (step: Row) =>
              opened.current["step:" + step.id] &&
              ["waiting_confirmation", "awaiting_acceptance"].includes(
                step.status,
              ),
          )
          .map(async (step: Row) => {
            const qualified = await request("query.workflow_instance", {
              id: record.id,
              project_id: projectId,
              execution_id: step.id,
              view: "qualification",
            });
            if (qualified.completion_status?.[step.id])
              completionStatus[step.id] = qualified.completion_status[step.id];
          }),
      );
      if (epoch.current === current) {
        if (locate) {
          locatedTarget.current = focusKey;
          setStepOffset(result.step_offset || 0);
          setChildOffset(result.child_offset || 0);
        }
        setData({
          ...result,
          completion_status: completionStatus,
          _projectId: projectId,
        });
      }
    },
    [
      request,
      record.id,
      projectId,
      childOffset,
      stepOffset,
      target.instance_id,
      target.execution_id,
    ],
  );
  useRuntimeRefresh(load, projectId, record.id, {
    invalidate,
    onError: (value) => setError(String(value)),
  });
  const openedSteps = JSON.stringify(
    Object.keys(open).filter((key) => key.startsWith("step:") && open[key]),
  );
  useEffect(() => {
    if (openedSteps !== "[]")
      void load().catch((value) => setError(String(value)));
  }, [openedSteps, load, setError]);

  useEffect(() => {
    try {
      localStorage.setItem(storageKey, JSON.stringify(open));
    } catch {}
  }, [storageKey, open]);
  const activeData =
    data?._projectId === projectId && data?.record?.id === record.id
      ? data
      : undefined;
  const runtime = activeData?.runtime || record.data,
    current = activeData?.record || record,
    plan = activeData?.plan || runtime.plan?.document;
  useEffect(() => {
    if (
      !stageNavigation ||
      !activeData ||
      (activeData.step_offset ?? 0) !== stepOffset
    )
      return;
    const hasLoadedSteps = activeData.plan?.steps?.some(
      (step: Row) => step.stage === stageNavigation.key,
    );
    if (stageNavigation.hasSteps && !hasLoadedSteps) {
      const visited = new Set([...stageNavigation.visitedPages, stepOffset]);
      for (
        let offset = 0;
        offset < activeData.step_total;
        offset += stepPageSize
      ) {
        if (!visited.has(offset)) {
          setStageNavigation({
            ...stageNavigation,
            visitedPages: [...visited],
          });
          setStepOffset(offset);
          return;
        }
      }
    }
    const element = stageElements.current.get(stageNavigation.key);
    if (!element || open[`stage:${stageNavigation.key}`] === false) return;
    element.scrollIntoView({
      behavior: matchMedia("(prefers-reduced-motion: reduce)").matches
        ? "auto"
        : "smooth",
      block: "start",
    });
    element
      .querySelector<HTMLButtonElement>(".workflow-stage-toggle")
      ?.focus({ preventScroll: true });
    setStageNavigation(undefined);
  }, [stageNavigation, activeData, stepOffset, open]);
  function selectStage(key: string, stepCount: number) {
    setOpen((previous) => ({ ...previous, [`stage:${key}`]: true }));
    setStageNavigation({ key, hasSteps: stepCount > 0, visitedPages: [] });
  }
  const draftKey = (executionId: string) =>
    `${projectId}:${record.id}:${executionId}`;
  function baseline(execution?: Row): ActionBaseline {
    return {
      target: current.id,
      revision: current.revision,
      planRevision: runtime.active_plan_revision_id,
      controlVersion: runtime.control_version,
      stepRevision: execution
        ? activeData?.step_revisions?.[execution.id]
        : undefined,
      execution: execution
        ? {
            id: execution.id,
            step: execution.step,
            input_digest: execution.input_digest,
            outputs: execution.outputs,
          }
        : undefined,
    };
  }
  function beginDraft(execution: Row) {
    const key = draftKey(execution.id);
    if (!drafts.current.has(key)) drafts.current.set(key, baseline(execution));
  }
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
      activeData &&
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
    if (!activeData) return;
    const focus = `${record.id}:${target.instance_id}:${target.execution_id}`;
    if (focusedTarget.current === focus) return;
    const selected = activeData.runtime.steps.find(
      (step: Row) => step.id === target.execution_id,
    );
    const next: Record<string, boolean> = {};
    if (selected) {
      const definition = activeData.plan?.steps?.find(
        (step: Row) => step.key === selected.step,
      );
      if (definition) next[`stage:${definition.stage}`] = true;
      next[`step:${selected.id}`] = true;
    } else if (target.instance_id && target.instance_id !== record.id) {
      for (const child of (activeData.children || []).filter(
        (child: Row) => child.id === activeData.focus?.child_id,
      )) {
        const parent = activeData.runtime.steps.find(
          (step: Row) => step.id === child.data.parent_execution_id,
        );
        const definition = activeData.plan?.steps?.find(
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
  }, [target.execution_id, target.instance_id, record.id, activeData]);
  useEffect(() => {
    if (!target.execution_id || !open[`step:${target.execution_id}`]) return;
    const element = document.getElementById(
      `workflow-execution-${target.execution_id}`,
    );
    if (!element) return;
    element.scrollIntoView({ block: "center" });
    element
      .querySelector<HTMLButtonElement>(".workflow-step-toggle")
      ?.focus({ preventScroll: true });
  }, [target.execution_id, open]);
  async function act(method: string, payload: Row = {}, execution?: Row) {
    capture();
    beginFeedback();
    setBusy(true);
    setError("");
    try {
      const executionId = execution?.id || payload.execution_id || "";
      const original =
        drafts.current.get(draftKey(executionId)) || baseline(execution);
      const originalPayload = { ...payload };
      if (original.execution) {
        for (const key of ["step", "execution_id", "input_digest", "outputs"])
          if (key in originalPayload)
            originalPayload[key] =
              original.execution[key === "execution_id" ? "id" : key];
      }
      const params = {
        project_id: projectId,
        target: original.target,
        expected_revision: original.revision,
        payload: {
          ...originalPayload,
          plan_revision_id: original.planRevision,
          control_version: original.controlVersion,
          ...(original.stepRevision !== undefined
            ? { expected_step_revision: original.stepRevision }
            : {}),
        },
      };
      const key = `${draftKey(executionId)}:${method}`;
      const fingerprint = JSON.stringify(params);
      if (attempts.current.get(key)?.fingerprint !== fingerprint)
        attempts.current.set(key, { fingerprint, id: crypto.randomUUID() });
      await request(method, {
        ...params,
        command_id: attempts.current.get(key)!.id,
      });
      attempts.current.delete(key);
      drafts.current.delete(draftKey(executionId));
      setReasons((previous) => {
        const next = { ...previous };
        delete next[executionId];
        return next;
      });
      await load();
      await refresh();
    } catch (error) {
      setError(String(error));
    } finally {
      setBusy(false);
      endFeedback();
    }
  }
  const toggle = (key: string, defaultOpen = false) =>
    setOpen((previous) => ({
      ...previous,
      [key]: !(previous[key] ?? defaultOpen),
    }));
  if (!activeData)
    return (
      <section className="panel" aria-label="当前工作流">
        <p role="status">正在读取步骤与操作…</p>
        {error ? <p role="alert">{error}</p> : null}
      </section>
    );
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
              void load(true).catch((error) => setError(String(error)))
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
      {!nested && (
        <WorkflowStageProgress
          request={request}
          record={current}
          projectId={projectId}
          onStageSelect={selectStage}
        />
      )}
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
        const stageOpen = open[stageKey] ?? true;
        return (
          <section
            id={`workflow-stage-${record.id}-${stage.key}`}
            ref={(element) => {
              if (element) stageElements.current.set(stage.key, element);
              else stageElements.current.delete(stage.key);
            }}
            className={`workflow-stage-row status-${status.key}`}
            key={stage.key}
          >
            <ActionButton
              type="button"
              className="workflow-stage-toggle"
              aria-expanded={stageOpen}
              onClick={() => toggle(stageKey, true)}
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
                  const completion =
                    activeData?.completion_status?.[execution.id];
                  const unmet = completion?.satisfied === false;
                  const confirmation = completion?.confirmation;
                  const humanSatisfied =
                    completion?.human_satisfied ??
                    (definition.confirmation !== "human" || !!confirmation);
                  const stepKey = `step:${execution.id}`;
                  const stepOpen = !!open[stepKey];
                  const children = (activeData?.children || []).filter(
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
                          activeData?.actors?.[execution.id]?.name ||
                            definition.executor?.type ||
                            "执行方式未记录",
                        )}{" "}
                        · 第 {displayText(execution.round)} 轮
                        {displayText(
                          execution.reason ? ` · ${execution.reason}` : "",
                        )}
                      </p>
                      {execution.reconciliation && (
                        <p
                          role={
                            ["unresolved", "conflicted"].includes(
                              execution.reconciliation.status,
                            )
                              ? "alert"
                              : "status"
                          }
                          className="workflow-step-summary"
                        >
                          {["unresolved", "conflicted"].includes(
                            execution.reconciliation.status,
                          )
                            ? `核对未完成：${execution.reconciliation.error?.message || "未取得可确认的执行结果"}${execution.reconciliation.error?.code ? `（${execution.reconciliation.error.code}）` : ""}`
                            : execution.reconciliation.status === "completed"
                              ? "原执行结果核对已完成"
                              : "正在核对原执行结果…"}
                        </p>
                      )}
                      {stepOpen ? (
                        <div className="workflow-step-body">
                          {definition.confirmation === "human" &&
                            completion &&
                            execution.status === "waiting_confirmation" &&
                            !humanSatisfied && (
                              <div role="status">
                                <p>
                                  {completion.pending
                                    ? "正在汇总完成条件，可先查看产物。"
                                    : unmet
                                      ? "本步骤验收未通过，需要先返工。"
                                      : "本步骤验收已通过，请核对产物并确认此版本，后续步骤将等待你的确认。"}
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
                            <WorkflowStepInput
                              key={`${projectId}:${record.id}:${execution.id}`}
                              request={request}
                              projectId={projectId}
                              item={{
                                instance_id: current.id,
                                revision: current.revision,
                                plan_revision_id:
                                  runtime.active_plan_revision_id,
                                control_version: runtime.control_version,
                                step_revision:
                                  activeData?.step_revisions?.[execution.id],
                                execution_id: execution.id,
                                step: execution.step,
                              }}
                              refresh={async () => {
                                await Promise.all([load(true), refresh()]);
                              }}
                            />
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
                                    onChange={(event) => {
                                      beginDraft(execution);
                                      setReasons((previous) => ({
                                        ...previous,
                                        [execution.id]: event.target.value,
                                      }));
                                    }}
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
                                        accept_result:
                                          completion?.human_can_accept === true,
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
                                  ? confirmation.approval_basis
                                    ? "评审未通过，用户已例外批准"
                                    : "已手动批准，原完成条件未满足"
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
      {stepOffset > 0 || activeData.next_step_offset != null ? (
        <div className="actions">
          <span>
            步骤 {stepOffset + 1}–
            {Math.min(stepOffset + stepPageSize, activeData.step_total)} /{" "}
            {activeData.step_total}
          </span>
          <ActionButton
            disabled={stepOffset === 0}
            onClick={() =>
              setStepOffset(Math.max(0, stepOffset - stepPageSize))
            }
          >
            上一页步骤
          </ActionButton>
          <ActionButton
            disabled={activeData.next_step_offset == null}
            onClick={() => setStepOffset(activeData.next_step_offset)}
          >
            下一页步骤
          </ActionButton>
        </div>
      ) : null}
      {childOffset > 0 || activeData.children_more ? (
        <div className="actions">
          {childOffset > 0 ? (
            <ActionButton
              onClick={() => setChildOffset(Math.max(0, childOffset - 100))}
            >
              上一页子流程
            </ActionButton>
          ) : null}
          {activeData.children_more ? (
            <ActionButton
              onClick={() => setChildOffset(activeData.next_child_offset)}
            >
              下一页子流程
            </ActionButton>
          ) : null}
        </div>
      ) : null}
      {error ? (
        <p role="alert" className="error">
          {displayText(error)}
        </p>
      ) : null}
    </section>
  );
}

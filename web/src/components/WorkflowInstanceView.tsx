import {
  ActionButton,
  ActionForm,
  ActionSelect,
  useActionFeedback,
} from "./ActionFeedback";
import { useEffect, useRef, useState } from "react";
import { MarkdownPreview } from "./configuration-ui";
type Row = Record<string, any>;
type Props = {
  nested?: boolean;
  request: (method: string, params?: Row) => Promise<any>;
  record: Row;
  projectId: string;
  refresh: () => Promise<void>;
  onDiscussion: (id: string, owner: string) => void;
};
const stateLabels: Record<string, string> = {
  pending: "等待前置",
  waiting: "等待子流程",
  waiting_dependencies: "等待前置",
  waiting_condition: "等待条件",
  active: "执行中",
  completed: "已完成",
  skipped: "已跳过",
  waiting_input: "等待讨论 / 采纳",
  waiting_confirmation: "等待人确认",
  blocked: "已阻塞",
  failed: "失败",
  cancelled: "已取消",
};
function stageRows(plan: Row | undefined, steps: Row[]) {
  const definitions = new Map<string, Row>(
    (plan?.steps || []).map((step: Row) => [step.key, step]),
  );
  const stages: Row[] = plan?.stages || [];
  const parents = new Map<string, string>();
  for (const step of plan?.steps || []) {
    const guard = (step.parameters?.call_guards || [])
      .map((key: string) => definitions.get(key))
      .find(
        (entry: Row | undefined) => entry?.stage && entry.stage !== step.stage,
      );
    if (guard && !parents.has(step.stage)) parents.set(step.stage, guard.stage);
  }
  const root = (key: string) => {
    const seen = new Set<string>();
    while (parents.has(key) && !seen.has(key)) {
      seen.add(key);
      key = parents.get(key)!;
    }
    return key;
  };
  const rows = new Map<string, Row>();
  for (const stage of stages.filter((stage) => !parents.has(stage.key)))
    rows.set(stage.key, { ...stage, steps: [] });
  for (const step of steps) {
    const key = root(definitions.get(step.key)?.stage || "workflow");
    if (!rows.has(key))
      rows.set(key, {
        ...(stages.find((stage) => stage.key === key) || {
          key,
          name: "流程阶段",
        }),
        steps: [],
      });
    rows.get(key)!.steps.push(step);
  }
  return [...rows.values()];
}
function stageStatus(steps: Row[]) {
  if (!steps.length) return "pending";
  if (steps.every((step) => ["completed", "skipped"].includes(step.status)))
    return steps.every((step) => step.status === "skipped")
      ? "skipped"
      : "completed";
  return (
    [
      "failed",
      "blocked",
      "waiting_confirmation",
      "waiting_input",
      "active",
      "waiting_dependencies",
      "waiting_condition",
    ].find((status) => steps.some((step) => step.status === status)) ||
    "pending"
  );
}
export function WorkflowInstanceView({
  request,
  record,
  nested = false,
  projectId,
  refresh,
  onDiscussion,
}: Props) {
  const {
    error,
    setError,
    capture: captureFeedback,
    beginFeedback,
    endFeedback,
  } = useActionFeedback();

  const [data, setData] = useState<Row>(),
    [busy, setBusy] = useState(false),
    [selected, setSelected] = useState("");
  const [overwriteReasons, setOverwriteReasons] = useState<
    Record<string, string>
  >({});
  const storageKey = `devdesk:runtime-stages:${projectId}:${record.id}`;
  const [expanded, setExpanded] = useState<Record<string, boolean>>(() => {
    try {
      const value = JSON.parse(localStorage.getItem(storageKey) || "{}");
      return Object.fromEntries(
        Object.entries(value || {}).filter(
          ([, flag]) => typeof flag === "boolean",
        ),
      ) as Record<string, boolean>;
    } catch {
      return {};
    }
  });
  useEffect(() => {
    try {
      localStorage.setItem(storageKey, JSON.stringify(expanded));
    } catch {}
  }, [storageKey, expanded]);
  const generation = useRef(0);
  useEffect(() => {
    const g = ++generation.current;
    let pending = false;
    const load = async () => {
      if (pending) return;
      pending = true;
      try {
        const value = await request("query.workflow_instance", {
          id: record.id,
          project_id: projectId,
        });
        if (g === generation.current)
          setData((old) =>
            old?.record.revision === value.record.revision &&
            JSON.stringify(
              (old?.children || []).map((child: Row) => [
                child.record.id,
                child.record.revision,
              ]),
            ) ===
              JSON.stringify(
                (value.children || []).map((child: Row) => [
                  child.record.id,
                  child.record.revision,
                ]),
              )
              ? old
              : value,
          );
      } catch (e) {
        if (g === generation.current) setError(String(e));
      } finally {
        pending = false;
      }
    };
    void load();
    const timer =
      nested || record.data.task_members?.length
        ? window.setInterval(load, 1000)
        : undefined;
    return () => {
      ++generation.current;
      if (timer) window.clearInterval(timer);
    };
  }, [request, record.id, record.revision, projectId, nested]);
  async function command(
    method: string,
    target: string,
    revision: number,
    payload: Row = {},
  ) {
    captureFeedback();

    (beginFeedback(), setBusy(true));
    setError("");
    try {
      await request(method, {
        project_id: projectId,
        command_id: crypto.randomUUID(),
        target,
        expected_revision: revision,
        payload,
      });
      await refresh();
    } catch (e) {
      setError(String(e));
    } finally {
      (endFeedback(), setBusy(false));
    }
  }
  const currentRecord = data?.record || record;
  const Heading = nested ? "h3" : "h2";
  const runtime = data?.record.data,
    active = runtime?.steps.find((s: Row) =>
      [
        "failed",
        "blocked",
        "waiting_confirmation",
        "waiting_input",
        "active",
      ].includes(s.status),
    );
  const renderStep = (step: Row, stage: Row) => {
    const definition = data?.plan.steps.find((s: Row) => s.key === step.key);
    const action = data?.actions.find((a: Row) => a.id === step.id);
    const approvals: Row[] = action?.model_approvals || [];
    const modelRejected = approvals.some((report) => !report.approve);
    return (
      <article key={step.id} className={`workflow-node status-${step.status}`}>
        <ActionButton
          aria-expanded={selected === step.id}
          onClick={() => {
            captureFeedback();
            return setSelected(selected === step.id ? "" : step.id);
          }}
        >
          {definition?.stage !== stage.key && (
            <small className="workflow-subflow-label">
              子流程 ·{" "}
              {data?.plan.stages
                .find((source: Row) => source.key === definition?.stage)
                ?.name.split(" / ")
                .at(-1)}
            </small>
          )}
          <strong>{definition?.name || step.key}</strong>
          <span>
            {stateLabels[step.status] || step.status} · 第 {step.round} 轮
          </span>
        </ActionButton>
        {step.reason && <p>{step.reason}</p>}
        {approvals.length > 0 && (
          <section aria-label="内容校验与 Approve">
            <h4>模型 {modelRejected ? "Reject" : "Approve"}</h4>
            {approvals.map((report) => (
              <details
                key={report.run_id}
                open={step.status === "waiting_confirmation"}
              >
                <summary>
                  {report.node} · {report.approve ? "Approve" : "Reject"}
                </summary>
                <p>{report.summary}</p>
                <ul>
                  {report.checks.map((check: Row) => (
                    <li key={check.key}>
                      <strong>
                        {check.key} · {check.passed ? "通过" : "未通过"}
                      </strong>
                      <p>{check.evidence}</p>
                    </li>
                  ))}
                </ul>
                <p className="muted">Skill 修订：{report.skills.join("、")}</p>
              </details>
            ))}
            {step.confirmation?.human_approve && (
              <p>
                人类已 Approve
                {step.confirmation.overwrite_model
                  ? `，覆盖理由：${step.confirmation.reason}`
                  : ""}
              </p>
            )}
            {modelRejected && step.status === "waiting_confirmation" && (
              <label>
                覆盖模型结论的理由
                <textarea
                  value={overwriteReasons[step.id] || ""}
                  onChange={(event) => {
                    const value = event.target.value;
                    setOverwriteReasons((old) => ({
                      ...old,
                      [step.id]: value,
                    }));
                  }}
                />
              </label>
            )}
          </section>
        )}
        {step.outputs.session && (
          <ActionButton
            onClick={() => {
              captureFeedback();
              return onDiscussion(step.outputs.session, runtime.owner_id);
            }}
          >
            继续讨论 / 查看提案
          </ActionButton>
        )}
        {(data?.actions.find((a: Row) => a.id === step.id)?.allowed || []).map(
          (method: string) => (
            <ActionButton
              key={method}
              disabled={
                busy ||
                runtime.paused ||
                (method === "workflow.step.confirm" &&
                  modelRejected &&
                  !overwriteReasons[step.id]?.trim())
              }
              onClick={() => {
                captureFeedback();
                return command(
                  method,
                  step.id,
                  step.revision,
                  method === "workflow.step.confirm"
                    ? {
                        input_hash: step.input_hash,
                        ...(modelRejected
                          ? {
                              overwrite_model: true,
                              reason: overwriteReasons[step.id],
                            }
                          : {}),
                      }
                    : method === "workflow.step.rework"
                      ? {
                          from_step:
                            data?.actions.find((a: Row) => a.id === step.id)
                              ?.rework_entries?.[0] || step.key,
                        }
                      : {},
                );
              }}
            >
              {method.endsWith("confirm")
                ? approvals.length
                  ? modelRejected
                    ? "覆盖模型结论并 Approve"
                    : "人类 Approve"
                  : "确认结果后继续"
                : method.endsWith("retry")
                  ? "技术重试"
                  : `从${data?.plan.steps.find((definition: Row) => definition.key === data?.actions.find((action: Row) => action.id === step.id)?.rework_entries?.[0])?.name || "本步骤"}返工`}
            </ActionButton>
          ),
        )}
        {selected === step.id && (
          <>
            <p>
              职责 {definition?.participant_slot || "协调器"} ·{" "}
              {approvals.length
                ? "模型给出内容判断后，等待人类 Approve"
                : definition?.advance_mode === "automatic"
                  ? "自动推进"
                  : "结果完成后等待人确认"}
            </p>
            <p>{definition?.interaction.goal}</p>
            <details>
              <summary>该轮输入、证据和输出引用</summary>
              <pre>{JSON.stringify(step, null, 2)}</pre>
            </details>
          </>
        )}
      </article>
    );
  };
  const stages = stageRows(data?.plan, runtime?.steps || []);
  const activeStage =
    stages.find((stage) =>
      stage.steps.some((step: Row) => step.id === active?.id),
    )?.key || stages.find((stage) => stage.steps.length)?.key;
  const toggle = (key: string, initial = false) =>
    setExpanded((previous) => ({
      ...previous,
      [key]: !(previous[key] ?? initial),
    }));
  return (
    <section
      className={`panel workflow-instance${nested ? " workflow-instance-child" : ""}`}
    >
      <div className="panel-heading">
        <div>
          <Heading>{nested ? "子流程进度" : "当前工作流"}</Heading>
          <p>
            {active
              ? `${data?.plan.steps.find((s: Row) => s.key === active.key)?.name} · ${stateLabels[active.status]}`
              : stateLabels[currentRecord.data.status] ||
                currentRecord.data.status}
          </p>
        </div>
        {!["completed", "cancelled"].includes(currentRecord.data.status) && (
          <ActionButton
            disabled={busy}
            onClick={() => {
              captureFeedback();
              return command(
                currentRecord.data.paused
                  ? "workflow.resume"
                  : "workflow.pause",
                currentRecord.id,
                currentRecord.revision,
              );
            }}
          >
            {currentRecord.data.paused ? "恢复流程" : "暂停流程"}
          </ActionButton>
        )}
      </div>
      {!data && !error && <p role="status">正在读取流程与配置锁…</p>}
      <div className="workflow-runtime workflow-runtime-stages">
        {stages.map((stage, index) => {
          const status = stageStatus(stage.steps);
          const open = expanded[stage.key] ?? stage.key === activeStage;
          const panelId = `stage-${record.id}-${encodeURIComponent(stage.key)}`;
          const children = (data?.children || []).filter((child: Row) =>
            stage.steps.some((step: Row) =>
              child.parent_step_id
                ? step.id === child.parent_step_id
                : data?.plan.steps.find(
                    (definition: Row) => definition.key === step.key,
                  )?.kind === "tasks.foreach",
            ),
          );
          return (
            <section
              key={stage.key}
              className={`workflow-runtime-stage status-${status}`}
              aria-label={`阶段 ${stage.name}`}
            >
              <ActionButton
                type="button"
                className="workflow-runtime-stage-toggle"
                disabled={busy}
                aria-label={`阶段 ${stage.name}`}
                aria-expanded={open}
                aria-controls={panelId}
                aria-describedby={`${panelId}-status`}
                onClick={() => toggle(stage.key, stage.key === activeStage)}
              >
                <strong>
                  {index + 1}. {stage.name}
                </strong>
                <span id={`${panelId}-status`}>
                  {stateLabels[status]} ·{" "}
                  {
                    stage.steps.filter((step: Row) =>
                      ["completed", "skipped"].includes(step.status),
                    ).length
                  }
                  /{stage.steps.length} 步骤 · {open ? "收起" : "展开"}
                </span>
              </ActionButton>
              {open && (
                <div className="workflow-runtime-stage-body" id={panelId}>
                  {stage.description && (
                    <details className="workflow-runtime-stage-description">
                      <summary>阶段说明</summary>
                      <MarkdownPreview text={stage.description} />
                    </details>
                  )}
                  {stage.steps.length ? (
                    <div className="workflow-stage-nodes">
                      {stage.steps.map((step: Row) => renderStep(step, stage))}
                    </div>
                  ) : (
                    <p className="muted">此阶段尚无步骤。</p>
                  )}
                  {children.map((child: Row) => (
                    <div
                      className="workflow-inline-subflow"
                      key={child.record.id}
                    >
                      <ActionButton
                        type="button"
                        className="workflow-child-toggle"
                        aria-expanded={
                          expanded[`child:${child.record.id}`] || false
                        }
                        onClick={() => toggle(`child:${child.record.id}`)}
                      >
                        <strong>子流程 · {child.title}</strong>
                        <span>
                          {stateLabels[child.record.data.status] ||
                            child.record.data.status}{" "}
                          ·{" "}
                          {expanded[`child:${child.record.id}`]
                            ? "收起"
                            : "展开"}
                        </span>
                      </ActionButton>
                      {expanded[`child:${child.record.id}`] && (
                        <WorkflowInstanceView
                          nested
                          record={child.record}
                          request={request}
                          projectId={projectId}
                          refresh={refresh}
                          onDiscussion={onDiscussion}
                        />
                      )}
                    </div>
                  ))}
                </div>
              )}
            </section>
          );
        })}
      </div>
      <details>
        <summary>Feature 冻结配置（只读）</summary>
        <p>
          Workflow {data?.configuration.workflow_ref} · Standard{" "}
          {data?.configuration.standard_ref} · 来源 Project 配置{" "}
          {data?.configuration.project_configuration_id}
        </p>
        <pre>{JSON.stringify(data?.configuration, null, 2)}</pre>
      </details>
      {runtime?.task_members.length > 0 && (
        <p>
          TaskGroup generation {runtime.generation} · 固定{" "}
          {runtime.task_members.length} 个成员
        </p>
      )}
      {error && (
        <p className="error" role="alert">
          {error}
        </p>
      )}
    </section>
  );
}

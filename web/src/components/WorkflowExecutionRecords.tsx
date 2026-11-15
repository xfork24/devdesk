import { useState } from "react";
import { type Request, type Row } from "./configuration-ui";
import {
  DiscussionLink,
  RawJsonLink,
  ReadableValue,
  ReadingLink,
  ReadState,
  useReading,
  navigateReading,
  executionUrl,
} from "./reading";
const labels: Record<string, string> = {
  completed: "已完成",
  running: "执行中",
  queued: "等待前置",
  failed: "失败",
  cancelled: "已取消",
  awaiting_acceptance: "待接纳",
  waiting_confirmation: "待人工确认",
  needs_reconciliation: "待核对",
  skipped: "已跳过",
};
function ExecutionReader({
  projectId,
  ownerId,
  request,
  executionId,
  instanceId,
}: {
  projectId: string;
  ownerId: string;
  request: Request;
  executionId: string;
  instanceId?: string;
}) {
  const state = useReading(request, "query.workflow_executions", {
    project_id: projectId,
    owner_id: ownerId,
    execution_id: executionId,
    instance_id: instanceId || undefined,
  });
  const value = state.value,
    record = value?.record;
  return (
    <article className="reading-body" aria-label="选中执行记录">
      <ReadState {...state} />
      {value && record && (
        <>
          <h3>
            {record.step_name} · 第 {record.round} 轮
            {record.historical ? " · 历史执行" : ""}
          </h3>
          <h4>执行身份与结果</h4>
          <p>
            {record.owner_title} · {record.instance_name}
          </p>
          <p>执行者/方式：{record.actor.name}</p>
          <p>状态：{labels[record.status] || record.status}</p>
          <ReadableValue value={record.reason || "未记录额外说明"} />
          <p className="muted">
            开始：
            {record.started
              ? new Date(record.started).toLocaleString("zh-CN", {
                  hour12: false,
                })
              : "未记录"}{" "}
            · 结束：
            {record.finished
              ? new Date(record.finished).toLocaleString("zh-CN", {
                  hour12: false,
                })
              : "未记录"}
          </p>
          <div className="reading-links">
            {record.turn_id && (
              <ReadingLink
                detail="context"
                projectId={projectId}
                ownerId={ownerId}
                target={{ execution_id: record.id, turn_id: record.turn_id }}
              >
                查看执行者上下文
              </ReadingLink>
            )}
            {record.session_id && (
              <DiscussionLink
                projectId={projectId}
                ownerId={ownerId}
                sessionId={record.session_id}
                taskId={
                  record.owner_id !== ownerId && record.owner_id !== projectId
                    ? record.owner_id
                    : undefined
                }
              >
                查看会话
              </DiscussionLink>
            )}
          </div>
          {record.outputs.length > 0 && (
            <section>
              <h4>关联交付物</h4>
              <ul>
                {record.outputs.map((o: Row) => (
                  <li key={o.id}>
                    <ReadingLink
                      detail="documents"
                      projectId={projectId}
                      ownerId={ownerId}
                      target={{ output_id: o.id }}
                    >
                      {o.name || o.port}
                    </ReadingLink>
                  </li>
                ))}
              </ul>
            </section>
          )}
          <h4>关键操作、决定与回执</h4>
          {!value.facts.length && (
            <p className="muted">没有额外记录；不能据此补充推断执行经过。</p>
          )}
          {value.facts.map((fact: Row) => (
            <section key={fact.id} className="execution-fact">
              <h4>{fact.name}</h4>
              <p className="muted">
                {fact.type} ·{" "}
                {new Date(fact.created_at).toLocaleString("zh-CN", {
                  hour12: false,
                })}
              </p>
              {fact.type === "workflow_decision" ||
              fact.type === "workflow_confirmation" ? (
                <ReadableValue
                  value={{
                    operation: fact.data.operation,
                    actor: fact.data.actor,
                    reason: fact.data.reason,
                    override_completion: fact.data.override_completion,
                  }}
                />
              ) : (
                <>
                  <p>记录状态：{fact.data.status || "已记录"}</p>
                  {fact.data.error && <ReadableValue value={fact.data.error} />}
                  <details>
                    <summary>核对回执字段</summary>
                    <ReadableValue value={fact.data.receipt || fact.data} />
                  </details>
                </>
              )}
              <RawJsonLink
                source="record"
                params={{
                  project_id: projectId,
                  owner_id: ownerId,
                  record_id: fact.id,
                }}
              />
            </section>
          ))}
          <details>
            <summary>执行来源与冻结引用</summary>
            <ReadableValue
              value={{
                execution_id: value.execution.id,
                plan_revision_id: value.execution.plan_revision_id,
                input_digest: value.execution.input_digest,
                agent: record.actor,
                inputs:
                  value.execution.inputs && Object.keys(value.execution.inputs),
                outputs: value.execution.outputs,
              }}
            />
          </details>
          <RawJsonLink
            source="execution"
            params={{
              project_id: projectId,
              owner_id: ownerId,
              execution_id: executionId,
              instance_id: record.instance_id,
            }}
          />
        </>
      )}
    </article>
  );
}
export function WorkflowExecutionRecords({
  projectId,
  ownerId,
  request,
  target = {},
}: {
  projectId: string;
  ownerId: string;
  request: Request;
  target?: Row;
}) {
  const state = useReading(request, "query.workflow_executions", {
    project_id: projectId,
    owner_id: ownerId,
  });
  const [showHistory, setShowHistory] = useState(false);
  const records: Row[] = state.value?.records || [];
  const hasTarget = Boolean(target.execution_id);
  const selected =
    records.find((r) => r.id === target.execution_id) ||
    (!hasTarget
      ? records.find(
          (r) =>
            !r.historical &&
            [
              "running",
              "failed",
              "needs_reconciliation",
              "waiting_confirmation",
            ].includes(r.status),
        ) || records.find((r) => !r.historical)
      : undefined);
  const instances = [...new Set(records.map((r) => r.instance_id))];
  return (
    <section aria-label="工作流执行记录">
      <h3>执行记录</h3>
      <p className="muted">选择一次执行，核对实际参与者、决定和回执。</p>
      <ReadState {...state} />
      <label>
        <input
          type="checkbox"
          checked={showHistory}
          onChange={(e) => setShowHistory(e.target.checked)}
        />
        显示历史执行
      </label>
      <div className="reading-layout">
        <nav className="reading-directory" aria-label="执行轮次目录">
          {instances.map((id) => (
            <section key={id}>
              <h4>
                {records.find((r) => r.instance_id === id)?.owner_title} ·{" "}
                {records.find((r) => r.instance_id === id)?.instance_name}
              </h4>
              {records
                .filter(
                  (r) =>
                    r.instance_id === id &&
                    (showHistory || !r.historical || r.id === selected?.id),
                )
                .map((r) => (
                  <button
                    key={r.id}
                    aria-current={selected?.id === r.id}
                    onClick={() =>
                      navigateReading(
                        executionUrl("records", projectId, ownerId, {
                          instance_id: r.instance_id,
                          execution_id: r.id,
                        }),
                      )
                    }
                  >
                    {r.step_name}
                    <small>
                      第 {r.round} 轮 · {r.historical ? "历史 · " : ""}
                      {labels[r.status] || r.status}
                    </small>
                  </button>
                ))}
            </section>
          ))}
        </nav>
        {selected ? (
          <ExecutionReader
            key={`${projectId}:${ownerId}:${selected.id}`}
            {...{ projectId, ownerId, request }}
            executionId={selected.id}
            instanceId={selected.instance_id}
          />
        ) : (
          <p className="muted">
            {state.loading
              ? "正在读取执行目录…"
              : hasTarget
                ? "目标执行不属于当前工作范围或尚不可读。"
                : "当前工作流程没有可读取的执行记录。"}
          </p>
        )}
      </div>
    </section>
  );
}

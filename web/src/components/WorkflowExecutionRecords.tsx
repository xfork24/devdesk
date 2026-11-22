import { displayText } from "../display-text";
import { useEffect, useState } from "react";
import { type Request, type Row } from "./rpc-types";
import { RawJsonLink, ReadableValue, ReadState } from "./reading";
import { useReading, navigateReading, executionUrl } from "./reading-state";
import { WorkflowReferences } from "./WorkflowReferences";
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
  const [factOffset, setFactOffset] = useState(0);
  const state = useReading(request, "query.workflow_executions", {
    project_id: projectId,
    owner_id: ownerId,
    execution_id: executionId,
    instance_id: instanceId || undefined,
    fact_offset: factOffset,
  });
  const value = state.value,
    record = value?.record;
  return (
    <article className="reading-body" aria-label="选中执行记录">
      <ReadState {...state} />
      {value && record && (
        <>
          <h3>
            {displayText(record.step_name)} · 第 {displayText(record.round)} 轮
          </h3>
          <h4>执行身份与结果</h4>
          <p>
            {displayText(record.owner_title)} ·{" "}
            {displayText(record.instance_name)}
          </p>
          <p>执行者/方式：{displayText(record.actor.name)}</p>
          <p>状态：{displayText(labels[record.status] || record.status)}</p>
          <ReadableValue value={record.reason || "未记录额外说明"} />
          <p className="muted">
            开始：
            {displayText(
              record.started
                ? new Date(record.started).toLocaleString("zh-CN", {
                    hour12: false,
                  })
                : "未记录",
            )}{" "}
            · 结束：
            {displayText(
              record.finished
                ? new Date(record.finished).toLocaleString("zh-CN", {
                    hour12: false,
                  })
                : "未记录",
            )}
          </p>
          <WorkflowReferences
            source={{ ...record, execution_id: record.id }}
            {...{ projectId, ownerId }}
            current="records"
          />
          <h4>关键操作、决定与回执</h4>
          {!value.facts.length && (
            <p className="muted">没有额外记录；不能据此补充推断执行经过。</p>
          )}
          {value.facts.map((fact: Row) => (
            <section key={fact.id} className="execution-fact">
              <h4>{displayText(fact.name)}</h4>
              <p className="muted">
                {displayText(fact.type)} ·{" "}
                {displayText(
                  new Date(fact.created_at).toLocaleString("zh-CN", {
                    hour12: false,
                  }),
                )}
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
                  <p>记录状态：{displayText(fact.data.status || "已记录")}</p>
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
          {factOffset > 0 || value.facts_more ? (
            <div className="actions">
              {factOffset > 0 ? (
                <button
                  onClick={() => setFactOffset(Math.max(0, factOffset - 100))}
                >
                  上一页决定与回执
                </button>
              ) : null}
              {value.facts_more ? (
                <button onClick={() => setFactOffset(value.next_fact_offset)}>
                  下一页决定与回执
                </button>
              ) : null}
            </div>
          ) : null}
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
  const [offset, setOffset] = useState(0);
  useEffect(() => setOffset(0), [projectId, ownerId]);
  const state = useReading(request, "query.workflow_executions", {
    project_id: projectId,
    owner_id: ownerId,
    offset,
    limit: 100,
  });
  const records: Row[] = (state.value?.records || []).filter(
    (record: Row) => record.execution_available ?? record.status !== "queued",
  );
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
        ) ||
        records.find((r) => !r.historical) ||
        records[0]
      : undefined);
  const instances = [...new Set(records.map((r) => r.instance_id))];
  return (
    <section aria-label="工作流执行记录">
      <h3>执行记录</h3>
      <p className="muted">
        审计视角：核对步骤的派发、人工决定、确认与工具回执。冻结输入在上下文页，会话消息在角色讨论页。
      </p>
      <ReadState {...state} />
      <div className="reading-layout">
        <nav className="reading-directory" aria-label="执行轮次目录">
          {instances.map((id) => (
            <section key={id}>
              <h4>
                {displayText(
                  records.find((r) => r.instance_id === id)?.owner_title,
                )}{" "}
                ·{" "}
                {displayText(
                  records.find((r) => r.instance_id === id)?.instance_name,
                )}
              </h4>
              {records
                .filter((r) => r.instance_id === id)
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
                    {displayText(r.step_name)}
                    <small>
                      第 {displayText(r.round)} 轮 ·{" "}
                      {displayText(labels[r.status] || r.status)}
                    </small>
                  </button>
                ))}
            </section>
          ))}
        </nav>
        {selected || target.execution_id ? (
          <ExecutionReader
            key={`${projectId}:${ownerId}:${target.execution_id || selected?.id}`}
            {...{ projectId, ownerId, request }}
            executionId={target.execution_id || selected!.id}
            instanceId={target.instance_id || selected?.instance_id}
          />
        ) : (
          <p className="muted">
            {displayText(
              state.loading
                ? "正在读取执行目录…"
                : hasTarget
                  ? "目标执行不属于当前工作范围或尚不可读。"
                  : "当前工作流程没有可读取的执行记录。",
            )}
          </p>
        )}
      </div>
      {offset > 0 || state.value?.more ? (
        <div className="actions">
          {offset > 0 ? (
            <button onClick={() => setOffset(Math.max(0, offset - 100))}>
              上一页执行
            </button>
          ) : null}
          {state.value?.more ? (
            <button onClick={() => setOffset(state.value!.next_offset)}>
              下一页执行
            </button>
          ) : null}
        </div>
      ) : null}
    </section>
  );
}

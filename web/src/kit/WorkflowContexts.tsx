import { displayText } from "../sdk/display-text";
import { type Request, type Row } from "../sdk/rpc-types";
import { AgentTaskView } from "./WorkflowTaskPreview";
import { RawJsonLink, ReadState, ReadableValue } from "./reading";
import {
  useReading,
  navigateReading,
  executionUrl,
  readingKeys,
} from "./reading-state";
import { useState } from "react";
import { WorkflowReferences } from "./WorkflowReferences";

function ManualContext({
  context,
  projectId,
  ownerId,
  request,
  onEdit,
}: {
  context: Row;
  projectId: string;
  ownerId: string;
  request: Request;
  onEdit?: (context: Row) => void;
}) {
  const [open, setOpen] = useState(!context.metadata_only);
  const state = useReading(
    request,
    "query.workflow_contexts",
    { project_id: projectId, owner_id: ownerId, context_id: context.id },
    Boolean(context.metadata_only && open),
    context.revision,
  );
  const current = context.metadata_only ? state.value?.context : context;
  return (
    <section data-context-id={context.id}>
      <details
        open={open}
        onToggle={(event) => setOpen(event.currentTarget.open)}
      >
        <summary>工作背景 · 版本 {context.revision}</summary>
        <ReadState {...state} />
        {current && (
          <>
            <ReadableValue value={current.content} />
            <RawJsonLink
              source="context"
              params={{
                project_id: projectId,
                owner_id: ownerId,
                context_id: current.id,
              }}
            />
            {onEdit && (
              <button onClick={() => onEdit(current)}>编辑上下文</button>
            )}
          </>
        )}
      </details>
    </section>
  );
}

function FrozenContext({
  entry,
  projectId,
  ownerId,
  request,
}: {
  entry: Row;
  projectId: string;
  ownerId: string;
  request: Request;
}) {
  const stepExecution = entry.kind !== "controller" && entry.execution_id;
  const state = useReading(
    request,
    "query.execution_task",
    { project_id: projectId, turn_id: entry.turn_id },
    Boolean(entry.task_available && entry.turn_id),
  );
  const execution = useReading(
    request,
    "query.workflow_executions",
    {
      project_id: projectId,
      owner_id: ownerId,
      instance_id: entry.instance_id,
      execution_id: entry.execution_id,
    },
    Boolean(stepExecution && !entry.task_available),
  );
  return (
    <>
      <ReadState {...state} />
      {state.value && (
        <AgentTaskView
          value={state.value}
          projectId={projectId}
          ownerId={ownerId}
        />
      )}
      {!entry.task_available && (
        <>
          <ReadState {...execution} />
          {execution.value ? (
            <>
              <h4>步骤执行上下文快照</h4>
              <ReadableValue
                value={{
                  plan_revision_id: execution.value.execution.plan_revision_id,
                  input_digest: execution.value.execution.input_digest,
                  inputs: execution.value.execution.inputs,
                  input_sources: execution.value.execution.input_sources,
                  runtime_context: execution.value.execution.runtime_context,
                }}
              />
            </>
          ) : (
            !stepExecution && (
              <p className="muted">
                项目经理编排轮次的请求与响应见关联会话；它没有步骤冻结任务。
              </p>
            )
          )}
        </>
      )}
    </>
  );
}
function AgentSnapshot({
  entry,
  projectId,
  ownerId,
  request,
}: {
  entry: Row;
  projectId: string;
  ownerId: string;
  request: Request;
}) {
  const [open, setOpen] = useState(false);
  const params = {
    project_id: projectId,
    owner_id: ownerId,
    session_id: entry.session_id,
    snapshot_id: entry.snapshot_id,
  };
  const state = useReading(request, "query.workflow_contexts", params, open);
  return (
    <details onToggle={(e) => setOpen(e.currentTarget.open)}>
      <summary>此会话继承的上下文快照</summary>
      <ReadState {...state} />
      {state.value && (
        <>
          <ReadableValue value={state.value.snapshot} />
          <RawJsonLink source="context" params={params} />
        </>
      )}
    </details>
  );
}
export function WorkflowContexts({
  projectId,
  ownerId,
  request,
  target = {},
  contexts = [],
  onEditContext,
}: {
  projectId: string;
  ownerId: string;
  request: Request;
  target?: Row;
  contexts?: Row[];
  onEditContext?: (context: Row) => void;
}) {
  const [offset, setOffset] = useState(0);
  const [manualOpen, setManualOpen] = useState(false);
  const hasTarget = Boolean(
    target.execution_id ||
    target.turn_id ||
    target.session_id ||
    target.agent_instance_id,
  );
  const state = useReading(request, "query.workflow_contexts", {
    project_id: projectId,
    owner_id: ownerId,
    offset,
    limit: 100,
  });
  const exactState = useReading(
    request,
    "query.workflow_contexts",
    {
      project_id: projectId,
      owner_id: ownerId,
      ...Object.fromEntries(
        readingKeys
          .filter((key) => target[key])
          .map((key) => [key, target[key]]),
      ),
    },
    hasTarget,
  );
  const turns: Row[] = [
    ...new Map<string, Row>(
      [
        ...(state.value?.agents || []),
        ...(exactState.value?.agents || []),
      ].flatMap((agent: Row) =>
        agent.entries.map(
          (entry: Row) =>
            [
              `${entry.session_id}:${entry.turn_id}`,
              { ...entry, actor: entry.actor || agent.actor },
            ] as const,
        ),
      ),
    ).values(),
  ];
  const executions: Row[] = (state.value?.executions || []).filter(
    (execution: Row) =>
      execution.context_available ?? execution.status !== "queued",
  );
  const groups: Row[] = executions.map((execution) => {
    const entries = turns.filter(
      (entry) => entry.execution_id === execution.id,
    );
    return {
      ...execution,
      execution_id: execution.id,
      entries: entries.length
        ? entries
        : [{ ...execution, execution_id: execution.id }],
    };
  });
  for (const turn of turns) {
    if (
      !executions.some(
        (execution) => turn.execution_id && execution.id === turn.execution_id,
      )
    ) {
      const key = turn.execution_id || turn.session_id;
      const group = groups.find((group) => group.key === key);
      if (group) group.entries.push(turn);
      else groups.push({ ...turn, key, entries: [turn] });
    }
  }
  const matches = (entry: Row) =>
    target.turn_id
      ? entry.turn_id === target.turn_id
      : target.execution_id
        ? entry.execution_id === target.execution_id
        : target.session_id
          ? entry.session_id === target.session_id
          : target.agent_instance_id &&
            entry.actor?.agent_instance_id === target.agent_instance_id;
  const selected =
    groups.find((group) => group.entries.some(matches)) ||
    (!hasTarget
      ? groups.find((group) => !group.historical) || groups[0]
      : undefined);
  const entries: Row[] = selected?.entries || [];
  const entry =
    entries.find(matches) ||
    entries.find((entry) => !entry.historical) ||
    entries[0];
  const visible = groups;
  const instances = [
    ...new Set(visible.map((group) => group.instance_id || group.session_id)),
  ];
  const choose = (entry: Row) =>
    navigateReading(
      executionUrl("context", projectId, ownerId, {
        instance_id: entry.instance_id,
        execution_id: entry.execution_id,
        session_id: entry.session_id,
        turn_id: entry.turn_id,
      }),
    );
  return (
    <section aria-label="步骤上下文">
      <div className="panel-heading">
        <div>
          <h3>步骤上下文快照</h3>
          <p className="muted">
            按流程、步骤和执行轮次查看冻结输入。会话与轮次 ID 与角色讨论共用。
          </p>
        </div>
        <button onClick={state.retry} disabled={state.loading}>
          刷新上下文
        </button>
      </div>
      <ReadState {...state} />
      {hasTarget && <ReadState {...exactState} />}
      <div className="reading-layout">
        <nav className="reading-directory" aria-label="流程步骤上下文目录">
          {instances.map((id) => (
            <section key={id}>
              <h4>
                {displayText(
                  visible.find(
                    (group) => (group.instance_id || group.session_id) === id,
                  )?.instance_name || "项目经理编排",
                )}
              </h4>
              {visible
                .filter(
                  (group) => (group.instance_id || group.session_id) === id,
                )
                .map((group) => (
                  <div key={group.execution_id || group.key}>
                    <button
                      aria-current={group === selected}
                      onClick={() =>
                        choose(
                          group.entries.find(
                            (entry: Row) => !entry.historical,
                          ) || group.entries[0],
                        )
                      }
                    >
                      {displayText(group.step_name || "编排控制")}
                      <small>
                        {displayText(group.owner_title)} ·{" "}
                        {displayText(
                          group.round ? `执行第 ${group.round} 轮 · ` : "",
                        )}
                        {displayText(
                          group.entries.filter((entry: Row) => entry.turn_id)
                            .length,
                        )}{" "}
                        个会话轮次
                      </small>
                    </button>
                  </div>
                ))}
            </section>
          ))}
          {state.value && !groups.length && (
            <p>当前范围尚无流程步骤上下文。独立角色讨论不自动成为流程步骤。</p>
          )}
        </nav>
        <article className="reading-body" aria-label="选中步骤上下文">
          {entry ? (
            <>
              <h3>{displayText(entry.step_name || "项目经理编排")}</h3>
              <p>
                {displayText(entry.instance_name)} ·{" "}
                {displayText(entry.actor?.name)} · {displayText(entry.status)}
              </p>
              {entry.turn_id && (
                <label>
                  会话与轮次
                  <select
                    aria-label="上下文任务与轮次"
                    value={entry.turn_id}
                    onChange={(event) => {
                      const next = entries.find(
                        (entry) => entry.turn_id === event.target.value,
                      );
                      if (next) choose(next);
                    }}
                  >
                    {entries
                      .filter((entry) => entry.turn_id)
                      .map((entry) => (
                        <option key={entry.turn_id} value={entry.turn_id}>
                          {displayText(entry.status)} · 会话{" "}
                          {displayText(entry.session_id)} · 轮次{" "}
                          {displayText(entry.turn_id)}
                        </option>
                      ))}
                  </select>
                </label>
              )}
              <WorkflowReferences
                source={entry}
                {...{ projectId, ownerId }}
                current="context"
              />
              <FrozenContext
                key={entry.turn_id || entry.execution_id}
                {...{ entry, projectId, ownerId, request }}
              />
              {entry.snapshot_id && (
                <AgentSnapshot
                  key={entry.session_id}
                  {...{ entry, projectId, ownerId, request }}
                />
              )}
            </>
          ) : (
            <p className="muted">
              {displayText(
                hasTarget && state.value
                  ? "目标不属于当前流程范围，或对应轮次尚未产生。"
                  : "选择步骤查看当时的上下文快照。",
              )}
            </p>
          )}
        </article>
      </div>
      {(offset > 0 || state.value?.more) && (
        <div className="actions">
          {offset > 0 && (
            <button onClick={() => setOffset(Math.max(0, offset - 100))}>
              上一页上下文
            </button>
          )}
          {state.value?.more && (
            <button onClick={() => setOffset(state.value!.next_offset)}>
              下一页上下文
            </button>
          )}
        </div>
      )}
      {contexts.length > 0 && (
        <details onToggle={(event) => setManualOpen(event.currentTarget.open)}>
          <summary>共享工作背景 · 手工维护（不代表当轮已发送）</summary>
          {manualOpen &&
            contexts.map((context) => (
              <ManualContext
                key={context.id}
                context={context}
                projectId={projectId}
                ownerId={ownerId}
                request={request}
                onEdit={onEditContext}
              />
            ))}
        </details>
      )}
    </section>
  );
}

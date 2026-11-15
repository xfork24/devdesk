import { type Request, type Row } from "./rpc-types";
import { AgentTaskView } from "./WorkflowTaskPreview";
import {
  RawJsonLink,
  DiscussionLink,
  ReadingLink,
  ReadState,
  ReadableValue,
} from "./reading";
import { useReading, navigateReading, executionUrl } from "./reading-state";
import { useState } from "react";

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
  const state = useReading(
    request,
    "query.execution_task",
    { project_id: projectId, turn_id: entry.turn_id },
    entry.task_available,
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
        <p className="muted">
          {entry.kind === "controller"
            ? "编排参与者的请求与响应记录在经理会话中；此处没有步骤冻结任务。"
            : "此轮未记录可读取的冻结步骤任务。"}
        </p>
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
  const state = useReading(request, "query.workflow_contexts", {
    project_id: projectId,
    owner_id: ownerId,
  });
  const [showHistory, setShowHistory] = useState(false);
  const agents: Row[] = state.value?.agents || [];
  const matching = agents.find(
    (a) =>
      (target.agent_instance_id && a.id === target.agent_instance_id) ||
      a.entries.some((e: Row) =>
        target.turn_id
          ? e.turn_id === target.turn_id
          : target.execution_id
            ? e.execution_id === target.execution_id
            : target.session_id && e.session_id === target.session_id,
      ),
  );
  const hasTarget = Boolean(
    target.agent_instance_id ||
    target.turn_id ||
    target.execution_id ||
    target.session_id,
  );
  const selected = matching || (!hasTarget ? agents[0] : undefined);
  const entries: Row[] = selected?.entries || [];
  const exact = entries.find((e) =>
    target.turn_id
      ? e.turn_id === target.turn_id
      : target.execution_id
        ? e.execution_id === target.execution_id
        : target.session_id && e.session_id === target.session_id,
  );
  const entry = exact || entries.find((e) => !e.historical) || entries[0];
  const visibleEntries = showHistory
    ? entries
    : entries.filter((e) => !e.historical || e.turn_id === entry?.turn_id);
  const shared = contexts.filter(
    (c) => c.owner_id === ownerId || c.owner_id === entry?.owner_id,
  );
  const choose = (target: Row) =>
    navigateReading(executionUrl("context", projectId, ownerId, target));
  return (
    <section aria-label="Agent 上下文">
      <h3>Agent 上下文</h3>
      <p className="muted">
        按实际参与的 Agent 实例组织。选择任务和轮次，查看当时冻结的依据。
      </p>
      <ReadState {...state} />
      <div className="reading-layout">
        <nav className="reading-directory" aria-label="Agent 实例目录">
          {agents.map((agent) => (
            <button
              key={agent.id}
              aria-current={agent.id === selected?.id}
              onClick={() => choose({ agent_instance_id: agent.id })}
            >
              {agent.actor.name}
              <small>
                {agent.actor.agent_instance_id
                  ? "Agent 实例"
                  : "实例身份未记录 · 独立会话"}{" "}
                · {agent.entries.length} 个任务轮次
              </small>
              <small title={agent.id}>
                实例{" "}
                {agent.id.split(":")[1]?.slice(0, 8) || agent.id.slice(0, 8)}
              </small>
            </button>
          ))}
          {state.value && !agents.length && (
            <p>当前工作流程尚无实际 Agent 上下文。</p>
          )}
        </nav>
        <article className="reading-body" aria-label="选中 Agent 上下文">
          {selected && entry ? (
            <>
              <h3>{selected.actor.name}</h3>
              <p className="muted">
                {entry.actor?.agent_revision_id} ·{" "}
                {entry.kind === "controller" ? "编排参与者" : "步骤执行者"}
              </p>
              <label>
                任务与轮次
                <select
                  aria-label="上下文任务与轮次"
                  value={entry.turn_id}
                  onChange={(e) =>
                    choose({
                      agent_instance_id: selected.id,
                      turn_id: e.target.value,
                    })
                  }
                >
                  {visibleEntries.map((e) => (
                    <option key={e.turn_id} value={e.turn_id}>
                      {e.owner_title || e.instance_name || "编排"} ·{" "}
                      {e.step_name} · 第 {e.round} 轮 ·{" "}
                      {e.historical ? "历史 · " : ""}
                      {e.status} · {e.turn_id.slice(0, 8)}
                    </option>
                  ))}
                </select>
              </label>
              {entries.some((e) => e.historical) && (
                <label>
                  <input
                    type="checkbox"
                    checked={showHistory}
                    onChange={(e) => setShowHistory(e.target.checked)}
                  />
                  显示前轮/继承会话
                </label>
              )}
              <p>
                冻结上下文：
                {entry.archived
                  ? "已归档"
                  : entry.historical
                    ? "历史轮次"
                    : "当前轮次"}
              </p>
              <div className="reading-links">
                <DiscussionLink
                  projectId={projectId}
                  ownerId={ownerId}
                  sessionId={entry.session_id}
                  taskId={
                    entry.owner_id !== ownerId && entry.owner_id !== projectId
                      ? entry.owner_id
                      : undefined
                  }
                >
                  查看此轮会话
                </DiscussionLink>
                {entry.execution_id && (
                  <ReadingLink
                    detail="records"
                    projectId={projectId}
                    ownerId={ownerId}
                    target={{
                      instance_id: entry.instance_id,
                      execution_id: entry.execution_id,
                    }}
                  >
                    查看执行记录
                  </ReadingLink>
                )}
              </div>
              <FrozenContext
                key={entry.turn_id}
                {...{ entry, projectId, ownerId, request }}
              />
              {entry.snapshot_id && (
                <AgentSnapshot
                  key={entry.session_id}
                  {...{ entry, projectId, ownerId, request }}
                />
              )}
              {shared.length > 0 && (
                <details>
                  <summary>共享工作背景 · 手工维护（不代表当轮已发送）</summary>
                  {shared.map((context) => (
                    <section key={context.id}>
                      <ReadableValue value={context.content} />
                      <RawJsonLink
                        source="context"
                        title="查看共享背景原始 JSON"
                        params={{
                          project_id: projectId,
                          owner_id: ownerId,
                          context_id: context.id,
                        }}
                      />
                      {onEditContext && (
                        <button onClick={() => onEditContext(context)}>
                          编辑上下文
                        </button>
                      )}
                    </section>
                  ))}
                </details>
              )}
              <details>
                <summary>会话与来源</summary>
                <ReadableValue
                  value={{
                    agent_instance_id: entry.actor?.agent_instance_id,
                    role_ref: entry.actor?.role_ref,
                    session_id: entry.session_id,
                    turn_id: entry.turn_id,
                    execution_id: entry.execution_id,
                  }}
                />
              </details>
            </>
          ) : (
            <p className="muted">
              {hasTarget && state.value
                ? "目标不属于当前工作流程的 Agent 上下文，或尚未派发为实际任务。"
                : "选择一个 Agent 查看它的任务上下文。"}
            </p>
          )}
        </article>
      </div>
      {!agents.length && contexts.length > 0 && (
        <details>
          <summary>尚未分配给 Agent 的共享工作背景</summary>
          {contexts.map((context) => (
            <section key={context.id}>
              <ReadableValue value={context.content} />
              <RawJsonLink
                source="context"
                title="查看共享背景原始 JSON"
                params={{
                  project_id: projectId,
                  owner_id: ownerId,
                  context_id: context.id,
                }}
              />
              {onEditContext && (
                <button onClick={() => onEditContext(context)}>
                  编辑上下文
                </button>
              )}
            </section>
          ))}
        </details>
      )}
    </section>
  );
}

import { MarkdownPreview } from "./configuration-ui";
import { type Row } from "./rpc-types";
import { RawJsonLink } from "./reading";
import { ReportBody } from "./DeliveryContent";
import { AgentTaskView } from "./WorkflowTaskPreview";

const kindNames: Record<string, string> = {
  user: "用户请求",
  assistant: "Agent 响应",
  tool: "工具调用 / 响应",
  status: "状态提示",
  approval: "审批",
  question: "待澄清问题",
};
export function ConversationContent({
  event,
  projectId,
  sessionId,
  ownerId,
}: {
  event: Row;
  projectId?: string;
  sessionId: string;
  ownerId: string;
}) {
  let parsed: unknown;
  try {
    parsed = JSON.parse(event.content);
  } catch {
    /* Plain text and streaming fragments remain visible. */
  }
  const value =
    parsed && typeof parsed === "object" ? (parsed as Row) : undefined;
  if (
    event.kind === "status" &&
    (event.content === "completed" || value?.writer_revoked !== undefined)
  )
    return null;
  const tool = event.kind === "tool";
  const isTask =
    !tool &&
    value &&
    (value.task_instructions || value.task?.task_instructions);
  const rawEvent = (event.kind === "assistant" || tool) && (
    <RawJsonLink
      source="event"
      title="原始事件"
      params={{
        project_id: projectId,
        id: sessionId,
        ...(tool
          ? { event_id: event.source_events?.at(-1)?.id || event.id }
          : { diagnostic_turn_id: event.turn_id }),
      }}
    />
  );
  return (
    <article className={`conversation-message ${event.kind}`}>
      <header>
        <strong>{kindNames[event.kind] || event.kind}</strong>
        {event.time && (
          <small>
            {" "}
            · {new Date(event.time).toLocaleString("zh-CN", { hour12: false })}
          </small>
        )}
      </header>
      <div
        className="conversation-message-content"
        role="group"
        aria-label="消息内容"
      >
        {isTask ? (
          <>
            <AgentTaskView
              value={{ ...value, turn_id: event.turn_id }}
              compact
              projectId={projectId}
              ownerId={ownerId}
            />
            {rawEvent && <span className="markdown-trailing">{rawEvent}</span>}
          </>
        ) : tool ? (
          <div className="markdown-preview" aria-label="工具类型">
            <h4>
              <span aria-label="工具名称">
                {[
                  value?.tool,
                  value?.name,
                  value?.method,
                  value?.message?.tool,
                  value?.message?.name,
                  value?.message?.method,
                  value?.type,
                ].find((name) => typeof name === "string" && name) ||
                  "工具事件"}
              </span>
              <span className="markdown-trailing">{rawEvent}</span>
            </h4>
          </div>
        ) : value ? (
          <>
            <ReportBody value={value} />
            {rawEvent && <span className="markdown-trailing">{rawEvent}</span>}
          </>
        ) : (
          <MarkdownPreview text={event.content} trailing={rawEvent} />
        )}
      </div>
    </article>
  );
}

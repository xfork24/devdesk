import { MarkdownPreview } from "./configuration-ui";
import { type Row } from "./rpc-types";
import { CompactValue, RawJsonLink, ReadableValue } from "./reading";
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
  const isTask =
    value && (value.task_instructions || value.task?.task_instructions);
  const tool = value && event.kind === "tool";
  const request =
    tool &&
    (value.arguments ??
      value.input ??
      value.command ??
      value.message?.arguments ??
      value.message?.input);
  const response =
    tool &&
    (value.result ??
      value.output ??
      value.aggregatedOutput ??
      value.message?.result ??
      value.message?.output);
  const error = tool && (value.error ?? value.message?.error);
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
      {isTask ? (
        <AgentTaskView
          value={{ ...value, turn_id: event.turn_id }}
          compact
          projectId={projectId}
          ownerId={ownerId}
        />
      ) : tool ? (
        <>
          <h4>
            {value.tool ||
              value.name ||
              value.method ||
              value.type ||
              "工具事件"}
          </h4>
          {(value.status !== undefined || value.exitCode !== undefined) && (
            <p>
              状态：{value.status || "未记录"}
              {value.exitCode !== undefined && ` · 退出码 ${value.exitCode}`}
            </p>
          )}
          {request !== undefined && (
            <section>
              <h4>请求</h4>
              <CompactValue value={request} label="请求" />
            </section>
          )}
          {response !== undefined && (
            <section>
              <h4>响应</h4>
              <CompactValue value={response} label="响应" />
            </section>
          )}
          {error && (
            <section role="status">
              <h4>错误</h4>
              <ReadableValue value={error} />
            </section>
          )}
          {request === undefined && response === undefined && !error && (
            <details>
              <summary>事件内容</summary>
              <ReportBody value={value} />
            </details>
          )}
        </>
      ) : value ? (
        <ReportBody value={value} />
      ) : (
        <MarkdownPreview text={event.content} />
      )}
      {event.kind === "assistant" && (
        <RawJsonLink
          source="event"
          title="原始事件"
          params={{
            project_id: projectId,
            id: sessionId,
            diagnostic_turn_id: event.turn_id,
          }}
        />
      )}
    </article>
  );
}

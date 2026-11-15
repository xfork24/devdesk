import { MarkdownPreview, type Row } from "./configuration-ui";
import { RawJsonLink, ReadableValue } from "./reading";
import { ReportBody } from "./DeliveryContent";
import { AgentTaskView } from "./WorkflowTaskPreview";

const kindNames: Record<string,string> = {user:"用户请求",assistant:"Agent 响应",tool:"工具调用 / 响应",status:"运行状态",approval:"审批",question:"待澄清问题"};
export function ConversationContent({event,projectId,sessionId,ownerId}: {event:Row;projectId?:string;sessionId:string;ownerId:string}) {
  let parsed: unknown;
  try { parsed = JSON.parse(event.content); } catch { /* Plain text and streaming fragments remain visible. */ }
  const value = parsed && typeof parsed === "object" ? parsed as Row : undefined;
  const sourceEvents:Row[] = event.source_events || [event];
  const isTask = value && (value.task_instructions || value.task?.task_instructions);
  const tool = value && event.kind === "tool";
  const request = tool && (value.arguments ?? value.input ?? value.command ?? value.message?.arguments ?? value.message?.input);
  const response = tool && (value.result ?? value.output ?? value.aggregatedOutput ?? value.message?.result ?? value.message?.output);
  const error = tool && (value.error ?? value.message?.error);
  return <article className={`conversation-message ${event.kind}`}>
    <header><strong>{kindNames[event.kind] || event.kind}</strong>{event.time && <small> · {new Date(event.time).toLocaleString("zh-CN",{hour12:false})}</small>}</header>
    {isTask ? <AgentTaskView value={{...value,turn_id:event.turn_id}} compact projectId={projectId} ownerId={ownerId}/> : tool ? <>
      <h4>{value.tool || value.name || value.method || value.type || "工具事件"}</h4>
      {(value.status !== undefined || value.exitCode !== undefined) && <p>状态：{value.status || "未记录"}{value.exitCode !== undefined && ` · 退出码 ${value.exitCode}`}</p>}
      {request !== undefined && <section><h4>请求</h4><ReadableValue value={request}/></section>}
      {response !== undefined && <details open={Boolean(error)}><summary>响应内容</summary><ReportBody value={response}/></details>}
      {error && <section role="status"><h4>错误</h4><ReadableValue value={error}/></section>}
      {request === undefined && response === undefined && !error && <details><summary>事件内容</summary><ReportBody value={value}/></details>}
    </> : value ? <ReportBody value={value}/> : <MarkdownPreview text={event.content}/>}
    {sourceEvents.length > 1 ? <details><summary>合并展示 · {sourceEvents.length} 条原始事件</summary>{sourceEvents.map(e=><div key={e.id}><RawJsonLink source="event" title={`原始事件 ${e.sequence}`} params={{project_id:projectId,id:sessionId,event_id:e.id}}/></div>)}</details> : <RawJsonLink source="event" params={{project_id:projectId,id:sessionId,event_id:event.id}}/>}
  </article>;
}

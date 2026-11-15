import { useEffect, useState } from "react";
import { MarkdownPreview, type Request, type Row } from "./configuration-ui";

export const readingKeys = ["instance_id", "execution_id", "output_id", "agent_instance_id", "session_id", "turn_id"] as const;
export function executionUrl(detail: string, projectId: string, ownerId: string, target: Row = {}) {
  const p = new URLSearchParams(location.search);
  for (const key of readingKeys) p.delete(key);
  p.set("tab", "execution"); p.set("detail", detail); p.set("project_id", projectId);
  if (ownerId !== projectId && !p.get("feature_id")) p.set("feature_id", ownerId);
  for (const [key, value] of Object.entries(target)) if (value) p.set(key, String(value));
  return `?${p}`;
}
export function navigateReading(url: string) {
  history.pushState(null, "", url);
  window.dispatchEvent(new PopStateEvent("popstate"));
}
export function ReadingLink({ detail, projectId, ownerId, target, children }: {
  detail: string; projectId: string; ownerId: string; target?: Row; children: React.ReactNode;
}) {
  const url = executionUrl(detail, projectId, ownerId, target);
  return <a href={url} onClick={e => { if (!e.metaKey && !e.ctrlKey && !e.shiftKey && e.button === 0) { e.preventDefault(); navigateReading(url); } }}>{children}</a>;
}
export function useReading(request: Request, method: string, params: Row, enabled = true) {
  const key = JSON.stringify(params);
  const [attempt, setAttempt] = useState(0);
  const [state, setState] = useState<{key: string; value?: Row; error?: string; loading: boolean}>({key: "", loading: false});
  useEffect(() => {
    if (!enabled) return;
    let active = true;
    setState({key, loading: true});
    void request(method, JSON.parse(key)).then(value => {
      if (active) setState({key, value, loading: false});
    }).catch(error => { if (active) setState({key, error: String(error), loading: false}); });
    return () => { active = false; };
  }, [request, method, key, enabled, attempt]);
  const current = state.key === key && enabled ? state : {key, value: undefined, error: undefined, loading: enabled};
  return {...current, retry: () => setAttempt(n => n + 1)};
}
export function ReadState({ loading, error, retry }: {loading: boolean; error?: string; retry: () => void}) {
  return loading ? <p role="status">正在读取…</p> : error ? <div role="alert"><p className="error">{error}</p><button onClick={retry}>重试读取</button></div> : null;
}
const rawMethods: Record<string, string> = {
  material: "query.workflow_materials", task: "query.execution_task", event: "query.session", execution: "query.workflow_executions", record: "query.workflow_record",
};
export function RawJsonLink({ source, params, title = "查看原始 JSON" }: {source: keyof typeof rawMethods; params: Row; title?: string}) {
  const p = new URLSearchParams({read: source});
  for (const [key, value] of Object.entries(params)) if (value != null && value !== "") p.set(key, String(value));
  const url = `?${p}`;
  return <a className="raw-json-link" href={url} target="_blank" rel="noopener noreferrer" onClick={e => {
    if (e.metaKey || e.ctrlKey || e.shiftKey) return;
    const popup = window.open(url, "_blank", "popup,width=1100,height=850");
    if (popup) { popup.opener = null; e.preventDefault(); }
  }}>{title} ↗</a>;
}
export function RawJsonReader({ request }: {request: Request}) {
  const p = new URLSearchParams(location.search), source = p.get("read") || "";
  const params = Object.fromEntries([...p].filter(([key]) => key !== "read"));
  const state = useReading(request, rawMethods[source] || "", params, Boolean(rawMethods[source]));
  const [copied, setCopied] = useState("");
  const raw = state.value === undefined ? "" : JSON.stringify(state.value, null, 2);
  return <main className="raw-reader"><header><h1>原始 JSON · {source}</h1><p>只读来源 · {params.output_id || params.turn_id || params.event_id || params.execution_id || params.record_id}</p><button disabled={!raw} onClick={() => { void navigator.clipboard.writeText(raw).then(() => setCopied("已复制完整响应")).catch(() => setCopied("复制失败，请手动选择文本")); }}>复制 JSON</button><span role="status">{copied}</span></header>
    {!rawMethods[source] && <p role="alert">不支持的原始数据来源。</p>}<ReadState {...state}/>{raw && <pre aria-label="完整原始 JSON"><code>{raw}</code></pre>}
  </main>;
}
const names: Record<string, string> = { summary: "摘要", description: "说明", instructions: "任务指令", title: "标题", status: "状态", reason: "原因", error: "错误", message: "消息", result: "结果", response: "响应", request: "请求", arguments: "请求参数", output: "输出", text: "正文", content: "内容", approve: "批准结论", checks: "检查项", passed: "通过", evidence: "证据", tasks: "任务", dependencies: "依赖", command: "命令", exit_code: "退出码", stdout: "标准输出", stderr: "错误输出", failure_type: "失败分类", candidate: "被测版本", environment: "环境", expected_failure: "预期失败", execution: "执行", inputs: "输入", outputs: "输出要求", input_sources: "输入来源", completion: "完成条件", authorization: "授权", runtime_context: "运行上下文" };
export const fieldName = (key: string) => names[key] || key;
export function ReadableValue({ value, depth = 0 }: {value: unknown; depth?: number}) {
  if (value === null || value === undefined) return <span className="muted">未记录</span>;
  if (typeof value === "string") return <MarkdownPreview text={value || "（空文本）"}/>;
  if (typeof value !== "object") return <span>{typeof value === "boolean" ? (value ? "是" : "否") : String(value)}</span>;
  if (Array.isArray(value)) {
    if (!value.length) return <p className="muted">暂无条目</p>;
    return <ol className="readable-list">{value.map((item, index) => <li key={index}><ReadableValue value={item} depth={depth + 1}/></li>)}</ol>;
  }
  const entries = Object.entries(value);
  if (!entries.length) return <p className="muted">无附加内容</p>;
  if (depth > 5) return <details><summary>更多嵌套字段</summary><ReadableValue value={value} depth={0}/></details>;
  return <dl className="readable-fields">{entries.map(([key, item]) => <div key={key}><dt>{fieldName(key)}</dt><dd><ReadableValue value={item} depth={depth + 1}/></dd></div>)}</dl>;
}

import { displayText } from "../display-text";
import {
  executionUrl,
  navigateReading,
  useReading,
  fieldName,
} from "./reading-state";
import { useState } from "react";
import { MarkdownPreview } from "./configuration-ui";
import { type Request, type Row } from "./rpc-types";

export function ReadingLink({
  detail,
  projectId,
  ownerId,
  target,
  children,
}: {
  detail: string;
  projectId: string;
  ownerId: string;
  target?: Row;
  children: React.ReactNode;
}) {
  const url = executionUrl(detail, projectId, ownerId, target);
  return (
    <a
      href={url}
      onClick={(e) => {
        if (!e.metaKey && !e.ctrlKey && !e.shiftKey && e.button === 0) {
          e.preventDefault();
          navigateReading(url);
        }
      }}
    >
      {displayText(children)}
    </a>
  );
}
export function ReadState({
  loading,
  error,
  retry,
}: {
  loading: boolean;
  error?: string;
  retry: () => void;
}) {
  return loading ? (
    <p role="status">正在读取…</p>
  ) : error ? (
    <div role="alert">
      <p className="error">{displayText(error)}</p>
      <button onClick={retry}>重试读取</button>
    </div>
  ) : null;
}
const rawMethods: Record<string, string> = {
  context: "query.workflow_contexts",
  material: "query.workflow_materials",
  task: "query.execution_task",
  event: "query.session",
  execution: "query.workflow_executions",
  record: "query.workflow_record",
};
export function RawJsonLink({
  source,
  params,
  title = "查看原始 JSON",
}: {
  source: keyof typeof rawMethods;
  params: Row;
  title?: string;
}) {
  const p = new URLSearchParams({ read: source });
  const view = new URLSearchParams(location.search).get("view");
  if (view) p.set("view", view);
  for (const [key, value] of Object.entries(params))
    if (value != null && value !== "") p.set(key, String(value));
  const url = `?${p}`;
  return (
    <a
      className="raw-json-link"
      href={url}
      target="_blank"
      rel="noopener noreferrer"
    >
      {displayText(title)} ↗
    </a>
  );
}
export function RawJsonReader({ request }: { request: Request }) {
  const p = new URLSearchParams(location.search),
    source = p.get("read") || "";
  p.delete("read");
  p.delete("view");
  const params = Object.fromEntries(p);
  const state = useReading(
    request,
    rawMethods[source] || "",
    params,
    Boolean(rawMethods[source]),
  );
  const [copied, setCopied] = useState("");
  const raw =
    state.value === undefined ? "" : JSON.stringify(state.value, null, 2);
  return (
    <main className="raw-reader">
      <header>
        <h1>
          {displayText(
            source === "event" ? "原始事件" : `原始 JSON · ${source}`,
          )}
        </h1>
        <p>
          只读来源 ·{" "}
          {displayText(
            params.event_id ||
              params.diagnostic_turn_id ||
              params.output_id ||
              params.turn_id ||
              params.execution_id ||
              params.record_id ||
              params.context_id,
          )}
        </p>
        <button
          disabled={!raw}
          onClick={() => {
            void navigator.clipboard
              .writeText(raw)
              .then(() => setCopied("已复制完整响应"))
              .catch(() => setCopied("复制失败，请手动选择文本"));
          }}
        >
          复制 JSON
        </button>
        <span role="status">{displayText(copied)}</span>
      </header>
      {!rawMethods[source] && <p role="alert">不支持的原始数据来源。</p>}
      <ReadState {...state} />
      {raw && (
        <p className="muted">UUID 显示前 8 位，复制 JSON 保留完整响应。</p>
      )}
      {raw && (
        <pre aria-label="完整原始 JSON">
          <code>{displayText(raw)}</code>
        </pre>
      )}
    </main>
  );
}
export function ReadableValue({
  value,
  depth = 0,
}: {
  value: unknown;
  depth?: number;
}) {
  if (value === null || value === undefined)
    return <span className="muted">未记录</span>;
  if (typeof value === "string") {
    if (depth < 4 && /^[\s]*[[{]/.test(value)) {
      try {
        const parsed = JSON.parse(value);
        if (parsed && typeof parsed === "object")
          return <ReadableValue value={parsed} depth={depth + 1} />;
      } catch {
        /* Preserve unparseable original text. */
      }
    }
    return <MarkdownPreview text={value || "（空文本）"} />;
  }
  if (typeof value !== "object")
    return (
      <span>
        {displayText(
          typeof value === "boolean" ? (value ? "是" : "否") : String(value),
        )}
      </span>
    );
  if (Array.isArray(value)) {
    if (!value.length) return <p className="muted">暂无条目</p>;
    return (
      <ol className="readable-list">
        {value.map((item, index) => (
          <li key={index}>
            <ReadableValue value={item} depth={depth + 1} />
          </li>
        ))}
      </ol>
    );
  }
  const entries = Object.entries(value);
  if (!entries.length) return <p className="muted">无附加内容</p>;
  if (depth > 5)
    return (
      <details>
        <summary>更多嵌套字段</summary>
        <ReadableValue value={value} depth={0} />
      </details>
    );
  return (
    <dl className="readable-fields">
      {entries.map(([key, item]) => (
        <div key={key}>
          <dt>{displayText(fieldName(key))}</dt>
          <dd>
            <ReadableValue value={item} depth={depth + 1} />
          </dd>
        </div>
      ))}
    </dl>
  );
}

export function DiscussionLink({
  projectId,
  ownerId,
  sessionId,
  taskId,
  featureId,
  turnId,
  children,
}: {
  projectId: string;
  ownerId: string;
  sessionId: string;
  taskId?: string;
  featureId?: string;
  turnId?: string;
  children: React.ReactNode;
}) {
  const p = new URLSearchParams(
    executionUrl("context", projectId, ownerId, { session_id: sessionId }),
  );
  p.set("tab", "discussion");
  p.delete("detail");
  if (featureId) p.set("feature_id", featureId);
  if (taskId) p.set("task_id", taskId);
  else p.delete("task_id");
  if (turnId) p.set("turn_id", turnId);
  const url = `?${p}`;
  return (
    <a
      href={url}
      onClick={(e) => {
        if (!e.metaKey && !e.ctrlKey && !e.shiftKey && e.button === 0) {
          e.preventDefault();
          navigateReading(url);
        }
      }}
    >
      {displayText(children)}
    </a>
  );
}

export function LazyDetails({
  summary,
  children,
}: {
  summary: React.ReactNode;
  children: React.ReactNode;
}) {
  const [open, setOpen] = useState(false);
  return (
    <details onToggle={(e) => setOpen(e.currentTarget.open)}>
      <summary>{displayText(summary)}</summary>
      {displayText(open && children)}
    </details>
  );
}

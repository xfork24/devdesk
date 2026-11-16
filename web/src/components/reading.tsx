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
      {children}
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
      <p className="error">{error}</p>
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
      {title} ↗
    </a>
  );
}
export function RawJsonReader({ request }: { request: Request }) {
  const p = new URLSearchParams(location.search),
    source = p.get("read") || "";
  const params = Object.fromEntries([...p].filter(([key]) => key !== "read"));
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
        <h1>{source === "event" ? "原始事件" : `原始 JSON · ${source}`}</h1>
        <p>
          只读来源 ·{" "}
          {params.event_id ||
            params.diagnostic_turn_id ||
            params.output_id ||
            params.turn_id ||
            params.execution_id ||
            params.record_id ||
            params.context_id}
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
        <span role="status">{copied}</span>
      </header>
      {!rawMethods[source] && <p role="alert">不支持的原始数据来源。</p>}
      <ReadState {...state} />
      {raw && (
        <pre aria-label="完整原始 JSON">
          <code>{raw}</code>
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
        {typeof value === "boolean" ? (value ? "是" : "否") : String(value)}
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
          <dt>{fieldName(key)}</dt>
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
  children,
}: {
  projectId: string;
  ownerId: string;
  sessionId: string;
  taskId?: string;
  children: React.ReactNode;
}) {
  const p = new URLSearchParams(
    executionUrl("context", projectId, ownerId, { session_id: sessionId }),
  );
  p.set("tab", "discussion");
  p.delete("detail");
  if (taskId) p.set("task_id", taskId);
  else p.delete("task_id");
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
      {children}
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
      <summary>{summary}</summary>
      {open && children}
    </details>
  );
}

function compactValue(value: unknown, depth = 0): unknown {
  if (typeof value === "string") {
    if (depth === 0 && /^[\s]*[[{]/.test(value)) {
      try {
        const parsed = JSON.parse(value);
        if (parsed && typeof parsed === "object")
          return compactValue(parsed, depth + 1);
      } catch {
        /* Original text remains available. */
      }
    }
    return value.length > 400
      ? value.slice(0, 400) +
          "\n\n（预览已缩短，完整内容可展开或查看原始 JSON）"
      : value;
  }
  if (!value || typeof value !== "object") return value;
  if (depth >= 3)
    return Array.isArray(value)
      ? `集合 · ${value.length} 项，展开查看`
      : `结构化内容 · ${Object.keys(value).length} 个字段，展开查看`;
  if (Array.isArray(value))
    return [
      ...value.slice(0, 3).map((v) => compactValue(v, depth + 1)),
      ...(value.length > 3 ? [`其余 ${value.length - 3} 项可展开查看`] : []),
    ];
  const entries = Object.entries(value);
  return Object.fromEntries([
    ...entries.slice(0, 10).map(([k, v]) => [k, compactValue(v, depth + 1)]),
    ...(entries.length > 10
      ? [["更多字段", `${entries.length - 10} 个字段可展开查看`]]
      : []),
  ]);
}
export function CompactValue({
  value,
  label,
}: {
  value: unknown;
  label: string;
}) {
  return (
    <>
      <ReadableValue value={compactValue(value)} />
      <LazyDetails summary={`展开完整${label}`}>
        <ReadableValue value={value} />
      </LazyDetails>
    </>
  );
}

import { useEffect, useState } from "react";
import { ActionButton, ActionSelect } from "./ActionFeedback";
import { sourcePayload, type Request, type Row } from "./configuration-ui";

export function SpecLint({
  request,
  project,
  kind,
  data,
  record,
}: {
  request: Request;
  project?: Row;
  kind: string;
  data: Row;
  record?: Row;
}) {
  const [text, setText] = useState("");
  const [encoding, setEncoding] = useState("text");
  const [result, setResult] = useState<Row>();
  const [error, setError] = useState("");
  const [busy, setBusy] = useState(false);
  useEffect(() => setResult(undefined), [text, encoding, data]);
  async function lint() {
    setBusy(true);
    setError("");
    try {
      const value = encoding === "json" ? JSON.parse(text) : text;
      const params =
        kind === "spec"
          ? { specification: data }
          : {
              delivery: {
                type: "delivery_type",
                identity: "unpublished-delivery",
                revision: 1,
                name: "待检查交付物",
                scope:
                  record?.scope ||
                  (project ? `project:${project.id}` : "global"),
                ...sourcePayload(kind, data),
              },
            };
      setResult(
        await request("query.spec_lint", {
          project_id: project?.id,
          ...params,
          value,
        }),
      );
    } catch (e) {
      setError(String(e));
      setResult(undefined);
    } finally {
      setBusy(false);
    }
  }
  return (
    <details className="full-width" aria-label="规范结构自检">
      <summary>检查内容样本</summary>
      <label>
        内容编码
        <ActionSelect
          aria-label="自检内容编码"
          value={encoding}
          onChange={(e) => setEncoding(e.target.value)}
        >
          <option value="text">文本 / Markdown</option>
          <option value="json">JSON</option>
        </ActionSelect>
      </label>
      <label>
        待检查内容
        <textarea
          aria-label="待检查内容"
          value={text}
          onChange={(e) => setText(e.target.value)}
        />
      </label>
      <ActionButton type="button" disabled={busy} onClick={() => void lint()}>
        运行结构自检
      </ActionButton>
      {error && <p role="alert">{error}</p>}
      {result && (
        <div role="status">
          <p>
            {result.structure_valid ? "结构检查通过" : "结构检查未通过"}
            ；语义质量仍需明确判断。
          </p>
          <ul>
            {(result.checks || []).map((check: Row) => (
              <li key={`${check.spec_ref}:${check.key}`}>
                {check.name}：
                {check.status === "requires_judgment"
                  ? "等待语义判断"
                  : check.passed
                    ? "通过"
                    : "未通过"}
              </li>
            ))}
          </ul>
          {result.format_schema_error && (
            <p>{result.format_schema_error.message}</p>
          )}
        </div>
      )}
    </details>
  );
}

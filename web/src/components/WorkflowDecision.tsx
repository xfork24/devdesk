import { useEffect, useRef, useState } from "react";
import { ActionButton } from "./ActionFeedback";
import { type Request, type Row } from "./rpc-types";

export function PendingWorkflowDecision({
  summary,
  request,
  projectId,
  busy,
  resolve,
}: {
  summary: Row;
  request: Request;
  projectId: string;
  busy: boolean;
  resolve: (record: Row, approved: boolean, commandId: string) => Promise<void>;
}) {
  const [detail, setDetail] = useState<Row>();
  const [loading, setLoading] = useState(false);
  const [error, setError] = useState("");
  const epoch = useRef(0);
  const intent = useRef<{ key: string; id: string } | undefined>(undefined);
  useEffect(
    () => () => {
      epoch.current += 1;
    },
    [],
  );
  const stale =
    detail && (detail.id !== summary.id || detail.revision < summary.revision);
  const eligible = detail && !stale && detail.data.status === "pending";
  const decide = (approved: boolean) => {
    if (!eligible) return;
    const key = JSON.stringify([
      projectId,
      detail.id,
      detail.revision,
      approved,
    ]);
    if (intent.current?.key !== key)
      intent.current = { key, id: crypto.randomUUID() };
    return resolve(detail, approved, intent.current.id);
  };
  const load = async () => {
    const ticket = ++epoch.current;
    setLoading(true);
    setError("");
    try {
      const value = await request("query.orchestration_decision", {
        project_id: projectId,
        id: summary.id,
      });
      if (ticket === epoch.current) setDetail(value);
    } catch (error) {
      if (ticket === epoch.current) setError(String(error));
    } finally {
      if (ticket === epoch.current) setLoading(false);
    }
  };
  return (
    <article aria-label="待决定的操作">
      <p>{summary.name}</p>
      <p>
        请求操作：{summary.data.operation} · 计划{" "}
        {summary.data.plan_revision_id}
      </p>
      <ActionButton disabled={loading || busy} onClick={() => void load()}>
        {detail ? "重新读取确切请求" : "查看确切请求"}
      </ActionButton>
      {detail && (
        <pre className="decision-request">{detail.data.request_json}</pre>
      )}
      {stale && (
        <p role="status">请求已变化。保留了原读取内容，请重新读取后决定。</p>
      )}
      {error && <p role="alert">{error}</p>}
      <div className="actions">
        <ActionButton
          disabled={busy || !eligible}
          onClick={() => void decide(true)}
        >
          批准此请求
        </ActionButton>
        <ActionButton
          disabled={busy || !eligible}
          onClick={() => void decide(false)}
        >
          拒绝此请求
        </ActionButton>
      </div>
    </article>
  );
}

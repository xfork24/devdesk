import { displayText } from "../sdk/display-text";
import { useReading } from "./reading-state";
import type { Request } from "../sdk/rpc-types";

export function WorkflowReasonText({
  request,
  projectId,
  instanceId,
  executionId,
  revision,
  preview,
  truncated,
  active = true,
  field = "reason",
}: {
  request: Request;
  projectId: string;
  instanceId: string;
  executionId?: string;
  revision?: number;
  preview?: string;
  truncated?: boolean;
  active?: boolean;
  field?: "reason" | "reconciliation_message";
}) {
  const full = useReading(
    request,
    "query.workflow_instance",
    {
      project_id: projectId,
      id: instanceId,
      execution_id: executionId,
      view: "reason",
    },
    Boolean(truncated && active),
    revision,
  );
  const current =
    full.value &&
    (revision === undefined ||
      full.value[
        field === "reason" ? "revision" : "reconciliation_revision"
      ] === revision);
  const body: unknown = current ? full.value?.[field] : undefined;
  const complete = typeof body === "string";
  return (
    <>
      <span className="workflow-reason-text">
        {displayText(complete ? body : preview || "")}
        {truncated && !complete && preview ? "…" : ""}
      </span>
      {full.loading && <span role="status"> 正在读取完整原因…</span>}
      {full.error && (
        <span role="status">
          {" "}
          完整原因读取失败。
          <button type="button" onClick={full.retry}>
            重试读取
          </button>
        </span>
      )}
      {full.value && !complete && (
        <span role="status"> 流程已更新，刷新后查看完整原因。</span>
      )}
    </>
  );
}

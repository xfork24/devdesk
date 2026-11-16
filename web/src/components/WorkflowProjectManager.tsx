import { useCallback, useEffect, useRef, useState } from "react";
import { ActionButton } from "./ActionFeedback";
import { WorkStandard } from "./WorkStandard";
import { WorkflowController, WorkflowPlanChanges } from "./WorkflowController";
import { ReadingLink, RawJsonLink } from "./reading";
import { type Request, type Row } from "./rpc-types";

type Props = {
  request: Request;
  record: Row;
  projectId: string;
  refresh: () => Promise<void>;
  nested?: boolean;
};

export function WorkflowProjectManager({
  request,
  record,
  projectId,
  refresh,
  nested = false,
}: Props) {
  const [data, setData] = useState<Row>();
  const [error, setError] = useState("");
  const [expandedChildren, setExpandedChildren] = useState<string[]>([]);
  const epoch = useRef(0);
  const load = useCallback(async () => {
    const ticket = ++epoch.current;
    const result = await request("query.workflow_instance", {
      id: record.id,
      project_id: projectId,
    });
    if (ticket === epoch.current) {
      setData(result);
      setError("");
    }
  }, [request, record.id, projectId]);
  useEffect(() => {
    void load().catch((error) => setError(String(error)));
    return () => {
      epoch.current += 1;
    };
  }, [record, load]);
  const refreshManagement = async () => {
    await load();
    await refresh();
  };
  if (!data) {
    return (
      <section
        className="panel"
        aria-label={nested ? "子流程管理" : "项目经理"}
      >
        <p role={error ? "alert" : "status"}>
          {error || "正在读取项目经理信息…"}
        </p>
        {error && (
          <ActionButton
            type="button"
            onClick={() =>
              void load().catch((error) => setError(String(error)))
            }
          >
            重新读取
          </ActionButton>
        )}
      </section>
    );
  }
  const current = data.record;
  const runtime = data.runtime;
  return (
    <section className="panel" aria-label={nested ? "子流程管理" : "项目经理"}>
      <div className="panel-heading">
        <h3>{current.name}</h3>
        <ActionButton
          type="button"
          onClick={() => void load().catch((error) => setError(String(error)))}
        >
          刷新管理状态
        </ActionButton>
      </div>
      <WorkStandard
        request={request}
        projectId={projectId}
        owner={{ id: runtime.owner_id }}
        instance={current}
        refresh={refreshManagement}
      />
      {!nested && (
        <WorkflowController
          request={request}
          instance={current}
          projectId={projectId}
          refresh={refreshManagement}
        />
      )}
      {!runtime.cancel_requested &&
        !["completed", "cancelled"].includes(runtime.status) && (
          <WorkflowPlanChanges
            request={request}
            instance={current}
            projectId={projectId}
            refresh={refreshManagement}
          />
        )}
      {!!data?.history?.length && (
        <details className="panel">
          <summary>计划与执行历史 · {data.history.length} 条</summary>
          {data.history.map((r: Row) => (
            <p key={r.id}>
              {r.name} ·{" "}
              {r.type === "workflow_execution_history" ? (
                <ReadingLink
                  detail="records"
                  projectId={projectId}
                  ownerId={runtime.owner_id}
                  target={{ instance_id: current.id, execution_id: r.data.id }}
                >
                  查看历史执行
                </ReadingLink>
              ) : (
                <RawJsonLink
                  source="record"
                  params={{
                    project_id: projectId,
                    owner_id: runtime.owner_id,
                    record_id: r.id,
                  }}
                  title="查看计划/变更原始记录"
                />
              )}
            </p>
          ))}
        </details>
      )}
      {(data?.children || []).map((child: Row) => (
        <details
          className="panel"
          key={child.id}
          onToggle={(event) => {
            const expanded = event.currentTarget.open;
            setExpandedChildren((previous) =>
              expanded
                ? [...new Set([...previous, child.id])]
                : previous.filter((id) => id !== child.id),
            );
          }}
        >
          <summary>子流程 · {child.name}</summary>
          {expandedChildren.includes(child.id) && (
            <WorkflowProjectManager
              nested
              request={request}
              record={child}
              projectId={projectId}
              refresh={refresh}
            />
          )}
        </details>
      ))}
      {error ? (
        <p role="alert" className="error">
          {error}
        </p>
      ) : null}
    </section>
  );
}

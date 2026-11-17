import { useEffect, useRef, useState } from "react";
import { ActionButton } from "./ActionFeedback";
import { WorkflowInstanceView } from "./WorkflowInstanceView";
import { type Request, type Row } from "./rpc-types";

type Counts = {
  total: number;
  completed: number;
  running: number;
  waiting: number;
  failed: number;
};
type Progress = {
  project_id: string;
  record: {
    id: string;
    name: string;
    revision: number;
    data: {
      owner_id: string;
      root_instance_id: string;
      status: string;
      paused: boolean;
      cancel_requested: boolean;
    };
  };
  counts: Counts;
  stages: { key: string; name: string; counts: Counts }[];
  next_stage_offset: number | null;
};
type Props = {
  request: Request;
  record: Row;
  projectId: string;
  refresh: () => Promise<void>;
  onDiscussion: (id: string, owner: string, turnId?: string) => void;
  ownerId?: string;
  target?: Row;
};
function status(counts: Counts) {
  if (counts.failed) return "需要处理";
  if (counts.running) return "执行中";
  if (counts.waiting) return "等待处理";
  if (counts.total && counts.completed === counts.total) return "已完成";
  return "等待前置";
}
// Overview polling reads stage counts only. The existing exact step/action view
// remains an explicit detail read until its own bounded protocol is migrated.
export function WorkflowProgressView(props: Props) {
  const { request, record, projectId } = props;
  const [data, setData] = useState<Progress>();
  const [error, setError] = useState("");
  const [offset, setOffset] = useState(0);
  const [expanded, setExpanded] = useState(Boolean(props.target?.execution_id));
  const generation = useRef(0);
  useEffect(() => {
    const current = ++generation.current;
    let active = true;
    let timer: ReturnType<typeof setTimeout>;
    async function poll() {
      try {
        const result: Progress = await request("query.workflow_progress", {
          project_id: projectId,
          id: record.id,
          stage_offset: offset,
        });
        if (active && generation.current === current) {
          setData(result);
          setError("");
        }
      } catch (error) {
        if (active && generation.current === current) setError(String(error));
      } finally {
        if (active) timer = setTimeout(() => void poll(), 2500);
      }
    }
    void poll();
    return () => {
      active = false;
      clearTimeout(timer);
      generation.current += 1;
    };
  }, [request, record.id, record.revision, projectId, offset]);
  const current =
    data?.project_id === projectId && data.record.id === record.id
      ? data
      : undefined;
  const label = current
    ? {
        running: "执行中",
        completed: "已完成",
        cancelled: "已取消",
        failed: "失败",
        queued: "等待前置",
      }[current.record.data.status] || status(current.counts)
    : "";
  return (
    <section className="panel workflow-instance" aria-label="工作流进度摘要">
      <div className="panel-heading">
        <div>
          <h3>{current?.record.name || record.name || "工作流程"}</h3>
          {current ? (
            <p role="status">
              {label} · {current.counts.completed} / {current.counts.total} 步骤
              {current.record.data.paused ? " · 已暂停" : ""}
              {current.record.data.cancel_requested ? " · 正在停止" : ""}
            </p>
          ) : (
            <p role="status">正在读取流程进度…</p>
          )}
        </div>
        <ActionButton
          type="button"
          aria-expanded={expanded}
          onClick={() => setExpanded((previous) => !previous)}
        >
          {expanded ? "收起步骤与操作" : "展开步骤与操作"}
        </ActionButton>
      </div>
      {current?.stages.map((stage) => (
        <div
          className="workflow-stage-row workflow-stage-progress"
          key={stage.key}
        >
          <strong>{stage.name}</strong>
          <span>
            {stage.counts.completed} / {stage.counts.total} ·{" "}
            {status(stage.counts)}
          </span>
        </div>
      ))}
      {offset > 0 || current?.next_stage_offset != null ? (
        <div className="actions" aria-label="阶段分页">
          <ActionButton
            type="button"
            disabled={offset === 0}
            onClick={() => setOffset((previous) => Math.max(0, previous - 20))}
          >
            上一页阶段
          </ActionButton>
          <ActionButton
            type="button"
            disabled={current?.next_stage_offset == null}
            onClick={() => setOffset(current!.next_stage_offset!)}
          >
            下一页阶段
          </ActionButton>
        </div>
      ) : null}
      {error ? <p role="alert">{error}</p> : null}
      {expanded ? <WorkflowInstanceView {...props} /> : null}
    </section>
  );
}

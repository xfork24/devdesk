import { useCallback, useRef, useState } from "react";
import { ArrowRight } from "lucide-react";
import { ActionButton } from "./ActionFeedback";
import { useRuntimeRefresh } from "./useRuntimeRefresh";
import type { Request, Row } from "./rpc-types";

type Counts = {
  total: number;
  completed: number;
  running: number;
  waiting: number;
  failed: number;
};
type Progress = {
  project_id: string;
  record: { id: string };
  counts: Counts;
  stages: { key: string; name: string; counts: Counts }[];
  next_stage_offset: number | null;
};

function stageStatus(counts: Counts) {
  if (counts.failed) return "需要处理";
  if (counts.running) return "执行中";
  if (counts.waiting) return "等待处理";
  if (counts.total && counts.completed === counts.total) return "已完成";
  return "等待前置";
}

export function WorkflowStageProgress({
  request,
  record,
  projectId,
  onStageSelect,
}: {
  request: Request;
  record: Row;
  projectId: string;
  onStageSelect: (key: string, stepCount: number) => void;
}) {
  const [data, setData] = useState<Progress>();
  const [error, setError] = useState("");
  const [offset, setOffset] = useState(0);
  const epoch = useRef(0);
  const invalidate = useCallback(() => {
    epoch.current += 1;
  }, []);
  const load = useCallback(async () => {
    const ticket = ++epoch.current;
    const result: Progress = await request("query.workflow_progress", {
      project_id: projectId,
      id: record.id,
      stage_offset: offset,
    });
    if (ticket === epoch.current) {
      setData(result);
      setError("");
    }
  }, [request, projectId, record, offset]);
  useRuntimeRefresh(load, projectId, record.id, {
    invalidate,
    onError: (value) => setError(String(value)),
  });
  const current =
    data?.project_id === projectId && data.record.id === record.id
      ? data
      : undefined;
  return (
    <section className="workflow-progress-summary" aria-label="阶段进度">
      <p role="status">
        {current
          ? `${current.counts.completed} / ${current.counts.total} 步骤已完成`
          : "正在读取阶段进度…"}
      </p>
      <ol className="workflow-progress-stages" aria-label="执行阶段进度">
        {current?.stages.map((stage, index) => (
          <li className="workflow-progress-stage" key={stage.key}>
            <button
              type="button"
              className="workflow-stage-row workflow-stage-progress"
              aria-controls={`workflow-stage-${record.id}-${stage.key}`}
              aria-label={`跳转到阶段：${stage.name}`}
              onClick={() => onStageSelect(stage.key, stage.counts.total)}
            >
              <strong>{stage.name}</strong>
              <span>
                {stage.counts.completed} / {stage.counts.total}{" "}
                {stageStatus(stage.counts)}
              </span>
            </button>
            {index < current.stages.length - 1 && (
              <ArrowRight className="workflow-stage-arrow" aria-hidden="true" />
            )}
          </li>
        ))}
      </ol>
      {offset > 0 || current?.next_stage_offset != null ? (
        <div className="actions" aria-label="阶段分页">
          <ActionButton
            disabled={offset === 0}
            onClick={() => setOffset(Math.max(0, offset - 20))}
          >
            上一页阶段
          </ActionButton>
          <ActionButton
            disabled={current?.next_stage_offset == null}
            onClick={() => setOffset(current!.next_stage_offset!)}
          >
            下一页阶段
          </ActionButton>
        </div>
      ) : null}
      {error && <p role="alert">{error}</p>}
    </section>
  );
}

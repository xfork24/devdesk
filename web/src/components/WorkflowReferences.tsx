import { displayText } from "../display-text";
import { DiscussionLink, ReadingLink } from "./reading";
import type { Row } from "./rpc-types";

export function WorkflowReferences({
  source,
  projectId,
  ownerId,
  current,
  compact = false,
}: {
  source: Row;
  projectId: string;
  ownerId: string;
  current?: string;
  compact?: boolean;
}) {
  const target = {
    instance_id: source.instance_id,
    execution_id: source.execution_id,
    session_id: source.session_id,
    turn_id: source.turn_id,
  };
  const outputs: Row[] = Array.isArray(source.outputs)
    ? source.outputs
    : Object.entries(source.outputs || {}).map(([port, id]) => ({ id, port }));
  return (
    <div className="workflow-references">
      <nav className="reading-links" aria-label="关联视图">
        {source.execution_id && current !== "overview" && (
          <ReadingLink
            detail="overview"
            {...{ projectId, ownerId }}
            target={target}
          >
            查看流程步骤
          </ReadingLink>
        )}
        {source.execution_id &&
          current !== "context" &&
          (source.context_available ?? source.status !== "queued") && (
            <ReadingLink
              detail="context"
              {...{ projectId, ownerId }}
              target={target}
            >
              查看执行者上下文
            </ReadingLink>
          )}
        {current !== "documents" &&
          outputs.map((output) => (
            <ReadingLink
              key={output.id}
              detail="documents"
              {...{ projectId, ownerId }}
              target={{ ...target, output_id: output.id }}
            >
              材料 · {displayText(output.name || output.port || output.id)}
            </ReadingLink>
          ))}
        {!compact &&
          source.execution_id &&
          current !== "records" &&
          (source.execution_available ?? source.status !== "queued") && (
            <ReadingLink
              detail="records"
              {...{ projectId, ownerId }}
              target={target}
            >
              查看执行记录
            </ReadingLink>
          )}
        {!compact && source.session_id && current !== "discussion" && (
          <DiscussionLink
            projectId={projectId}
            ownerId={source.owner_id || ownerId}
            featureId={source.feature_id}
            taskId={source.owner_kind === "task" ? source.owner_id : undefined}
            sessionId={source.session_id}
            turnId={source.turn_id}
          >
            查看此轮会话
          </DiscussionLink>
        )}
      </nav>
    </div>
  );
}

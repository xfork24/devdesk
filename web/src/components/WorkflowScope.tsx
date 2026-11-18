import { WorkflowInstanceView } from "./WorkflowInstanceView";
import { WorkflowProjectManager } from "./WorkflowProjectManager";
import { ReadState } from "./reading";
import { useReading } from "./reading-state";
import type { Request, Row } from "./rpc-types";

export function WorkflowScope({
  projectId,
  ownerId = projectId,
  request,
  refresh,
  records,
  view,
  target,
  onDiscussion,
  tasks = [],
}: {
  projectId: string;
  ownerId?: string;
  request: Request;
  refresh: () => Promise<void>;
  records: Row[];
  view: string;
  target: Row;
  onDiscussion: (id: string, owner: string, turnId?: string) => void;
  tasks?: Row[];
}) {
  const state = useReading(request, "query.workflow_executions", {
    project_id: projectId,
    owner_id: ownerId,
  });
  const destination = useReading(
    request,
    "query.workflow_executions",
    {
      project_id: projectId,
      owner_id: ownerId,
      instance_id: target.instance_id,
      execution_id: target.execution_id,
    },
    Boolean(target.instance_id && target.execution_id),
  );
  const roots = new Map<string, Row>();
  const startedTaskOwners = new Set<string>([
    ...records
      .filter((record) => record.type === "workflow_instance")
      .map((record) => record.data.owner_id),
    ...(state.value?.records || []).map((entry: Row) => entry.owner_id),
  ]);
  for (const record of records) {
    if (
      record.type === "workflow_instance" &&
      record.data.parent_execution_id == null &&
      record.work_root !== false &&
      (ownerId === projectId || record.data.owner_id === ownerId)
    )
      roots.set(record.id, record);
  }
  for (const entry of state.value?.records || []) {
    if (roots.has(entry.instance_id)) {
      const record = roots.get(entry.instance_id)!;
      roots.set(entry.instance_id, { ...record, _reading: entry });
    }
    if (
      (!entry.historical || entry.instance_id === target.instance_id) &&
      !entry.parent_execution_id &&
      !roots.has(entry.instance_id)
    ) {
      roots.set(entry.instance_id, {
        id: entry.instance_id,
        name: entry.instance_name,
        _reading: entry,
        data: { owner_id: entry.owner_id, steps: [], status: "queued" },
      });
    }
  }
  for (const entry of destination.value?.record
    ? [destination.value.record]
    : destination.value?.records || []) {
    const id = entry.root_instance_id || entry.instance_id;
    if (!roots.has(id))
      roots.set(id, {
        id,
        name: entry.instance_name,
        _reading: entry,
        data: { owner_id: ownerId, steps: [], status: "queued" },
      });
  }
  return (
    <>
      <ReadState {...state} />
      {[...roots.values()].map((record) =>
        view === "manager" ? (
          <WorkflowProjectManager
            key={record.id}
            {...{ request, record, projectId, ownerId, refresh }}
            unstartedTasks={tasks.filter(
              (task) =>
                !startedTaskOwners.has(task.id) &&
                task.feature_id === record.data.owner_id,
            )}
          />
        ) : (
          <WorkflowInstanceView
            key={record.id}
            {...{
              request,
              record,
              projectId,
              ownerId,
              refresh,
              target,
              onDiscussion,
            }}
          />
        ),
      )}
      {state.value && !roots.size && (
        <p className="muted">当前项目尚无已启动的工作流程。</p>
      )}
    </>
  );
}

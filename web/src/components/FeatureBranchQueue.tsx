import { useId, useState } from "react";
import { ChevronRight, GitBranch, GitPullRequest, Layers3 } from "lucide-react";
import { displayText } from "../display-text";
import { ReadState } from "./reading";
import { useReading } from "./reading-state";
import type { Request } from "./rpc-types";

export type FeatureQueueItem = { id: string; title: string; status: string };
type BranchPage = {
  candidate: {
    branch: string;
    status: string;
    pull_request_number: number | null;
  } | null;
  feature_branch: string | null;
  tasks: (FeatureQueueItem & { branch: string | null })[];
  next_task_offset: number | null;
};
type BranchSource = {
  projectId: string;
  github: string;
  request: Request;
  revision: number;
};

const terminalLabels: Record<string, string> = {
  completed: "已完成",
  cancelled: "已取消",
  failed: "失败",
  interrupted: "已中断",
};

function BranchStatus({ status }: { status: string }) {
  const terminalLabel = terminalLabels[status];
  return (
    <span
      className={`badge queue-branch-status status-${terminalLabel ? status : "running"}`}
    >
      {terminalLabel || "工作中"}
    </span>
  );
}

function BranchToggle({
  label,
  open,
  controls,
  toggle,
}: {
  label: string;
  open: boolean;
  controls: string;
  toggle: () => void;
}) {
  return (
    <button
      type="button"
      className="queue-branch-toggle"
      aria-label={displayText(`${open ? "折叠" : "展开"} ${label}`)}
      aria-expanded={open}
      aria-controls={controls}
      onClick={toggle}
    >
      <ChevronRight size={16} aria-hidden="true" />
    </button>
  );
}

function BranchName({ branch }: { branch?: string | null }) {
  return branch ? (
    <code className="queue-branch-ref" title={branch}>
      {displayText(branch)}
    </code>
  ) : (
    <span className="queue-branch-ref">尚无分支记录</span>
  );
}

function FeatureBranches({
  feature,
  source,
}: {
  feature: FeatureQueueItem;
  source: BranchSource;
}) {
  const [tasksOpen, setTasksOpen] = useState(false);
  const taskList = useId();
  const [offsets, setOffsets] = useState([0]);
  const state = useReading(
    source.request,
    "query.feature_branches",
    {
      project_id: source.projectId,
      feature_id: feature.id,
      task_offset: offsets[offsets.length - 1],
    },
    true,
    source.revision,
  );
  const page: BranchPage | undefined = state.value as BranchPage | undefined;
  if (!page)
    return (
      <li className="queue-branch-message">
        <ReadState {...state} />
      </li>
    );
  const { candidate, tasks, next_task_offset: nextOffset } = page;
  return (
    <>
      <li>
        <div className="queue-branch-row">
          <GitPullRequest size={17} aria-hidden="true" />
          <span className="queue-branch-label">
            <strong>Candidate 分支</strong>
            <BranchName branch={candidate?.branch} />
          </span>
          {source.github && candidate?.pull_request_number && (
            <a
              className="queue-branch-pr"
              href={`${source.github}/pull/${candidate.pull_request_number}`}
              target="_blank"
              rel="noreferrer"
            >
              PR #{candidate.pull_request_number}
            </a>
          )}
          {candidate ? <BranchStatus status={candidate.status} /> : null}
        </div>
      </li>
      <li>
        <div className="queue-branch-row">
          <BranchToggle
            label="Feature 分支的 Tasks"
            open={tasksOpen}
            controls={taskList}
            toggle={() => setTasksOpen((open) => !open)}
          />
          <GitBranch size={17} aria-hidden="true" />
          <span className="queue-branch-label">
            <strong>Feature 分支</strong>
            <BranchName branch={page.feature_branch} />
          </span>
          <BranchStatus status={feature.status} />
        </div>
        <ul id={taskList} hidden={!tasksOpen}>
          {tasksOpen &&
            (tasks.length ? (
              tasks.map((task) => (
                <li key={task.id}>
                  <div className="queue-branch-row">
                    <GitBranch size={16} aria-hidden="true" />
                    <span className="queue-branch-label">
                      <strong>{displayText(task.title)}</strong>
                      <BranchName branch={task.branch} />
                    </span>
                    <BranchStatus status={task.status} />
                  </div>
                </li>
              ))
            ) : (
              <li className="queue-branch-message">暂无 Task</li>
            ))}
          {tasksOpen && (offsets.length > 1 || nextOffset != null) && (
            <li className="queue-branch-message queue-task-pagination">
              <button
                type="button"
                disabled={offsets.length === 1}
                onClick={() => setOffsets((items) => items.slice(0, -1))}
              >
                上一页 Tasks
              </button>
              <button
                type="button"
                disabled={nextOffset == null}
                onClick={() => {
                  if (nextOffset != null)
                    setOffsets((items) => [...items, nextOffset]);
                }}
              >
                下一页 Tasks
              </button>
            </li>
          )}
        </ul>
      </li>
    </>
  );
}

function FeatureNode({
  feature,
  source,
  selected,
  select,
}: {
  feature: FeatureQueueItem;
  source: BranchSource;
  selected: boolean;
  select: (id: string) => void;
}) {
  const [open, setOpen] = useState(false);
  const branchList = useId();
  return (
    <li>
      <div className={`queue-branch-row${selected ? " selected" : ""}`}>
        <BranchToggle
          label={feature.title}
          open={open}
          controls={branchList}
          toggle={() => setOpen((previous) => !previous)}
        />
        <button
          type="button"
          className="queue-feature-select"
          aria-pressed={selected}
          onClick={() => {
            select(feature.id);
            setOpen(true);
          }}
        >
          <Layers3 size={17} aria-hidden="true" />
          <strong>{displayText(feature.title)}</strong>
        </button>
        <BranchStatus status={feature.status} />
      </div>
      <ul id={branchList} hidden={!open}>
        {open && <FeatureBranches feature={feature} source={source} />}
      </ul>
    </li>
  );
}

export function FeatureBranchQueue({
  features,
  selectedId,
  onSelect,
  projectId,
  repository,
  mainline,
  request,
  revision,
}: {
  features: FeatureQueueItem[];
  selectedId: string;
  onSelect: (id: string) => void;
  projectId: string;
  repository: string;
  mainline: string;
  request: Request;
  revision: number;
}) {
  const github = repository
    ? `https://github.com/${repository.split("/").map(encodeURIComponent).join("/")}`
    : "";
  const source = { projectId, github, request, revision };
  return (
    <ul className="feature-branch-queue" aria-label="项目分支结构">
      <li>
        <div className="queue-branch-row queue-mainline">
          <GitBranch size={18} aria-hidden="true" />
          <strong>{displayText(mainline)}</strong>
          <span>主分支</span>
        </div>
        <ul>
          {features.map((feature) => (
            <FeatureNode
              key={feature.id}
              feature={feature}
              source={source}
              selected={feature.id === selectedId}
              select={onSelect}
            />
          ))}
        </ul>
      </li>
    </ul>
  );
}

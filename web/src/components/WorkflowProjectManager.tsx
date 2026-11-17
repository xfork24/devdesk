import { displayText } from "../display-text";
import { useCallback, useEffect, useRef, useState } from "react";
import { ActionButton } from "./ActionFeedback";
import { WorkStandard } from "./WorkStandard";
import {
  WorkflowController,
  WorkflowPlanChanges,
  type WorkflowCollaboration,
} from "./WorkflowController";
import { ReadingLink, RawJsonLink } from "./reading";
import { type Request, type Row } from "./rpc-types";

type Props = {
  request: Request;
  record: Row;
  projectId: string;
  refresh: () => Promise<void>;
  nested?: boolean;
  ownerId?: string;
  inherited?: WorkflowCollaboration;
  onCollaboration?: (next: WorkflowCollaboration) => void;
  unstartedTasks?: Row[];
};

export function WorkflowProjectManager({
  request,
  record,
  projectId,
  refresh,
  nested = false,
  ownerId,
  inherited,
  onCollaboration,
  unstartedTasks = [],
}: Props) {
  const [data, setData] = useState<Row>();
  const [error, setError] = useState("");
  const [collaboration, setCollaboration] = useState<WorkflowCollaboration>();
  const shared = inherited || collaboration;
  const updateCollaboration = onCollaboration || setCollaboration;
  const pendingIds = new Set<string>();
  for (const approval of shared?.data.approvals || []) {
    if (
      approval.instance_id === record.id ||
      approval.ancestor_instance_ids?.includes(record.id)
    )
      pendingIds.add(approval.arguments.execution_id);
  }
  for (const decision of shared?.data.decisions || []) {
    if (
      decision.current !== false &&
      decision.data.status === "pending" &&
      (decision.definition_id === record.id ||
        decision.ancestor_instance_ids?.includes(record.id))
    )
      pendingIds.add(
        decision.data.request.arguments.execution_id || decision.id,
      );
  }
  for (const approval of shared?.data.runtimeApprovals || []) {
    if (
      approval.instance_id === record.id ||
      approval.ancestor_instance_ids?.includes(record.id)
    )
      pendingIds.add(approval.id);
  }
  for (const waiting of shared?.data.driverWaiting || []) {
    if (
      waiting.status === "waiting_input" &&
      (waiting.instance_id === record.id ||
        waiting.ancestor_instance_ids?.includes(record.id))
    )
      pendingIds.add(waiting.execution_id);
  }
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
  const current = data?.record || record;
  const runtime = data?.runtime || record.data;
  const title = nested
    ? `子流程 · ${/^执行各 Task(?: · \d+)?$/.test(current.name) ? "Task开发" : current.name}`
    : `主工作流 · ${current.name === "默认 Feature 开发流程" ? "Feature开发" : current.name}`;
  const cardClass = `panel manager-workflow-card ${nested ? "manager-child-flow" : "manager-workflow-summary"}`;
  if (!data) {
    return (
      <section
        className={cardClass}
        data-instance-id={current.id}
        aria-label={displayText(nested ? title : "项目经理")}
      >
        <div className="panel-heading">
          <h3>{displayText(title)}</h3>
        </div>
        <p role={error ? "alert" : "status"}>
          {displayText(
            error || (nested ? "正在读取子流程…" : "正在读取项目经理信息…"),
          )}
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
  const stateLabels: Record<string, string> = {
    running: "进行中",
    completed: "已完成",
    cancelled: "已取消",
    failed: "失败",
    queued: "等待执行",
    waiting_input: "等待输入",
    waiting_confirmation: "等待人类确认",
    awaiting_acceptance: "等待接纳",
    needs_reconciliation: "需要核对",
    skipped: "已跳过",
  };
  const content = (
    <>
      <div className="panel-heading">
        <h3>
          {displayText(title)}
          {nested && pendingIds.size > 0 && (
            <span className="manager-interaction-status">
              {displayText(pendingIds.size)} 项待处理
            </span>
          )}
        </h3>
        <ActionButton
          type="button"
          onClick={() => void load().catch((error) => setError(String(error)))}
        >
          刷新管理状态
        </ActionButton>
      </div>
      {error ? (
        <p role="alert" className="error">
          {displayText(error)}
        </p>
      ) : null}
      <section aria-label="当前工作进度" className="manager-progress">
        <h4>当前工作进度</h4>
        <p>
          流程状态：{displayText(stateLabels[runtime.status] || runtime.status)}
          {displayText(runtime.paused ? " · 已暂停" : "")}
        </p>
        <p>
          已完成{" "}
          {displayText(
            (runtime.steps || []).filter((step: Row) =>
              ["completed", "skipped"].includes(step.status),
            ).length,
          )}{" "}
          / {displayText((runtime.steps || []).length)} 个步骤
        </p>
        <progress
          aria-label="步骤完成进度"
          value={
            (runtime.steps || []).filter((step: Row) =>
              ["completed", "skipped"].includes(step.status),
            ).length
          }
          max={Math.max(1, (runtime.steps || []).length)}
        />
        <ul>
          {(runtime.steps || [])
            .filter(
              (step: Row) =>
                !["completed", "skipped", "queued"].includes(step.status),
            )
            .map((step: Row) => (
              <li key={step.id}>
                {displayText(
                  data?.plan?.steps?.find(
                    (definition: Row) => definition.key === step.step,
                  )?.name || step.step,
                )}{" "}
                ·{" "}
                {displayText(
                  step.reason || stateLabels[step.status] || step.status,
                )}{" "}
                <ReadingLink
                  detail="overview"
                  projectId={projectId}
                  ownerId={ownerId || runtime.owner_id}
                  target={{ instance_id: current.id, execution_id: step.id }}
                >
                  查看流程步骤
                </ReadingLink>
              </li>
            ))}
        </ul>
        <p className="reading-links">
          <ReadingLink
            detail="overview"
            projectId={projectId}
            ownerId={ownerId || runtime.owner_id}
          >
            查看工作流程
          </ReadingLink>
          <ReadingLink
            detail="documents"
            projectId={projectId}
            ownerId={ownerId || runtime.owner_id}
          >
            查看全部材料
          </ReadingLink>
        </p>
      </section>
      <section className="manager-standard" aria-label="流程规范">
        <WorkStandard
          request={request}
          projectId={projectId}
          owner={{ id: runtime.owner_id }}
          instance={current}
          refresh={refreshManagement}
        />
      </section>
      {!nested && unstartedTasks.length > 0 && (
        <details className="manager-task-settings">
          <summary>待启动任务的规范配置</summary>
          <div className="task-grid">
            {unstartedTasks.map((task) => (
              <article key={task.id}>
                <strong>{displayText(task.title)}</strong>
                <WorkStandard
                  request={request}
                  projectId={projectId}
                  owner={task}
                  refresh={refreshManagement}
                />
              </article>
            ))}
          </div>
        </details>
      )}
      {!runtime.cancel_requested &&
        !["completed", "cancelled"].includes(runtime.status) && (
          <WorkflowPlanChanges
            embedded
            request={request}
            instance={current}
            projectId={projectId}
            refresh={refreshManagement}
          />
        )}
      {!!data?.history?.length && (
        <details className="manager-history">
          <summary>
            计划与执行历史 · {displayText(data.history.length)} 条
          </summary>
          {data?.history.map((r: Row) => (
            <p key={r.id}>
              {displayText(r.name)} ·{" "}
              {r.type === "workflow_execution_history" ? (
                <ReadingLink
                  detail="records"
                  projectId={projectId}
                  ownerId={ownerId || runtime.owner_id}
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

      <WorkflowController
        embedded
        nested={nested}
        shared={nested ? shared : undefined}
        onRead={updateCollaboration}
        request={request}
        instance={current}
        projectId={projectId}
        refresh={refreshManagement}
      />
    </>
  );
  return (
    <>
      <section
        className={cardClass}
        data-instance-id={current.id}
        aria-label={displayText(nested ? title : "项目经理")}
      >
        {content}
      </section>
      {(data?.children || []).map((child: Row) => (
        <WorkflowProjectManager
          key={child.id}
          nested
          ownerId={ownerId}
          request={request}
          record={child}
          projectId={projectId}
          refresh={refresh}
          inherited={shared}
          onCollaboration={updateCollaboration}
        />
      ))}
    </>
  );
}

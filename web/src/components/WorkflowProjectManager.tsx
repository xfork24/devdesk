import { displayText } from "../display-text";
import { useCallback, useEffect, useRef, useState } from "react";
import { useRuntimeRefresh } from "./useRuntimeRefresh";
import { runtimeDisplayKey } from "../runtime-state";
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
  const [loadedData, setData] = useState<Row>();
  const data =
    loadedData?._projectId === projectId && loadedData?.record?.id === record.id
      ? loadedData
      : undefined;
  const [error, setError] = useState("");
  const [manualRefresh, setManualRefresh] = useState(0);
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
  const [history, setHistory] = useState<Row[]>();
  const [historyError, setHistoryError] = useState("");
  const [historyLoading, setHistoryLoading] = useState(false);
  const [historyBefore, setHistoryBefore] = useState<number>();
  const [fullConfiguration, setFullConfiguration] = useState<Row>();
  const [configurationLoading, setConfigurationLoading] = useState(false);
  const [childOffset, setChildOffset] = useState(0);
  const lifetime = useRef(0);
  useEffect(
    () => () => {
      lifetime.current += 1;
    },
    [projectId, record.id],
  );
  useEffect(() => {
    setHistory(undefined);
    setHistoryBefore(undefined);
    setFullConfiguration(undefined);
    setChildOffset(0);
  }, [projectId, record.id]);
  const loadHistory = async (before?: number) => {
    const ticket = lifetime.current;
    setHistoryLoading(true);
    setHistoryError("");
    try {
      const value = await request("query.workflow_instance", {
        project_id: projectId,
        id: record.id,
        view: "history",
        ...(before === undefined ? {} : { history_before: before }),
      });
      if (ticket === lifetime.current) {
        setHistory((previous) =>
          before === undefined
            ? value.history || []
            : [...(previous || []), ...(value.history || [])],
        );
        setHistoryBefore(value.next_history_before ?? undefined);
      }
    } catch (error) {
      if (ticket === lifetime.current) setHistoryError(String(error));
    } finally {
      if (ticket === lifetime.current) setHistoryLoading(false);
    }
  };

  const epoch = useRef(0);
  const load = useCallback(
    async (fresh = false) => {
      const ticket = ++epoch.current;
      const [result, progress] = await Promise.all([
        request(
          "query.workflow_management_summary",
          {
            id: record.id,
            project_id: projectId,
            child_offset: childOffset,
          },
          { fresh },
        ),
        request(
          "query.workflow_progress",
          {
            id: record.id,
            project_id: projectId,
          },
          { fresh },
        ),
      ]);
      if (ticket === epoch.current) {
        const next = { ...result, progress, _projectId: projectId };
        setData((previous) =>
          runtimeDisplayKey(previous) === runtimeDisplayKey(next)
            ? previous
            : next,
        );
        setError("");
      }
    },
    [request, record.id, projectId, childOffset],
  );
  const invalidate = useCallback(() => {
    epoch.current += 1;
  }, []);
  useRuntimeRefresh(load, projectId, record.id, {
    invalidate,
    onError: (value) => setError(String(value)),
  });
  const refreshManagement = async () => {
    await load();
    await refresh();
  };
  const current = data?.record || record;
  const runtime = data?.runtime || data?.record?.data || record.data;
  const observedRevision = Math.max(
    current.revision || 0,
    data?.progress?.record?.revision || 0,
  );
  const reloadShared = shared?.reload;
  useEffect(() => {
    if (!nested || !reloadShared) return;
    let active = true;
    // A child can reach its next gate after the root's collaboration read.
    // Refresh once per observed child revision without giving it another poller.
    void reloadShared(true).catch((value) => {
      if (active) setError(String(value));
    });
    return () => {
      active = false;
    };
  }, [nested, record.id, observedRevision, reloadShared]);
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
          onClick={() =>
            void Promise.all([load(true), shared?.reload(true)])
              .then(() => setManualRefresh((value) => value + 1))
              .catch((error) => setError(String(error)))
          }
        >
          刷新管理状态
        </ActionButton>
      </div>
      {data?.delivery ? (
        <p aria-label="PR 合入与同步状态">
          {data.delivery.pull_request_url ? (
            <a
              href={data.delivery.pull_request_url}
              target="_blank"
              rel="noreferrer"
            >
              PR #{data.delivery.pull_request_number}
            </a>
          ) : (
            "PR 待创建"
          )}
          {" · "}
          {data.delivery.merged ? "远端已合入" : "等待合入"}
          {" · "}
          {data.delivery.synchronized ? "本地已同步" : "本地待同步"}
        </p>
      ) : null}
      {(data?.child_slots || []).map((slots: Row) => (
        <p key={slots.execution_id} aria-label="子流程并发槽位">
          Task 并发：{slots.active} / {slots.limit}，可用 {slots.available}
        </p>
      ))}
      {(data?.children || []).length ? (
        <ul aria-label="Task 依赖与交付进展">
          {data.children.map((child: Row) => (
            <li key={child.id}>
              {displayText(child.name)} ·{" "}
              {child.dependency_wait_count
                ? `等待 ${child.dependency_wait_count} 项依赖`
                : child.status === "queued"
                  ? "依赖已满足"
                  : displayText(stateLabels[child.status] || child.status)}
              {child.delivery?.pull_request_url ? (
                <>
                  {" "}
                  ·{" "}
                  <a
                    href={child.delivery.pull_request_url}
                    target="_blank"
                    rel="noreferrer"
                  >
                    PR #{child.delivery.pull_request_number}
                  </a>{" "}
                  ·{" "}
                  {child.delivery.synchronized
                    ? "已合入并同步"
                    : child.delivery.merged
                      ? "已合入，等待本地同步"
                      : "等待合入"}
                </>
              ) : null}
            </li>
          ))}
        </ul>
      ) : null}
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
          已完成 {displayText(data?.progress?.counts?.completed ?? 0)} /{" "}
          {displayText(data?.progress?.counts?.total || 0)} 个步骤
        </p>
        <progress
          aria-label="步骤完成进度"
          value={data?.progress?.counts?.completed ?? 0}
          max={Math.max(1, data?.progress?.counts?.total || 0)}
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
        {runtime.configuration_truncated && !fullConfiguration ? (
          <ActionButton
            disabled={configurationLoading}
            onClick={() => {
              const ticket = lifetime.current;
              setConfigurationLoading(true);
              void request("query.workflow_instance", {
                project_id: projectId,
                id: current.id,
                view: "detail",
              })
                .then((value) => {
                  if (ticket === lifetime.current)
                    setFullConfiguration(value.record);
                })
                .catch((error) => {
                  if (ticket === lifetime.current) setError(String(error));
                })
                .finally(() => {
                  if (ticket === lifetime.current)
                    setConfigurationLoading(false);
                });
            }}
          >
            {configurationLoading
              ? "正在读取完整执行配置…"
              : "读取完整执行配置"}
          </ActionButton>
        ) : (
          <WorkStandard
            request={request}
            projectId={projectId}
            owner={{ id: runtime.owner_id }}
            instance={fullConfiguration || current}
            refresh={refreshManagement}
          />
        )}
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
      <WorkflowPlanChanges
        embedded
        readOnly={
          runtime.cancel_requested ||
          ["completed", "cancelled"].includes(runtime.status)
        }
        request={request}
        instance={current}
        projectId={projectId}
        refresh={refreshManagement}
      >
        <details
          className="manager-history"
          onToggle={(event) => {
            if (event.currentTarget.open && !history && !historyLoading) {
              void loadHistory();
            }
          }}
        >
          <summary>
            计划与执行历史{history ? ` · ${history.length} 条` : ""}
          </summary>
          {historyError && <p role="alert">{historyError}</p>}
          {historyLoading && <p role="status">正在读取计划与执行历史…</p>}
          {(history || []).map((r: Row) => (
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
          {historyBefore !== undefined ? (
            <ActionButton
              disabled={historyLoading}
              onClick={() => void loadHistory(historyBefore)}
            >
              继续读取历史
            </ActionButton>
          ) : null}
        </details>
      </WorkflowPlanChanges>

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
        data-manual-refresh={manualRefresh}
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
          record={{
            ...child,
            data: child.data || {
              owner_id: runtime.owner_id,
              root_instance_id: runtime.root_instance_id,
              steps: [],
            },
          }}
          projectId={projectId}
          refresh={refresh}
          inherited={shared}
          onCollaboration={updateCollaboration}
        />
      ))}
      {(childOffset > 0 || data?.children_more) && (
        <div className="actions">
          {childOffset > 0 && (
            <ActionButton
              onClick={() => setChildOffset(Math.max(0, childOffset - 20))}
            >
              上一页子流程
            </ActionButton>
          )}
          {data?.next_child_offset != null && (
            <ActionButton
              onClick={() => setChildOffset(data.next_child_offset)}
            >
              下一页子流程
            </ActionButton>
          )}
        </div>
      )}
    </>
  );
}

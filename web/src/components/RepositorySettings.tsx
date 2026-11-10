import {
  ActionButton,
  ActionForm,
  ActionSelect,
  useActionFeedback,
} from "./ActionFeedback";
import { useEffect, useState } from "react";
type Row = Record<string, any>;
const operationLabels: Record<string, string> = {
  prepared: "等待接入",
  dispatched: "处理中",
  observed: "正在确认结果",
  completed: "已完成",
  needs_reconciliation: "需要核对",
};
type Props = {
  project: Row;
  ready: boolean;
  request: (method: string, params?: Row) => Promise<any>;
  refresh: () => Promise<void>;
};
export function RepositorySettings({
  project,
  ready,
  request,
  refresh,
}: Props) {
  const {
    error,
    setError,
    capture: captureFeedback,
    beginFeedback,
    endFeedback,
  } = useActionFeedback();

  const [state, setState] = useState<Row>();
  const [busy, setBusy] = useState(false);

  const repository = project.workspace_definition?.repository;
  const connection = state?.operations.find(
    (op: Row) => op.action.kind === "connect",
  );
  const connectionBlocked =
    connection?.status === "needs_reconciliation" &&
    connection.reason !== "reconcile_requested";
  useEffect(() => {
    let active = true;
    let timer: ReturnType<typeof setTimeout>;
    async function poll() {
      captureFeedback();

      try {
        const value = await request("query.repository", {
          project_id: project.id,
        });
        if (!active) return;
        setState(value);
        setError("");
        if (
          !ready &&
          value.operations.some(
            (op: Row) =>
              op.action.kind === "connect" && op.status === "completed",
          )
        )
          await refresh();
      } catch (e) {
        if (active) setError(String(e));
      } finally {
        if (active) timer = setTimeout(poll, 1500);
      }
    }
    void poll();
    return () => {
      active = false;
      clearTimeout(timer);
    };
  }, [project.id, ready, request, refresh]);
  return (
    <section className="panel" aria-label="GitHub 仓库状态">
      <div className="panel-heading">
        <div>
          <h2>GitHub 仓库</h2>
          <p className="muted">打开项目时自动检查权限、登记并克隆仓库。</p>
        </div>
      </div>
      <div style={{ padding: 20 }}>
        <p className="repository-connection-summary">
          <a
            href={`https://github.com/${repository}`}
            target="_blank"
            rel="noopener noreferrer"
          >
            {repository}
          </a>{" "}
          · {project.workspace_definition?.mainline}{" "}
          <span role="status">
            {ready
              ? "仓库已接入"
              : connectionBlocked
                ? "仓库接入未完成，请根据下方原因核对后重试"
                : "正在准备仓库，完成后即可开始工作"}
          </span>
        </p>
        {error && (
          <div className="error" role="alert">
            {error}
          </div>
        )}
        {state?.operations
          .filter((op: Row) => op.status !== "completed")
          .map((op: Row) => (
            <div className="setting-row" key={op.id}>
              <div>
                <strong>
                  {op.action.kind === "connect" ? "仓库接入" : op.action.kind} ·{" "}
                  {op.reason === "reconcile_requested"
                    ? "等待后台核对"
                    : operationLabels[op.status] || op.status}
                </strong>
                <p className="muted">
                  {op.reason === "reconcile_requested"
                    ? "已收到重试请求，后台将继续核对原仓库操作。"
                    : op.reason}
                </p>
              </div>
              {op.status === "needs_reconciliation" && (
                <ActionButton
                  disabled={busy || op.reason === "reconcile_requested"}
                  onClick={async () => {
                    captureFeedback();

                    (beginFeedback(), setBusy(true));
                    setError("");
                    try {
                      await request("repository.reconcile", {
                        project_id: project.id,
                        command_id: crypto.randomUUID(),
                        target: op.id,
                        expected_revision: op.revision,
                        payload: {},
                      });
                      await refresh();
                    } catch (e) {
                      setError(String(e));
                    } finally {
                      (endFeedback(), setBusy(false));
                    }
                  }}
                >
                  核对并重试原操作
                </ActionButton>
              )}
            </div>
          ))}
        {state?.worktrees.map((w: Row) => (
          <div key={w.id}>
            <strong>
              {w.kind} · {w.branch}
            </strong>
            <p className="muted">
              {w.path} · base {w.base}
            </p>
          </div>
        ))}
      </div>
    </section>
  );
}

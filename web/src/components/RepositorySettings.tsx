import { useCallback, useEffect, useState } from "react";
type Row = Record<string, any>;
type Props = {
  project: Row;
  request: (method: string, params?: Row) => Promise<any>;
  refresh: () => Promise<void>;
};
export function RepositorySettings({ project, request, refresh }: Props) {
  const [repository, setRepository] = useState(
    project.workspace_definition?.repository || "",
  );
  const [branch, setBranch] = useState(
    project.workspace_definition?.mainline || "main",
  );
  const [diagnosis, setDiagnosis] = useState<Row>();
  const [state, setState] = useState<Row>();
  const [busy, setBusy] = useState(false);
  const [error, setError] = useState("");
  const load = useCallback(async () => {
    const value = await request("query.repository", { project_id: project.id });
    setState(value);
  }, [project.id, request]);
  useEffect(() => {
    let active = true;
    let inFlight = false;
    async function poll() {
      if (inFlight) return;
      inFlight = true;
      try {
        const value = await request("query.repository", {
          project_id: project.id,
        });
        if (active) setState(value);
      } catch (e) {
        if (active) setError(String(e));
      } finally {
        inFlight = false;
      }
    }
    void poll();
    const timer = setInterval(poll, 1500);
    return () => {
      active = false;
      clearInterval(timer);
    };
  }, [project.id, request]);
  async function action(work: () => Promise<void>) {
    setBusy(true);
    setError("");
    try {
      await work();
      await load();
      await refresh();
    } catch (e) {
      setError(String(e));
    } finally {
      setBusy(false);
    }
  }
  return (
    <section className="panel">
      <div className="panel-heading">
        <div>
          <h2>GitHub 仓库接入</h2>
          <p className="muted">
            使用独立项目登记仓库。宿主负责 Git/PR，Agent 容器仅访问所属
            worktree。当前接入进度按操作回执显示。
          </p>
        </div>
      </div>
      <div style={{ padding: 20 }}>
        <label>
          GitHub owner/repo
          <input
            aria-label="GitHub owner/repo"
            value={repository}
            onChange={(e) => {
              setRepository(e.target.value);
              setDiagnosis(undefined);
            }}
            disabled={project.execution_mode === "live"}
          />
        </label>
        <label>
          Mainline
          <input
            aria-label="GitHub Mainline"
            value={branch}
            onChange={(e) => {
              setBranch(e.target.value);
              setDiagnosis(undefined);
            }}
            disabled={project.execution_mode === "live"}
          />
        </label>
        <div className="actions">
          <button
            disabled={busy || !repository}
            onClick={() =>
              action(async () => {
                setDiagnosis(
                  await request("github.diagnose", { repository, branch }),
                );
              })
            }
          >
            检查 GitHub 规则与权限
          </button>
          <button
            disabled={
              busy ||
              !diagnosis?.compatible ||
              project.execution_mode === "live" ||
              state?.operations.some((op: Row) => op.action.kind === "connect")
            }
            onClick={() =>
              action(async () => {
                await request("repository.connect", {
                  project_id: project.id,
                  command_id: crypto.randomUUID(),
                  target: project.id,
                  expected_revision: project.revision,
                  payload: { repository, mainline: branch },
                });
              })
            }
          >
            登记并克隆仓库
          </button>
        </div>
        {diagnosis && (
          <div className={diagnosis.compatible ? "notice" : "error"}>
            {diagnosis.compatible ? "本轮本地 CAS 策略可用" : "本轮策略不兼容"}{" "}
            · {diagnosis.viewer_permission}
            {diagnosis.blocked_reasons.map((reason: string) => (
              <p key={reason}>{reason}</p>
            ))}
          </div>
        )}
        {error && (
          <div className="error" role="alert">
            {error}
          </div>
        )}
        {state?.operations.map((op: Row) => (
          <div className="setting-row" key={op.id}>
            <div>
              <strong>
                {op.action.kind} · {op.status}
              </strong>
              <p className="muted">
                {op.id} · {op.reason}
              </p>
            </div>
            {op.status === "needs_reconciliation" && (
              <button
                disabled={busy}
                onClick={() =>
                  action(async () => {
                    await request("repository.reconcile", {
                      project_id: project.id,
                      command_id: crypto.randomUUID(),
                      target: op.id,
                      expected_revision: op.revision,
                      payload: {},
                    });
                  })
                }
              >
                核对原操作
              </button>
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

import {
  ActionButton,
  ActionForm,
  ActionSelect,
  useActionFeedback,
} from "./ActionFeedback";
import { useEffect, useRef, useState } from "react";
type Row = Record<string, any>;
type Props = {
  revision?: string;
  request: (method: string, params?: Row) => Promise<any>;
  projectId: string;
  instanceId: string;
  owner: Row;
  onRestored: (id: string) => void;
  onLocate?: (entry: Row) => void;
};
export function AgentExperience({
  request,
  revision,
  projectId,
  instanceId,
  owner,
  onRestored,
  onLocate,
}: Props) {
  const {
    error,
    setError,
    capture: captureFeedback,
    beginFeedback,
    endFeedback,
  } = useActionFeedback();

  const [project, setProject] = useState(""),
    [feature, setFeature] = useState(""),
    [from, setFrom] = useState(""),
    [to, setTo] = useState(""),
    [page, setPage] = useState(0),
    [data, setData] = useState<Row>(),
    [detail, setDetail] = useState<Row>(),
    [busy, setBusy] = useState(false);
  const generation = useRef(0);
  useEffect(() => {
    if (!instanceId) return;
    const g = ++generation.current;
    setDetail(undefined);
    setError("");
    request("query.agent_experience", {
      ...(projectId === "global"
        ? { scope: "global" }
        : { project_id: projectId }),
      agent_instance_id: instanceId,
      project_name: project,
      feature_name: feature,
      from: from ? new Date(from).getTime() : undefined,
      to: to ? new Date(to).getTime() : undefined,
      offset: page * 20,
      limit: 20,
    })
      .then((result) => {
        if (g === generation.current) setData(result);
      })
      .catch((e) => {
        if (g === generation.current) setError(String(e));
      });
    return () => {
      ++generation.current;
    };
  }, [
    request,
    projectId,
    instanceId,
    project,
    feature,
    from,
    to,
    page,
    revision,
  ]);
  const filters = (
    <div className="form-grid">
      <label>
        项目名称
        <input
          value={project}
          onChange={(e) => {
            captureFeedback();

            setProject(e.target.value);
            setPage(0);
          }}
        />
      </label>
      <label>
        Feature 名称
        <input
          value={feature}
          onChange={(e) => {
            captureFeedback();

            setFeature(e.target.value);
            setPage(0);
          }}
        />
      </label>
      <label>
        起始时间
        <input
          type="datetime-local"
          value={from}
          onChange={(e) => {
            captureFeedback();

            setFrom(e.target.value);
            setPage(0);
          }}
        />
      </label>
      <label>
        截止时间
        <input
          type="datetime-local"
          value={to}
          onChange={(e) => {
            captureFeedback();

            setTo(e.target.value);
            setPage(0);
          }}
        />
      </label>
    </div>
  );
  return (
    <details className="experience-panel">
      <summary>AgentID 经验 · 项目 / Feature / 时间</summary>
      {!instanceId ? (
        <p>选择 AgentID 后查看该实例的工作履历。</p>
      ) : (
        <>
          <p className="mono">
            {instanceId} · {data?.scope.kind || ""}
          </p>
          {filters}
          <p>{data?.total || 0} 条快照 · 时间按当前时区显示</p>
          {data?.records.map((entry: Row) => (
            <article key={entry.snapshot_id + entry.session_id}>
              <strong>
                {entry.project_name} / {entry.feature_name}
              </strong>
              <p>
                {new Date(entry.captured_at).toLocaleString()} · {entry.kind} ·{" "}
                {entry.task_id
                  ? `Task ${entry.task_id.slice(0, 8)}`
                  : "Feature"}
              </p>
              <div className="actions">
                <ActionButton
                  onClick={async () => {
                    captureFeedback();

                    setError("");
                    const g = ++generation.current;
                    try {
                      const value = await request("query.agent_experience", {
                        ...(projectId === "global"
                          ? { scope: "global" }
                          : { project_id: projectId }),
                        agent_instance_id: instanceId,
                        snapshot_id: entry.snapshot_id,
                        source_project_id: entry.project_id,
                      });
                      if (g === generation.current) setDetail(value);
                    } catch (e) {
                      if (g === generation.current) setError(String(e));
                    }
                  }}
                >
                  展开摘要与快照
                </ActionButton>
                {onLocate && (
                  <ActionButton
                    onClick={() => {
                      captureFeedback();
                      return onLocate(entry);
                    }}
                  >
                    定位来源工作
                  </ActionButton>
                )}
              </div>
            </article>
          ))}
          {data?.unavailable.map((p: Row) => (
            <p key={p.project_id} className="notice">
              {p.project_name}：历史项目当前不可用
            </p>
          ))}
          <div className="actions">
            <ActionButton
              disabled={!page}
              onClick={() => {
                captureFeedback();
                return setPage((p) => p - 1);
              }}
            >
              上一页
            </ActionButton>
            <ActionButton
              disabled={(page + 1) * 20 >= (data?.total || 0)}
              onClick={() => {
                captureFeedback();
                return setPage((p) => p + 1);
              }}
            >
              下一页
            </ActionButton>
          </div>
          {detail && (
            <article className="notice">
              <h3>{detail.entry.feature_name} · 快照摘要</h3>
              <p>
                Model {detail.entry.binding?.model} · Harness{" "}
                {detail.entry.binding?.installation?.key.provider} · 完整性{" "}
                {detail.integrity}
              </p>
              <pre>
                {detail.snapshot.content ||
                  "该快照没有附加文字上下文；文档与执行引用仍可读取。"}
              </pre>
              <details>
                <summary>完整快照与证据引用</summary>
                <pre>{JSON.stringify(detail, null, 2)}</pre>
              </details>
              <ActionButton
                disabled={busy}
                onClick={async () => {
                  captureFeedback();

                  (beginFeedback(), setBusy(true));
                  setError("");
                  try {
                    const result = await request("experience.restore", {
                      ...(projectId === "global"
                        ? { scope: "global" }
                        : { project_id: projectId }),
                      command_id: crypto.randomUUID(),
                      target: owner.id,
                      expected_revision: owner.revision,
                      payload: {
                        agent_instance_id: instanceId,
                        snapshot_id: detail.snapshot.id,
                        source_project_id: detail.entry.project_id,
                      },
                    });
                    onRestored(result.id);
                  } catch (e) {
                    setError(String(e));
                  } finally {
                    (endFeedback(), setBusy(false));
                  }
                }}
              >
                基于此快照开启新讨论
              </ActionButton>
              <p>恢复保留来源引用，当前代码、文档和工作流配置保持各自版本。</p>
            </article>
          )}
        </>
      )}
      {error && (
        <p role="alert" className="error">
          {error}
        </p>
      )}
    </details>
  );
}

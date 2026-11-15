import { useEffect, useState } from "react";
import { ActionButton } from "./ActionFeedback";
import { DeliveryOutput } from "./DeliveryContent";
import type { Request, Row } from "./configuration-ui";

type Material = {
  id: string;
  delivery_name: string;
  owner_title: string;
  owner_kind: string;
  step_name: string;
  stage_name: string | null;
  path: string;
  port: string;
  status: string;
  round: number;
};
const statuses: Record<string, string> = {
  completed: "已接纳",
  awaiting_acceptance: "待接纳",
  waiting_confirmation: "待确认",
  failed: "执行失败",
  cancelled: "已取消",
  needs_reconciliation: "待核对",
};

function MaterialContent({
  material,
  ownerId,
  projectId,
  request,
}: {
  material: Material;
  ownerId: string;
  projectId: string;
  request: Request;
}) {
  const [expanded, setExpanded] = useState(false);
  const [content, setContent] = useState<Row>();
  const [loading, setLoading] = useState(false);
  const [error, setError] = useState("");
  const [attempt, setAttempt] = useState(0);
  useEffect(() => {
    if (!expanded || content) return;
    let active = true;
    setLoading(true);
    setError("");
    void request("query.workflow_materials", {
      project_id: projectId,
      owner_id: ownerId,
      output_id: material.id,
    })
      .then((value) => {
        if (active) setContent(value);
      })
      .catch((error) => {
        if (active) setError(String(error));
      })
      .finally(() => {
        if (active) setLoading(false);
      });
    return () => {
      active = false;
    };
  }, [expanded, content, request, projectId, ownerId, material.id, attempt]);
  return (
    <details
      className="workflow-material"
      onToggle={(event) => setExpanded(event.currentTarget.open)}
    >
      <summary>
        {material.delivery_name} · {material.step_name} · {material.port}
      </summary>
      <p className="muted">
        {material.owner_kind === "task"
          ? "Task"
          : material.owner_kind === "feature"
            ? "Feature"
            : "Project"}
        {" · "}
        {material.owner_title}
        {" · "}
        {material.stage_name || material.path}
        {" · "}
        {statuses[material.status] || material.status}
        {" · 第 "}
        {material.round}
        {" 轮"}
      </p>
      {loading && <p role="status">正在读取产物…</p>}
      {error && (
        <div role="alert">
          <p>{error}</p>
          <ActionButton onClick={() => setAttempt((value) => value + 1)}>
            重试读取
          </ActionButton>
        </div>
      )}
      {content && (
        <DeliveryOutput
          output={content.output}
          definitions={content.definitions}
        />
      )}
    </details>
  );
}

export function WorkflowMaterials({
  projectId,
  ownerId,
  request,
}: {
  projectId: string;
  ownerId: string;
  request: Request;
}) {
  const [draft, setDraft] = useState("");
  const [search, setSearch] = useState("");
  const [page, setPage] = useState(0);
  const [refresh, setRefresh] = useState(0);
  const [records, setRecords] = useState<Material[]>([]);
  const [total, setTotal] = useState(0);
  const [loading, setLoading] = useState(true);
  const [error, setError] = useState("");
  const pageSize = 20;
  useEffect(() => {
    let active = true;
    setLoading(true);
    setError("");
    setRecords([]);
    void request("query.workflow_materials", {
      project_id: projectId,
      owner_id: ownerId,
      search,
      offset: page * pageSize,
      limit: pageSize,
    })
      .then((value) => {
        if (!active) return;
        setRecords(value.records);
        setTotal(value.total);
        if (page > 0 && page * pageSize >= value.total)
          setPage(Math.max(0, Math.ceil(value.total / pageSize) - 1));
      })
      .catch((error) => {
        if (active) setError(String(error));
      })
      .finally(() => {
        if (active) setLoading(false);
      });
    return () => {
      active = false;
    };
  }, [request, projectId, ownerId, search, page, refresh]);
  return (
    <section aria-label="工作流产物" className="workflow-materials">
      <div className="panel-heading">
        <h3>工作流产物</h3>
        <ActionButton
          disabled={loading}
          onClick={() => {
            setPage(0);
            setRefresh((value) => value + 1);
          }}
        >
          刷新产物
        </ActionButton>
      </div>
      <p className="muted">
        汇总当前 Feature 和 Task 的交付物，可按类型、步骤或任务名称搜索。
      </p>
      <form
        className="material-search"
        onSubmit={(event) => {
          event.preventDefault();
          setSearch(draft.trim());
          setPage(0);
        }}
      >
        <label>
          搜索工作流产物
          <input
            value={draft}
            maxLength={200}
            onChange={(event) => setDraft(event.target.value)}
          />
        </label>
        <ActionButton type="submit" disabled={loading}>
          搜索产物
        </ActionButton>
      </form>
      {loading ? (
        <p role="status">正在读取工作流产物…</p>
      ) : error ? (
        <p role="alert" className="error">
          {error}
        </p>
      ) : (
        <>
          <p role="status">共 {total} 项工作流产物</p>
          {records.map((material) => (
            <MaterialContent
              key={material.id}
              material={material}
              projectId={projectId}
              ownerId={ownerId}
              request={request}
            />
          ))}
          {!records.length && (
            <p>{search ? "没有匹配的工作流产物。" : "当前工作流尚无产物。"}</p>
          )}
          {total > pageSize && (
            <div className="pager" aria-label="工作流产物分页">
              <ActionButton
                disabled={page === 0}
                onClick={() => setPage((value) => value - 1)}
              >
                上一页
              </ActionButton>
              <span>
                第 {page + 1} / {Math.ceil(total / pageSize)} 页
              </span>
              <ActionButton
                disabled={(page + 1) * pageSize >= total}
                onClick={() => setPage((value) => value + 1)}
              >
                下一页
              </ActionButton>
            </div>
          )}
        </>
      )}
    </section>
  );
}

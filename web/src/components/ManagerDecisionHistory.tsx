import { useEffect, useState } from "react";
import { displayText } from "../display-text";
import { ManagerDisclosure } from "./ManagerDisclosure";
import { ManagerInteraction } from "./ManagerInteraction";
import { type Request, type Row } from "./rpc-types";

type Scope = {
  request: Request;
  projectId: string;
  rootId: string;
  ownerId: string;
  revision?: number;
};

type HistoryItem = {
  id: string;
  title: string;
  step: string;
  status: string;
  created_at: number;
  signed_decision_id?: string;
  material_count?: number;
  material_labels: string[];
};

type HistoryPage = {
  records: HistoryItem[];
  next_decision_before?: number;
};

const statuses: Record<string, string> = {
  approved: "已确认",
  rejected: "已退回",
  withdrawn: "已撤回",
};

function DecisionDetail({
  request,
  projectId,
  rootId,
  ownerId,
  id,
}: Scope & { id: string }) {
  const [decision, setDecision] = useState<Row>();
  const [error, setError] = useState("");
  useEffect(() => {
    const controller = new AbortController();
    void request(
      "query.orchestration",
      { project_id: projectId, root_instance_id: rootId, view: "decision", id },
      { signal: controller.signal },
    ).then(
      (value) => {
        if (!controller.signal.aborted) setDecision(value);
      },
      (reason: unknown) => {
        if (!controller.signal.aborted) setError(String(reason));
      },
    );
    return () => controller.abort();
  }, [id, projectId, request, rootId]);

  if (error) return <p role="alert">{displayText(error)}</p>;
  if (!decision) return <p role="status">正在读取原决定…</p>;
  return (
    <ManagerInteraction
      interaction={{
        ...(decision.data.request.interaction || {}),
        signed_reason: decision.signed_reason,
        rework_budgets: decision.rework_budgets,
        supplemental_input:
          decision.data.request.operation === "workflow.step.input"
            ? decision.data.request.arguments.input
            : undefined,
        exception: decision.data.request.operation === "workflow.step.approve",
      }}
      projectId={projectId}
      ownerId={decision.scope?.task_id || decision.scope?.feature_id || ownerId}
      status={decision.data.status}
      busy={false}
      expanded
    />
  );
}

function History({ request, projectId, rootId, ownerId, revision }: Scope) {
  const [pages, setPages] = useState<number[]>([]);
  const [page, setPage] = useState<HistoryPage>();
  const [selected, setSelected] = useState<string>();
  const [error, setError] = useState("");
  const cursor = pages.at(-1);
  const latest = cursor === undefined ? revision : undefined;
  useEffect(() => {
    const controller = new AbortController();
    void request(
      "query.orchestration",
      {
        project_id: projectId,
        root_instance_id: rootId,
        view: "history",
        ...(cursor === undefined ? {} : { decision_before: cursor }),
      },
      { signal: controller.signal },
    ).then(
      (value) => {
        if (!controller.signal.aborted) setPage(value as HistoryPage);
      },
      (reason: unknown) => {
        if (!controller.signal.aborted) setError(String(reason));
      },
    );
    return () => controller.abort();
  }, [cursor, latest, projectId, request, rootId]);

  function go(next: number[]) {
    setPage(undefined);
    setSelected(undefined);
    setError("");
    setPages(next);
  }

  if (error) return <p role="alert">{displayText(error)}</p>;
  if (!page) return <p role="status">正在读取审批历史…</p>;
  return (
    <div className="manager-disclosure-body">
      {page.records.length === 0 && <p>暂无已处理的决定。</p>}
      <ul aria-label="已处理的决定">
        {page.records.map((item) => (
          <li key={item.id}>
            <button
              type="button"
              aria-expanded={item.id === selected}
              onClick={() =>
                setSelected((previous) =>
                  previous === item.id ? undefined : item.id,
                )
              }
            >
              {displayText(item.title || item.step || "历史决定")}
            </button>{" "}
            <span>
              {displayText(statuses[item.status] || item.status)} ·{" "}
              {new Date(item.created_at).toLocaleString()}
              {item.signed_decision_id ? " · 已签署" : ""}
              {item.material_count != null
                ? ` · ${item.material_count} 项材料`
                : ""}
            </span>
            {item.material_labels?.filter(Boolean).length > 0 && (
              <p>
                {displayText(item.material_labels.filter(Boolean).join("、"))}
              </p>
            )}
            {item.id === selected && (
              <DecisionDetail
                id={item.id}
                request={request}
                projectId={projectId}
                rootId={rootId}
                ownerId={ownerId}
              />
            )}
          </li>
        ))}
      </ul>
      {(pages.length > 0 || page.next_decision_before != null) && (
        <div className="actions">
          <button
            type="button"
            disabled={pages.length === 0}
            onClick={() => go(pages.slice(0, -1))}
          >
            上一页历史
          </button>
          <button
            type="button"
            disabled={page.next_decision_before == null}
            onClick={() => go([...pages, page.next_decision_before!])}
          >
            下一页历史
          </button>
        </div>
      )}
    </div>
  );
}

export function ManagerDecisionHistory(scope: Scope) {
  const [opened, setOpened] = useState(false);
  return (
    <ManagerDisclosure
      className="manager-disclosure"
      onToggle={(event) => setOpened(event.currentTarget.open)}
    >
      <summary>审批历史</summary>
      {opened && <History {...scope} />}
    </ManagerDisclosure>
  );
}

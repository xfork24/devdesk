import { displayText } from "../display-text";
import { useEffect, useState } from "react";
import { ActionButton } from "./ActionFeedback";
import { DeliveryOutput } from "./DeliveryContent";
import { MarkdownPreview } from "./configuration-ui";
import { type Request, type Row } from "./rpc-types";
import { RawJsonLink, ReadState } from "./reading";
import { useReading, navigateReading, executionUrl } from "./reading-state";
import { WorkflowReferences } from "./WorkflowReferences";
import { materialTitle } from "./workflow-material";

function MaterialReader({
  projectId,
  ownerId,
  outputId,
  request,
}: {
  projectId: string;
  ownerId: string;
  outputId: string;
  request: Request;
}) {
  const state = useReading(request, "query.workflow_materials", {
    project_id: projectId,
    owner_id: ownerId,
    output_id: outputId,
  });
  return (
    <article
      className="reading-body"
      aria-label="选中材料正文"
      id={`material-${outputId}`}
    >
      <ReadState {...state} />
      {state.value?.document ? (
        <>
          <h3>{displayText(state.value.document.title)}</h3>
          <p>手工材料 · 修订 {displayText(state.value.document.version)}</p>
          <MarkdownPreview text={state.value.document.body} />
          <RawJsonLink
            source="material"
            params={{
              project_id: projectId,
              owner_id: ownerId,
              output_id: outputId,
            }}
          />
        </>
      ) : (
        state.value && (
          <>
            <h3>
              {displayText(
                materialTitle(
                  state.value.record.delivery_name,
                  state.value.record.step_name,
                ),
              )}
            </h3>
            <p className="muted">
              {displayText(state.value.record.owner_title)} ·{" "}
              {displayText(state.value.record.step_name)} · 第{" "}
              {displayText(state.value.record.round)} 轮 ·{" "}
              {displayText(state.value.record.status)}
            </p>
            {state.value.record.historical && (
              <p className="muted">历史执行产物 · 保留产生时的来源与规范</p>
            )}
            <WorkflowReferences
              source={{ ...state.value.record, output_id: outputId }}
              {...{ projectId, ownerId }}
              current="documents"
            />
            <RawJsonLink
              source="material"
              params={{
                project_id: projectId,
                owner_id: ownerId,
                output_id: outputId,
              }}
            />
            <DeliveryOutput
              output={state.value.output}
              definitions={state.value.definitions}
            />
          </>
        )
      )}
    </article>
  );
}
export function WorkflowMaterials({
  projectId,
  ownerId,
  request,
  target = {},
  documents = [],
}: {
  projectId: string;
  ownerId: string;
  request: Request;
  target?: Row;
  documents?: Row[];
}) {
  const [draft, setDraft] = useState(""),
    [search, setSearch] = useState(""),
    [page, setPage] = useState(0);
  const state = useReading(request, "query.workflow_materials", {
    project_id: projectId,
    owner_id: ownerId,
    search,
    execution_id: target.execution_id || undefined,
    offset: page * 20,
    limit: 20,
  });
  useEffect(() => {
    setPage(0);
    setSearch("");
    setDraft("");
  }, [target.execution_id]);
  const records: Row[] = state.value?.records || [];
  const manualDocuments: Row[] = state.value?.documents || documents;
  const selected =
    target.output_id || records[0]?.id || manualDocuments[0]?.id || "";
  const manual = manualDocuments.find((d) => d.id === selected);
  const groups = new Map<string, Row[]>();
  for (const item of records) {
    const key = item.delivery_ref;
    groups.set(key, [...(groups.get(key) || []), item]);
  }
  useEffect(() => {
    if (selected)
      document
        .getElementById(`material-${selected}`)
        ?.scrollIntoView({ block: "nearest" });
  }, [selected]);
  function select(id: string) {
    navigateReading(
      executionUrl("documents", projectId, ownerId, {
        output_id: id,
        execution_id: target.execution_id,
      }),
    );
  }
  return (
    <section aria-label="工作流产物" className="workflow-materials">
      <div className="panel-heading">
        <div>
          <h3>材料目录</h3>
          <p className="muted">
            当前范围内的流程产物与历史材料；每份材料可追溯到产生它的步骤和上下文。
          </p>
        </div>
        <ActionButton onClick={state.retry} disabled={state.loading}>
          刷新产物
        </ActionButton>
      </div>
      {target.execution_id && (
        <p>
          正在查看指定执行的材料。{" "}
          <a
            href={executionUrl("documents", projectId, ownerId)}
            onClick={(e) => {
              e.preventDefault();
              navigateReading(executionUrl("documents", projectId, ownerId));
            }}
          >
            返回全部材料
          </a>
        </p>
      )}
      <form
        className="material-search"
        onSubmit={(e) => {
          e.preventDefault();
          setSearch(draft.trim());
          setPage(0);
        }}
      >
        <label>
          搜索工作流产物
          <input
            value={draft}
            maxLength={200}
            onChange={(e) => setDraft(e.target.value)}
          />
        </label>
        <ActionButton type="submit" disabled={state.loading}>
          搜索产物
        </ActionButton>
      </form>
      <div className="reading-layout">
        <nav className="reading-directory" aria-label="材料目录">
          <ReadState {...state} />
          {state.value && (
            <p role="status">
              共 {displayText(state.value.total)} 项工作流产物
            </p>
          )}
          {[...groups].map(([key, items]) => (
            <section key={key}>
              <h4>
                {displayText(
                  materialTitle(items[0].delivery_name, items[0].step_name),
                )}
              </h4>
              {items.map((item) => (
                <div key={item.id}>
                  <button
                    aria-current={selected === item.id}
                    onClick={() => select(item.id)}
                  >
                    {displayText(
                      materialTitle(item.delivery_name, item.step_name),
                    )}
                    <small>
                      {displayText(item.owner_title)} ·{" "}
                      {displayText(item.step_name)} · 第{" "}
                      {displayText(item.round)} 轮
                      {displayText(item.historical ? " · 历史产物" : "")}
                    </small>
                  </button>
                </div>
              ))}
            </section>
          ))}
          {manualDocuments.length > 0 && (
            <section>
              <h4>手工材料修订</h4>
              {manualDocuments.map((doc) => (
                <button
                  key={doc.id}
                  aria-current={selected === doc.id}
                  onClick={() => select(doc.id)}
                >
                  {displayText(doc.title)}
                  <small>修订 {displayText(doc.version)}</small>
                </button>
              ))}
            </section>
          )}
          {state.value && !records.length && (
            <p>
              {displayText(
                search ? "没有匹配的工作流产物。" : "当前范围尚无产物。",
              )}
            </p>
          )}
          {(state.value?.total || 0) > 20 && (
            <div className="pager">
              <ActionButton
                disabled={!page}
                onClick={() => setPage((p) => p - 1)}
              >
                上一页
              </ActionButton>
              <span>第 {page + 1} 页</span>
              <ActionButton
                disabled={(page + 1) * 20 >= state.value!.total}
                onClick={() => setPage((p) => p + 1)}
              >
                下一页
              </ActionButton>
            </div>
          )}
        </nav>
        {manual ? (
          <article className="reading-body" id={`material-${selected}`}>
            <h3>{displayText(manual.title)}</h3>
            <p className="muted">
              手工材料 · 修订 {displayText(manual.version)}
            </p>
            <MarkdownPreview text={manual.body} />
            <RawJsonLink
              source="material"
              params={{
                project_id: projectId,
                owner_id: ownerId,
                output_id: selected,
              }}
            />
          </article>
        ) : selected ? (
          <MaterialReader
            key={`${projectId}:${ownerId}:${selected}`}
            {...{ projectId, ownerId, request }}
            outputId={selected}
          />
        ) : (
          <p className="muted">
            从目录选择材料，或从工作流程的“查看材料”进入。
          </p>
        )}
      </div>
    </section>
  );
}

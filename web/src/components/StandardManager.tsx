import { useEffect, useRef, useState } from "react";
import { ActionButton, useActionFeedback } from "./ActionFeedback";
import {
  loadCatalog,
  MarkdownPreview,
  RevisionSelect,
  type Request,
  type Row,
} from "./configuration-ui";

export function StandardDiff({ diff }: { diff: Row }) {
  return (
    <div role="status" className="standard-diff">
      <p>
        {diff.equal
          ? "规范定义相同"
          : `${diff.changes?.length || 0} 项规范变化`}
      </p>
      {(diff.changes || []).map((change: Row, index: number) => (
        <details key={index}>
          <summary>
            {(
              {
                added: "新增",
                removed: "移除",
                definition_changed: "定义变化",
                reference_changed: "引用版本变化",
                relationship_added: "新增关联",
                relationship_removed: "移除关联",
                entries_changed: "入口变化",
              } as Row
            )[change.kind] || change.kind}{" "}
            · {change.source_kind || change.edge?.relation || "Workflow"}{" "}
            {change.before_ref || change.after_ref || change.edge?.path || ""}
          </summary>
          {(change.fields || []).map((field: Row) => (
            <div key={field.path}>
              <strong>{field.path}</strong>
              <div className="standard-comparison">
                <pre>{JSON.stringify(field.before, null, 2)}</pre>
                <pre>{JSON.stringify(field.after, null, 2)}</pre>
              </div>
            </div>
          ))}
          {!change.fields?.length ? (
            <pre>{JSON.stringify(change, null, 2)}</pre>
          ) : null}
        </details>
      ))}
    </div>
  );
}

export function StandardDetail({ snapshot }: { snapshot: Row }) {
  const sourceName = (reference: string) =>
    snapshot.nodes.find((node: Row) => node.reference === reference)?.name ||
    reference;
  const ports = (values: Row = {}) => (
    <ul>
      {Object.entries(values).map(([name, value]) => (
        <li key={name}>
          {name} → {sourceName((value as Row).delivery_ref)}
          {(value as Row).content_checks ? " · 内容检查" : ""}
        </li>
      ))}
    </ul>
  );
  return (
    <div className="standard-detail">
      <p>
        不可变规范快照 · {snapshot.total_nodes ?? snapshot.nodes.length}{" "}
        个源修订 · {snapshot.total_edges ?? snapshot.edges.length} 条关联
      </p>
      <p className="muted">
        定义摘要：<code>{snapshot.snapshot_digest}</code>
      </p>
      <h4>Workflow 入口</h4>
      <ul>
        {snapshot.root_workflow_refs.map((reference: string) => (
          <li key={reference}>
            {snapshot.nodes.find((n: Row) => n.reference === reference)?.name ||
              reference}{" "}
            · <code>{reference}</code>
          </li>
        ))}
      </ul>
      {snapshot.nodes.map((node: Row) => (
        <details key={node.reference}>
          <summary>
            {node.kind === "delivery_type"
              ? "Delivery"
              : node.kind === "workflow"
                ? "Workflow"
                : "Spec"}{" "}
            · {node.name} · {node.reference}
          </summary>
          {node.kind === "spec" ? (
            <>
              <MarkdownPreview text={node.content.body} />
              {(node.content.checks || []).map((check: Row) => (
                <div key={check.key}>
                  <strong>{check.name}</strong>
                  <MarkdownPreview text={check.instructions} />
                </div>
              ))}
            </>
          ) : node.kind === "workflow" ? (
            <>
              {Object.keys(node.content.inputs || {}).length ? (
                <>
                  <strong>流程输入</strong>
                  {ports(node.content.inputs)}
                </>
              ) : null}
              {(node.content.stages || []).map((stage: Row) => (
                <section key={stage.key}>
                  <h4>{stage.name}</h4>
                  {stage.description ? (
                    <MarkdownPreview text={stage.description} />
                  ) : null}
                  {node.content.steps
                    .filter((step: Row) => step.stage === stage.key)
                    .map((step: Row) => (
                      <details key={step.node_id || step.key}>
                        <summary>
                          {step.name} ·{" "}
                          {
                            (
                              {
                                agent: "Agent",
                                tool: "工具",
                                workflow: "子流程",
                                control: "控制",
                              } as Row
                            )[step.executor.type]
                          }
                        </summary>
                        {step.task_instructions ? (
                          <MarkdownPreview text={step.task_instructions} />
                        ) : null}
                        {step.dependencies?.length ? (
                          <p>前置步骤：{step.dependencies.join("、")}</p>
                        ) : null}
                        {step.executor.workflow_ref ? (
                          <p>
                            调用：{sourceName(step.executor.workflow_ref)}
                            {step.executor.work_binding
                              ? " · 按工作已采用的规范选择"
                              : ""}
                          </p>
                        ) : null}
                        {step.spec_refs?.length ? (
                          <p>
                            检查规范：
                            {step.spec_refs.map(sourceName).join("、")}
                          </p>
                        ) : null}
                        {ports(step.outputs)}
                        <details>
                          <summary>推进与确认条件</summary>
                          <pre>
                            {JSON.stringify(
                              {
                                condition: step.condition,
                                completion: step.completion,
                                confirmation: step.confirmation,
                                budget: step.budget,
                              },
                              null,
                              2,
                            )}
                          </pre>
                        </details>
                      </details>
                    ))}
                </section>
              ))}
            </>
          ) : (
            <>
              <p>格式：{node.content.format}</p>
              <p>
                内容规范：
                {node.content.spec_refs?.map(sourceName).join("、") || "未关联"}
              </p>
              {node.content.content_schema ? (
                <details>
                  <summary>结构合同</summary>
                  <pre>
                    {JSON.stringify(node.content.content_schema, null, 2)}
                  </pre>
                </details>
              ) : null}
            </>
          )}
          <ul>
            {snapshot.edges
              .filter((edge: Row) => edge.from === node.reference)
              .map((edge: Row) => (
                <li key={`${edge.path}:${edge.to}`}>
                  {edge.path} → {edge.to} · {edge.relation}
                </li>
              ))}
          </ul>
        </details>
      ))}
    </div>
  );
}

export function StandardManager({
  request,
  project,
}: {
  request: Request;
  project?: Row;
}) {
  const [records, setRecords] = useState<Row[]>([]);
  const [selected, setSelected] = useState(
    () => new URLSearchParams(location.search).get("standard_id") || "",
  );
  const [snapshot, setSnapshot] = useState<Row>();
  const [usage, setUsage] = useState<Row>();
  const [name, setName] = useState("");
  const [roots, setRoots] = useState<string[]>([]);
  const [candidate, setCandidate] = useState<Row>();
  const [base, setBase] = useState("");
  const [compare, setCompare] = useState("");
  const [diff, setDiff] = useState<Row>();
  const [busy, setBusy] = useState(false);
  const { error, setError, message, setMessage } = useActionFeedback();
  const epoch = useRef(0);
  async function load() {
    setRecords(await loadCatalog(request, project, true));
  }
  useEffect(() => {
    let active = true;
    void loadCatalog(request, project, true)
      .then((rows) => {
        if (active) setRecords(rows);
      })
      .catch((e) => {
        if (active) setError(String(e));
      });
    return () => {
      active = false;
    };
  }, [request, project?.id]);
  useEffect(() => {
    const current = ++epoch.current;
    setSnapshot(undefined);
    setUsage(undefined);
    setDiff(undefined);
    if (!selected) return;
    void Promise.all([
      request("query.standard", {
        id: selected,
        project_id: project?.id,
        limit: 1000,
      }),
      request("query.standard_usage", {
        id: selected,
        project_id: project?.id,
      }),
    ])
      .then(([value, uses]) => {
        if (epoch.current === current) {
          setSnapshot(value);
          setUsage(uses);
        }
      })
      .catch((e) => {
        if (epoch.current === current) setError(String(e));
      });
    return () => {
      epoch.current++;
    };
  }, [selected, project?.id, request]);
  async function act(action: () => Promise<void>) {
    setBusy(true);
    setError("");
    try {
      await action();
    } catch (e) {
      setError(String(e));
    } finally {
      setBusy(false);
    }
  }
  const standards = records.filter(
    (r) => r.kind === "standard" && r.data.schema_version === 4,
  );
  return (
    <section className="panel" aria-label="Standard 规范快照">
      <h3>Standard 规范快照</h3>
      <p>
        冻结 Workflow、Delivery、Spec
        及其关联。工作可以采用同一个快照，也可以选择不同修订。
      </p>
      {error ? (
        <p className="error" role="alert">
          {error}
        </p>
      ) : null}
      {message ? <p role="status">{message}</p> : null}
      <RevisionSelect
        label="查看 Standard"
        records={standards}
        value={selected}
        onChange={(value) => {
          setSelected(value);
          const params = new URLSearchParams(location.search);
          if (value) params.set("standard_id", value);
          else params.delete("standard_id");
          history.replaceState(null, "", `?${params}`);
        }}
      />
      <details>
        <summary>从 Workflow 生成快照</summary>
        <label>
          快照名称
          <input value={name} onChange={(e) => setName(e.target.value)} />
        </label>
        <RevisionSelect
          label="入口 Workflow"
          multiple
          records={records.filter((r) => r.kind === "workflow")}
          value={roots}
          onChange={(value) => {
            setRoots(value);
            setCandidate(undefined);
          }}
        />
        <p className="muted">
          只选择已发布入口；Delivery、Spec 和子流程由系统解析。
        </p>
        <ActionButton
          disabled={busy || !roots.length}
          onClick={() =>
            void act(async () => {
              setCandidate(
                await request("standard.capture.preview", {
                  root_workflow_refs: roots,
                  project_id: project?.id,
                }),
              );
            })
          }
        >
          预览规范快照
        </ActionButton>
        {candidate ? (
          <>
            <StandardDetail snapshot={candidate} />
            <ActionButton
              disabled={busy || !name.trim()}
              onClick={() =>
                void act(async () => {
                  const previous = records.find((r) => r.id === base);
                  const row = await request("standard.capture", {
                    project_id: project?.id,
                    command_id: crypto.randomUUID(),
                    target: base || undefined,
                    expected_revision: previous?.revision,
                    payload: {
                      name,
                      root_workflow_refs: roots,
                      preview_digest: candidate.snapshot_digest,
                      scope: project
                        ? { kind: "project", project_id: project.id }
                        : { kind: "global" },
                    },
                  });
                  await load();
                  setSelected(row.id);
                  setCandidate(undefined);
                  setBase("");
                  setMessage("不可变规范快照已生成");
                })
              }
            >
              生成快照
            </ActionButton>
          </>
        ) : null}
      </details>
      {snapshot ? (
        <>
          <h3>
            {snapshot.record.name} · v{snapshot.record.version}
            {snapshot.archived ? " · 已归档" : ""}
          </h3>
          <ActionButton
            disabled={
              busy ||
              (project
                ? snapshot.record.scope.kind !== "project"
                : snapshot.record.scope.kind !== "global")
            }
            onClick={() =>
              void act(async () => {
                await request("standard.archive", {
                  project_id: project?.id,
                  command_id: crypto.randomUUID(),
                  target: selected,
                  expected_revision: snapshot.record.revision,
                  payload: { archived: !snapshot.archived },
                });
                setSnapshot({ ...snapshot, archived: !snapshot.archived });
                await load();
              })
            }
          >
            {snapshot.archived ? "恢复快照选择" : "归档快照"}
          </ActionButton>
          <StandardDetail snapshot={snapshot} />
          <ActionButton
            disabled={
              busy ||
              (project
                ? snapshot.record.scope.kind !== "project"
                : snapshot.record.scope.kind !== "global")
            }
            onClick={() => {
              setRoots(snapshot.root_workflow_refs);
              setName(snapshot.record.name);
              setBase(selected);
              setCandidate(undefined);
              setMessage("已准备入口选择，请展开生成快照并预览新修订。");
            }}
          >
            基于这些入口生成新修订
          </ActionButton>
          <RevisionSelect
            label="与另一个 Standard 比较"
            records={standards}
            value={compare}
            onChange={(value) => {
              setCompare(value);
              setDiff(undefined);
            }}
          />
          <ActionButton
            disabled={busy || !compare}
            onClick={() =>
              void act(async () => {
                setDiff(
                  await request("query.standard_diff", {
                    before_ref: selected,
                    after_ref: compare,
                    project_id: project?.id,
                  }),
                );
              })
            }
          >
            比较规范
          </ActionButton>
          {diff ? <StandardDiff diff={diff} /> : null}
          <details>
            <summary>使用位置（{usage?.total || 0}）</summary>
            <ul>
              {usage?.records.map((row: Row) => (
                <li key={row.id}>
                  {row.kind} ·{" "}
                  {row.data.owner_id || row.data.work?.id || row.definition_id}{" "}
                  · {row.data.workflow_ref}
                </li>
              ))}
            </ul>
          </details>
        </>
      ) : (
        <p className="muted">选择快照查看确切规范与关系。</p>
      )}
    </section>
  );
}

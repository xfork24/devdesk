import { displayText } from "../display-text";
import { useReadRequest, ignoreCancelledRefresh } from "./use-read-request";
import { useEffect, useRef, useState, type ReactNode } from "react";
import { ActionButton } from "./ActionFeedback";
import { useActionFeedback } from "./action-feedback";
import { latest, loadCatalogWithStandardHistory } from "./configuration-state";
import { MarkdownPreview, RevisionSelect } from "./configuration-ui";
import { type Request, type Row } from "./rpc-types";

import { StandardDiff } from "./StandardDiff";

type StandardDetailProps = {
  snapshot: Row;
  renderOverride?: (node: Row) => ReactNode;
};

export function StandardDetail({
  snapshot,
  renderOverride,
}: StandardDetailProps) {
  const sourceName = (reference: string) => {
    const node = snapshot.nodes.find(
      (node: Row) => node.reference === reference,
    );
    return node ? `${node.name} · v${reference.split(":").at(-1)}` : reference;
  };
  const ports = (values: Row = {}) => (
    <ul>
      {Object.entries(values).map(([name, value]) => (
        <li key={name}>
          {displayText(name)} →{" "}
          {displayText(sourceName((value as Row).delivery_ref))}
          {displayText((value as Row).content_checks ? " · 内容检查" : "")}
        </li>
      ))}
    </ul>
  );
  return (
    <div className="standard-detail">
      <p>
        不可变规范快照 ·{" "}
        {displayText(snapshot.total_nodes ?? snapshot.nodes.length)} 个源修订 ·{" "}
        {displayText(snapshot.total_edges ?? snapshot.edges.length)} 条关联
      </p>
      <p className="muted">
        定义摘要：<code>{displayText(snapshot.snapshot_digest)}</code>
      </p>
      <h4>Workflow 入口</h4>
      <ul>
        {snapshot.root_workflow_refs.map((reference: string) => (
          <li key={reference}>{displayText(sourceName(reference))}</li>
        ))}
      </ul>
      {snapshot.nodes.map((node: Row) => (
        <section
          className="standard-source"
          key={node.reference}
          aria-label={`${node.name} · ${node.reference}`}
        >
          <details>
            <summary>
              {displayText(
                node.kind === "delivery_type"
                  ? "Delivery"
                  : node.kind === "workflow"
                    ? "Workflow"
                    : "Spec",
              )}{" "}
              · {displayText(sourceName(node.reference))}
            </summary>
            <p className="muted">
              <code>{displayText(node.reference)}</code>
            </p>
            {node.kind === "spec" ? (
              <>
                <MarkdownPreview text={node.content.body} />
                {(node.content.checks || []).map((check: Row) => (
                  <div key={check.key}>
                    <strong>{displayText(check.name)}</strong>
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
                    <h4>{displayText(stage.name)}</h4>
                    {stage.description ? (
                      <MarkdownPreview text={stage.description} />
                    ) : null}
                    {node.content.steps
                      .filter((step: Row) => step.stage === stage.key)
                      .map((step: Row) => (
                        <details key={step.node_id || step.key}>
                          <summary>
                            {displayText(step.name)} ·{" "}
                            {displayText(
                              (
                                {
                                  agent: "Agent",
                                  tool: "工具",
                                  workflow: "子流程",
                                  control: "控制",
                                } as Row
                              )[step.executor.type],
                            )}
                          </summary>
                          {step.task_instructions ? (
                            <MarkdownPreview text={step.task_instructions} />
                          ) : null}
                          {step.dependencies?.length ? (
                            <p>
                              前置步骤：
                              {displayText(step.dependencies.join("、"))}
                            </p>
                          ) : null}
                          {step.executor.workflow_ref ? (
                            <p>
                              调用：
                              {displayText(
                                sourceName(step.executor.workflow_ref),
                              )}
                              {displayText(
                                step.executor.work_binding
                                  ? " · 按工作已采用的规范选择"
                                  : "",
                              )}
                            </p>
                          ) : null}
                          {step.spec_refs?.length ? (
                            <p>
                              检查规范：
                              {displayText(
                                step.spec_refs.map(sourceName).join("、"),
                              )}
                            </p>
                          ) : null}
                          {ports(step.outputs)}
                          <details>
                            <summary>推进与确认条件</summary>
                            <pre>
                              {displayText(
                                JSON.stringify(
                                  {
                                    condition: step.condition,
                                    completion: step.completion,
                                    confirmation: step.confirmation,
                                    budget: step.budget,
                                  },
                                  null,
                                  2,
                                ),
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
                <p>格式：{displayText(node.content.format)}</p>
                <p>
                  内容规范：
                  {displayText(
                    node.content.spec_refs?.map(sourceName).join("、") ||
                      "未关联",
                  )}
                </p>
                {node.content.content_schema ? (
                  <details>
                    <summary>结构合同</summary>
                    <pre>
                      {displayText(
                        JSON.stringify(node.content.content_schema, null, 2),
                      )}
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
                    {displayText(edge.path)} → {displayText(edge.to)} ·{" "}
                    {displayText(edge.relation)}
                  </li>
                ))}
            </ul>
            {renderOverride?.(node)}
          </details>
        </section>
      ))}
    </div>
  );
}

type StandardRevisionEditorProps = {
  snapshot: Row;
  records: Row[];
  request: Request;
  projectId?: string;
  onCreated: (record: Row) => Promise<void>;
};

function StandardRevisionEditor({
  snapshot,
  records,
  request: parentRequest,
  projectId,
  onCreated,
}: StandardRevisionEditorProps) {
  const request = useReadRequest(parentRequest);
  const [name, setName] = useState(snapshot.record.name as string);
  const [overrides, setOverrides] = useState<Record<string, string>>({});
  const [candidate, setCandidate] = useState<Row>();
  const [busy, setBusy] = useState(false);
  const [error, setError] = useState("");
  const generation = useRef(0);
  const scope = projectId ? `project:${projectId}` : "global";
  const head = latest(records.filter((row) => row.type === "standard")).find(
    (row) => row.definition_id === snapshot.record.definition_id,
  );
  const revising =
    snapshot.record.scope === scope &&
    !snapshot.archived &&
    head?.id === snapshot.record.id;
  const substitutions = Object.entries(overrides).map(
    ([source_ref, replacement_ref]) => ({
      source_ref,
      replacement_ref,
    }),
  );
  async function preview() {
    const ticket = ++generation.current;
    setBusy(true);
    setError("");
    try {
      const value = await request("standard.capture.preview", {
        project_id: projectId,
        base_standard_ref: snapshot.record.id,
        standard_overrides: substitutions,
      });
      if (ticket === generation.current) setCandidate(value);
    } catch (e) {
      if (ticket === generation.current) setError(String(e));
    } finally {
      setBusy(false);
    }
  }
  async function save() {
    if (!candidate) return;
    setBusy(true);
    setError("");
    try {
      const row = await request("standard.capture", {
        project_id: projectId,
        command_id: crypto.randomUUID(),
        target: revising ? snapshot.record.id : undefined,
        expected_revision: revising ? snapshot.record.revision : undefined,
        payload: {
          name: name.trim(),
          base_standard_ref: snapshot.record.id,
          standard_overrides: substitutions,
          preview_digest: candidate.snapshot_digest,
          scope,
        },
      });
      await onCreated(row);
    } catch (e) {
      setError(String(e));
    } finally {
      setBusy(false);
    }
  }
  return (
    <fieldset className="standard-revision-editor" disabled={busy}>
      <legend>替换配置并生成新版本</legend>
      <p className="muted">
        在配置项旁选择要替换的已发布修订，同一项在快照中的所有引用同步替换。
      </p>
      <StandardDetail
        snapshot={snapshot}
        renderOverride={(node) => (
          <div className="standard-source-override">
            <RevisionSelect
              label="替换为"
              records={records.filter(
                (row) =>
                  row.type === node.kind &&
                  row.id !== node.reference &&
                  !row.archived &&
                  !row.template_archived,
              )}
              value={overrides[node.reference] || ""}
              onChange={(replacement) => {
                generation.current++;
                setCandidate(undefined);
                setError("");
                setOverrides((current) => {
                  const next = { ...current };
                  if (replacement) next[node.reference] = replacement;
                  else delete next[node.reference];
                  return next;
                });
              }}
            />
          </div>
        )}
      />
      <section
        className="standard-revision-save"
        aria-label="生成 Standard 新版本"
      >
        <h4>生成新版本</h4>
        <label>
          快照名称
          <input
            required
            value={name}
            onChange={(event) => setName(event.target.value)}
          />
        </label>
        <p>
          已替换 {substitutions.length} 项 ·{" "}
          {revising
            ? `保存为当前 Standard 的 v${snapshot.record.version + 1}`
            : projectId
              ? "保存为新的项目 Standard · v1"
              : "保存为新的 Standard · v1"}
        </p>
        {error ? (
          <p role="alert" className="error">
            {displayText(error)}
          </p>
        ) : null}
        <div className="actions">
          <ActionButton
            disabled={!substitutions.length}
            onClick={() => void preview()}
          >
            预览替换
          </ActionButton>
          <ActionButton
            className="primary"
            disabled={!candidate || !name.trim()}
            onClick={() => void save()}
          >
            生成新版本
          </ActionButton>
        </div>
        {candidate ? (
          <>
            <StandardDiff diff={candidate.provenance.diff} />
            <details>
              <summary>查看新版本快照</summary>
              <StandardDetail snapshot={candidate} />
            </details>
          </>
        ) : null}
      </section>
    </fieldset>
  );
}

export function StandardManager({
  request: parentRequest,
  project,
}: {
  request: Request;
  project?: Row;
}) {
  const request = useReadRequest(parentRequest);
  const [records, setRecords] = useState<Row[]>([]);
  const [catalogLoaded, setCatalogLoaded] = useState(false);
  const initialized = useRef(false);
  const [selected, setSelected] = useState(
    () => new URLSearchParams(location.search).get("standard_id") || "",
  );
  const [snapshot, setSnapshot] = useState<Row>();
  const [usage, setUsage] = useState<Row>();
  const [showHistory, setShowHistory] = useState(
    () =>
      new URLSearchParams(location.search).get("standard_history") === "true",
  );
  const [compare, setCompare] = useState("");
  const [diff, setDiff] = useState<Row>();
  const [busy, setBusy] = useState(false);
  const { error, setError, message, setMessage } = useActionFeedback();

  async function load() {
    setRecords(
      await loadCatalogWithStandardHistory(request, project?.id, true),
    );
  }
  useEffect(() => {
    let active = true;
    void loadCatalogWithStandardHistory(request, project?.id, true)
      .then((rows) => {
        if (active) {
          setRecords(rows);
          setCatalogLoaded(true);
        }
      })
      .catch((e) => {
        if (active) setError(String(e));
      });
    return () => {
      active = false;
    };
  }, [request, project?.id, setError]);
  useEffect(() => {
    if (initialized.current || !catalogLoaded) return;
    initialized.current = true;
    if (
      !records.some((row) => row.type === "standard" && row.id === selected)
    ) {
      const first = latest(
        records.filter(
          (row) =>
            row.type === "standard" && !row.template_archived && !row.archived,
        ),
      )[0];
      setSelected(first?.id || "");
    } else if (
      records.some(
        (row) => row.id === selected && (row.template_archived || row.archived),
      )
    ) {
      setShowHistory(true);
    }
  }, [records, selected, catalogLoaded]);
  useEffect(() => {
    const params = new URLSearchParams(location.search);
    if (selected) params.set("standard_id", selected);
    else params.delete("standard_id");
    if (showHistory) params.set("standard_history", "true");
    else params.delete("standard_history");
    history.replaceState(null, "", `?${params}`);
  }, [selected, showHistory]);
  useEffect(() => {
    setSnapshot(undefined);
    setUsage(undefined);
    setDiff(undefined);
    setError("");
  }, [selected, project?.id, setError]);
  const selectedAvailable = records.some(
    (row) => row.type === "standard" && row.id === selected,
  );
  useEffect(() => {
    let active = true;
    if (!selectedAvailable) return;
    const controller = new AbortController();
    void request(
      "query.standard",
      { id: selected, project_id: project?.id, limit: 1000 },
      { signal: controller.signal },
    )
      .then((value) => {
        if (active) setSnapshot(value);
      })
      .catch((error) => {
        if (active) setError(String(error));
      });
    void request(
      "query.standard_usage",
      { id: selected, project_id: project?.id },
      { signal: controller.signal },
    )
      .then((value) => {
        if (active) setUsage(value);
      })
      .catch((error) => {
        if (active) setError(String(error));
      });
    return () => {
      active = false;
      controller.abort();
    };
  }, [selected, selectedAvailable, project?.id, request, setError]);
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
  const allStandards = records.filter((r) => r.type === "standard");
  const currentStandards = latest(
    allStandards.filter((r) => !r.template_archived && !r.archived),
  );
  const standards = showHistory ? allStandards : currentStandards;
  const selectedRecord = allStandards.find((row) => row.id === selected);
  const choices =
    selectedRecord && !standards.some((row) => row.id === selected)
      ? [...standards, selectedRecord]
      : standards;
  return (
    <section className="panel standard-manager" aria-label="Standard 规范快照">
      <h3>Standard 规范快照</h3>
      <p>
        冻结 Workflow、Delivery、Spec
        及其关联。工作可以采用同一个快照，也可以选择不同修订。
      </p>
      {error ? (
        <p className="error" role="alert">
          {displayText(error)}
        </p>
      ) : null}
      {message ? <p role="status">{displayText(message)}</p> : null}
      <RevisionSelect
        label="查看 Standard"
        records={choices}
        value={selected}
        onChange={(value) => {
          setSelected(value);
          const params = new URLSearchParams(location.search);
          if (value) params.set("standard_id", value);
          else params.delete("standard_id");
          history.replaceState(null, "", `?${params}`);
        }}
      />
      <label className="standard-history-toggle">
        <input
          type="checkbox"
          checked={showHistory}
          onChange={(event) => {
            const show = event.target.checked;
            setShowHistory(show);
            if (!show && !currentStandards.some((row) => row.id === selected))
              setSelected(currentStandards[0]?.id || "");
          }}
        />
        显示历史与归档快照
      </label>
      {snapshot ? (
        <>
          {snapshot.archived ? (
            <p className="notice">
              此快照已归档，可查看历史内容或基于它生成新快照。
            </p>
          ) : null}
          <StandardRevisionEditor
            key={snapshot.record.id}
            snapshot={snapshot}
            records={records}
            request={request}
            projectId={project?.id}
            onCreated={async (row) => {
              await load().catch(ignoreCancelledRefresh);
              setSelected(row.id);
              setMessage("Standard 新版本已生成");
            }}
          />
          <section className="standard-compare" aria-label="Standard 比较">
            <div className="standard-compare-controls">
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
            </div>
            {diff ? <StandardDiff diff={diff} /> : null}
          </section>
          <details>
            <summary>
              使用位置 · 当前 Standard：{displayText(snapshot.record.name)} · v
              {displayText(snapshot.record.version)}（
              {displayText(usage?.total || 0)}）
            </summary>
            <ul>
              {usage?.records.map((row: Row) => (
                <li key={row.id}>
                  {displayText(row.type)} ·{" "}
                  {displayText(
                    row.data.owner_id || row.data.work?.id || row.definition_id,
                  )}{" "}
                  · {displayText(row.data.workflow_ref)}
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

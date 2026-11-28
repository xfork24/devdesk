import { displayText, shortId } from "../sdk/display-text";
import { ActionButton } from "../sdk/ActionFeedback";
import { useActionFeedback } from "../sdk/action-feedback";
import { useEffect, useState } from "react";
import { MarkdownPreview } from "./configuration-ui";
import { type Request, type Row } from "../sdk/rpc-types";
import { experienceKey } from "./experience-reference";

const pageSize = 10;
const experienceReferenceLimit = 20;

type Props = {
  revision?: string;
  request: Request;
  projectId: string;
  roleId: string;
  currentInstance?: string;
  owner: Row;
  onRestored: (id: string) => void;
  onLocate?: (entry: Row) => void;
  references: Row[];
  onReference?: (entry: Row) => void;
  onDeleted: (entry: Row) => void;
};
export function RoleExperience({
  request,
  revision,
  projectId,
  roleId,
  currentInstance,
  owner,
  onRestored,
  onLocate,
  references,
  onReference,
  onDeleted,
}: Props) {
  const {
    error,
    setError,
    capture: captureFeedback,
    beginFeedback,
    endFeedback,
  } = useActionFeedback();

  const [open, setOpen] = useState(false);
  const [project, setProject] = useState("");
  const [feature, setFeature] = useState("");
  const [from, setFrom] = useState("");
  const [to, setTo] = useState("");
  const [page, setPage] = useState(0);
  const [data, setData] = useState<Row>();
  const [listRevision, reload] = useState(0);
  const [expanded, setExpanded] = useState<Row>();
  const [detailResult, setDetailResult] = useState<{
    key: string;
    value?: Row;
    error?: string;
  }>();
  const [busy, setBusy] = useState(false);
  const scoped =
    projectId === "global" ? { scope: "global" } : { project_id: projectId };
  const selected = new Set(references.map(experienceKey));

  useEffect(() => {
    if (!open || !roleId) return;
    const controller = new AbortController();
    const options = { signal: controller.signal };
    request(
      "query.role_experience",
      {
        ...(projectId === "global"
          ? { scope: "global" }
          : { project_id: projectId }),
        role_id: roleId,
        project_name: project,
        feature_name: feature,
        from: from ? new Date(from).getTime() : undefined,
        to: to ? new Date(to).getTime() : undefined,
        offset: page * pageSize,
        limit: pageSize,
      },
      options,
    )
      .then((result) => {
        if (controller.signal.aborted) return;
        const lastPage = Math.max(0, Math.ceil(result.total / pageSize) - 1);
        if (page > lastPage) setPage(lastPage);
        setData(result);
      })
      .catch((e) => {
        if (!controller.signal.aborted) setError(String(e));
      });
    return () => controller.abort();
  }, [
    request,
    projectId,
    open,
    roleId,
    project,
    feature,
    from,
    to,
    page,
    revision,
    listRevision,
    setError,
  ]);

  useEffect(() => {
    if (!expanded) return;
    const key = experienceKey(expanded);
    const controller = new AbortController();
    request(
      "query.role_experience",
      {
        ...(projectId === "global"
          ? { scope: "global" }
          : { project_id: projectId }),
        role_id: roleId,
        snapshot_id: expanded.snapshot_id,
        source_project_id: expanded.project_id,
      },
      { signal: controller.signal },
    )
      .then((result) => {
        if (!controller.signal.aborted) setDetailResult({ key, value: result });
      })
      .catch((e) => {
        if (!controller.signal.aborted)
          setDetailResult({ key, error: String(e) });
      });
    return () => controller.abort();
  }, [expanded, roleId, projectId, request]);

  async function change(work: () => Promise<void>) {
    beginFeedback();
    setError("");
    setBusy(true);
    try {
      await work();
    } catch (e) {
      setError(String(e));
    } finally {
      endFeedback();
      setBusy(false);
    }
  }

  const filters = (
    <div className="role-experience-filters">
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
    <details
      className="experience-panel"
      onToggle={(event) => setOpen(event.currentTarget.open)}
    >
      <summary>角色经验</summary>
      <div className="agent-information-content">
        {!roleId ? (
          <p>选择角色后查看经验。</p>
        ) : (
          <>
            {filters}
            <p>
              {data ? `${displayText(data.total)} 条经验` : "正在读取角色经验…"}
            </p>
            {data?.records.map((entry: Row) => {
              const key = experienceKey(entry);
              const referenced = selected.has(key);
              const open = expanded && experienceKey(expanded) === key;
              const resolved =
                open && detailResult?.key === key ? detailResult : undefined;
              const detail = resolved?.value;
              const detailError = resolved?.error;
              const unavailable = entry.availability === "unavailable";
              const title = `${entry.project_name} / ${entry.feature_name || "讨论"}`;
              return (
                <article
                  key={key}
                  className={
                    referenced
                      ? "experience-entry referenced"
                      : "experience-entry"
                  }
                >
                  <strong>{displayText(title)}</strong>
                  <p>
                    {displayText(new Date(entry.captured_at).toLocaleString())}{" "}
                    {entry.agent_name && ` · ${displayText(entry.agent_name)}`}
                    {" · "}
                    {displayText(entry.kind)} ·{" "}
                    {displayText(
                      entry.task_id
                        ? `Task ${shortId(entry.task_id)}`
                        : entry.feature_id
                          ? "Feature"
                          : "讨论",
                    )}
                  </p>
                  {unavailable && entry.metadata_excerpt && (
                    <p className="notice">
                      来源暂不可用，当前显示缓存的历史摘要。
                    </p>
                  )}
                  <div className="actions">
                    {onReference && (
                      <ActionButton
                        aria-pressed={referenced}
                        disabled={
                          busy ||
                          unavailable ||
                          (!referenced &&
                            references.length >= experienceReferenceLimit)
                        }
                        onClick={() => {
                          if (
                            referenced ||
                            entry.agent_instance_id === currentInstance
                          ) {
                            onReference(entry);
                            return;
                          }
                          return change(async () => {
                            const source =
                              detail ||
                              (await request("query.role_experience", {
                                ...scoped,
                                role_id: roleId,
                                source_project_id: entry.project_id,
                                snapshot_id: entry.snapshot_id,
                              }));
                            const excerpt = Array.from(
                              snapshotSummary(source.snapshot.content),
                            )
                              .slice(0, 1600)
                              .join("");
                            onReference({ ...entry, excerpt });
                          });
                        }}
                      >
                        {referenced ? "已引用" : "引用到当前讨论"}
                      </ActionButton>
                    )}
                    <ActionButton
                      aria-expanded={!!open}
                      disabled={unavailable}
                      onClick={() => {
                        setDetailResult(undefined);
                        setExpanded(open ? undefined : entry);
                      }}
                    >
                      {open ? "收起摘要与快照" : "展开摘要与快照"}
                    </ActionButton>
                    {onLocate && (
                      <ActionButton
                        disabled={busy || unavailable || !entry.session_id}
                        onClick={() =>
                          onLocate(open && detail ? detail.entry : entry)
                        }
                      >
                        定位来源工作
                      </ActionButton>
                    )}
                    <ActionButton
                      disabled={busy}
                      onClick={() => {
                        if (!window.confirm(`删除经验「${title}」？`)) return;
                        return change(async () => {
                          await request("experience.delete", {
                            ...scoped,
                            command_id: crypto.randomUUID(),
                            target: entry.snapshot_id,
                            expected_revision: 0,
                            payload: {
                              role_id: roleId,
                              agent_instance_id: entry.agent_instance_id,
                              snapshot_id: entry.snapshot_id,
                              source_project_id: entry.project_id,
                            },
                          });
                          onDeleted(entry);
                          if (open) setExpanded(undefined);
                          reload((version) => version + 1);
                        });
                      }}
                    >
                      删除经验
                    </ActionButton>
                  </div>
                  {open && (
                    <section
                      className="notice experience-detail"
                      aria-label="快照摘要"
                    >
                      {detailError ? (
                        <p role="alert" className="error">
                          {displayText(detailError)}
                        </p>
                      ) : !detail ? (
                        <p role="status">正在读取快照…</p>
                      ) : (
                        <>
                          <h3>{displayText(title)} · 快照摘要</h3>
                          <p>
                            {detail.entry.workflow_name &&
                              `工作流 ${detail.entry.workflow_name} · `}
                            Model {displayText(detail.entry.binding?.model)} ·
                            Harness{" "}
                            {displayText(
                              detail.entry.binding?.installation?.key?.provider,
                            )}{" "}
                            · 完整性 {displayText(detail.integrity)}
                          </p>
                          <MarkdownPreview
                            text={snapshotSummary(detail.snapshot.content)}
                          />
                          <details>
                            <summary>完整快照与证据引用</summary>
                            <pre>
                              {displayText(JSON.stringify(detail, null, 2))}
                            </pre>
                          </details>
                          <ActionButton
                            disabled={
                              busy ||
                              entry.restorable === false ||
                              owner.revision === undefined ||
                              (detail.entry.work_owner?.kind === "entity" &&
                                (detail.entry.work_owner.id !== owner.id ||
                                  detail.entry.work_owner.project !==
                                    projectId))
                            }
                            onClick={() =>
                              change(async () => {
                                const result = await request(
                                  "experience.restore",
                                  {
                                    ...scoped,
                                    command_id: crypto.randomUUID(),
                                    target: owner.id,
                                    expected_revision: owner.revision,
                                    payload: {
                                      agent_instance_id:
                                        entry.agent_instance_id,
                                      snapshot_id: entry.snapshot_id,
                                      source_project_id: entry.project_id,
                                    },
                                  },
                                );
                                onRestored(result.id);
                              })
                            }
                          >
                            基于此快照开启新讨论
                          </ActionButton>
                        </>
                      )}
                    </section>
                  )}
                </article>
              );
            })}
            {data?.unavailable.map((p: Row) => (
              <p key={p.project_id} className="notice">
                {displayText(p.project_name)}：历史项目当前不可用
              </p>
            ))}
            {(page > 0 || data?.total > pageSize) && (
              <div className="actions" aria-label="经验分页">
                <ActionButton
                  disabled={!page}
                  onClick={() => setPage((p) => p - 1)}
                >
                  上一页
                </ActionButton>
                <span>
                  第 {page + 1} /{" "}
                  {Math.max(1, Math.ceil((data?.total || 0) / pageSize))} 页
                </span>
                <ActionButton
                  disabled={(page + 1) * pageSize >= (data?.total || 0)}
                  onClick={() => setPage((p) => p + 1)}
                >
                  下一页
                </ActionButton>
              </div>
            )}
          </>
        )}
        {error && (
          <p role="alert" className="error">
            {displayText(error)}
          </p>
        )}
      </div>
    </details>
  );
}

function snapshotSummary(content: string): string {
  if (!content) return "该快照没有附加文字上下文。";
  try {
    const parsed = JSON.parse(content);
    if (Array.isArray(parsed?.messages)) {
      const messages = (parsed.messages as Row[])
        .filter(
          (event) =>
            ["user", "assistant"].includes(event.kind) &&
            event.mode !== "delta" &&
            event.content,
        )
        .slice(-4);
      if (messages.length)
        return messages
          .map(
            (event) => `**${event.kind === "user" ? "用户" : "Agent"}**

${event.content}`,
          )
          .join("\n\n");
    }
  } catch {
    // Older snapshots may contain plain Markdown.
  }
  return content;
}

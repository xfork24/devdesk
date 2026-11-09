import { AgentExperience } from "./AgentExperience";
import { FormEvent, useCallback, useEffect, useRef, useState } from "react";

type Row = Record<string, any>;
type Request = (method: string, params?: Row) => Promise<any>;
type Props = {
  project: Row;
  feature?: Row;
  tasks: Row[];
  agents: Row[];
  installations: Row[];
  configurations?: Row[];
  onLocateExperience?: (entry: Row) => void;
  initialOwner?: string;
  initialSession?: string;
  request: Request;
  refresh: () => Promise<void>;
};
const labels: Record<string, string> = {
  analyst: "需求分析师",
  architect: "架构师",
  developer: "开发",
  tester: "测试",
  reviewer: "评审",
};

export function RoleConversation({
  project,
  feature,
  tasks,
  agents,
  installations,
  configurations = [],
  onLocateExperience,
  initialOwner,
  initialSession,
  request,
  refresh,
}: Props) {
  const [owner, setOwner] = useState(initialOwner || project.id);
  const [catalog, setCatalog] = useState<Row[]>([]);
  const [instanceId, setInstanceId] = useState("");
  const [agent, setAgent] = useState("builtin-agent-analyst-v1");
  const [editing, setEditing] = useState<Row>();
  const [sessions, setSessions] = useState<Row[]>([]);
  const [sessionId, setSessionId] = useState("");
  const [detail, setDetail] = useState<Row>();
  const [events, setEvents] = useState<Row[]>([]);
  const [input, setInput] = useState("");
  const [busy, setBusy] = useState(false);
  const [error, setError] = useState("");
  const [proposalJson, setProposalJson] = useState("");
  const [answers, setAnswers] = useState<Record<string, string>>({});
  const [selections, setSelections] = useState<Record<string, string[]>>({});
  const generation = useRef(0);
  const cursor = useRef(0);
  const listGeneration = useRef(0);
  const selectedOwner =
    owner === project.id
      ? project
      : owner === feature?.id
        ? feature
        : tasks.find((t) => t.id === owner);
  const featureLock = configurations.find(
    (r) => r.kind === "feature_lock" && r.definition_id === feature?.id,
  );
  const runtime = configurations.find(
    (r) =>
      r.kind === "workflow_instance" &&
      r.definition_id === owner &&
      r.data.status !== "cancelled",
  );
  const plan =
    owner === feature?.id
      ? featureLock?.data.plan
      : featureLock?.data.task_plan;
  const recommended = plan?.steps.find(
    (step: Row) =>
      step.participant_slot &&
      runtime?.data.steps.some(
        (s: Row) =>
          s.key === step.key &&
          ["active", "waiting_input", "waiting_confirmation"].includes(
            s.status,
          ),
      ),
  );
  const projectConfiguration = configurations
    .filter((r) => r.kind === "project_configuration")
    .sort((a, b) => b.version - a.version)[0];
  const recommendation = recommended
    ? featureLock?.data.agent_refs[recommended.participant_slot]
    : owner === project.id
      ? projectConfiguration?.data.agent_refs.analyst
      : undefined;
  const composite = catalog.find((r) => r.id === agent && r.kind === "agent");
  const [instanceScope, setInstanceScope] = useState("project");
  useEffect(() => {
    if (!featureLock || owner === project.id || sessionId) return;
    if (!Object.values(featureLock.data.agent_refs).includes(agent)) {
      setAgent(recommendation || featureLock.data.agent_refs.analyst);
      setInstanceId("");
    }
  }, [featureLock?.id, owner, recommendation, sessionId]);
  useEffect(() => {
    let current = true;
    if (
      !agent ||
      agent.startsWith("builtin-") ||
      catalog.some((r) => r.id === agent)
    )
      return;
    request("query.agent_revision", { id: agent, project_id: project.id })
      .then((row) => {
        if (current)
          setCatalog((old) =>
            old.some((r) => r.id === row.id) ? old : [...old, row],
          );
      })
      .catch(() => {});
    return () => {
      current = false;
    };
  }, [agent, request, project.id]);
  useEffect(() => {
    if (detail?.session.id) {
      setAgent(detail.session.agent);
      setInstanceId(
        detail.session.runtime_binding?.agent_configuration
          ?.agent_instance_id || "",
      );
    }
  }, [detail?.session.id]);

  const choices:Row[] = [
    ...agents,
    ...catalog
      .filter((r) => r.kind == "agent" && !agents.some((a) => a.id === r.id))
      .map((r) => ({ ...r, agent_profile: { role: r.data.role } })),
  ];
  const roles = choices.filter(
    (a) =>
      a.agent_profile &&
      (catalog.some((r) => r.id === a.id && r.kind === "agent") ||
        detail?.session.agent === a.id) &&
      (featureLock && owner !== project.id
        ? Object.values(featureLock.data.agent_refs).includes(a.id)
        : !choices.some((d) => d.supersedes === a.id)),
  );
  useEffect(() => {
    if (sessionId || composite || !catalog.length) return;
    const candidates = catalog.filter(
      (r) =>
        r.kind == "agent" &&
        (!featureLock ||
          owner === project.id ||
          Object.values(featureLock.data.agent_refs).includes(r.id)),
    );
    const selected =
      candidates.find((r) => r.id === recommendation) ||
      candidates.find((r) => r.data.role === "analyst") ||
      candidates[0];
    if (selected) setAgent(selected.id);
  }, [catalog, owner, recommendation, sessionId, composite?.id]);
  const instances = catalog.filter(
    (r) => r.kind === "agent_instance" && r.data.agent_revision_id === agent,
  );
  useEffect(() => {
    let current = true;
    request("query.configurations", {
      local: true,
      project_id: project.id,
      limit: 100,
    })
      .then((d) => {
        if (current) setCatalog(d.records);
      })
      .catch((e) => {
        if (current) setError(String(e));
      });
    return () => {
      current = false;
    };
  }, [request, project.id]);
  useEffect(() => {
    if (recommendation && !sessionId) {
      setAgent(recommendation);
      setInstanceId("");
    }
  }, [recommendation, owner]);
  const active = detail?.turns.find(
    (t: Row) => !["completed", "failed", "cancelled"].includes(t.status),
  );
  const lastCompleted = detail?.turns.find(
    (t: Row) => t.status === "completed",
  );
  const scopedRequest = useCallback(
    (method: string, params: Row = {}) =>
      request(method, { ...params, project_id: project.id }),
    [request, project.id],
  );
  const command = (
    method: string,
    target: string | undefined,
    expected_revision: number | undefined,
    payload: Row = {},
  ) =>
    scopedRequest(method, {
      command_id: crypto.randomUUID(),
      target,
      expected_revision,
      payload,
    });
  const loadSessions = useCallback(async () => {
    const current = ++listGeneration.current;
    const result = await scopedRequest("query.sessions", { owner_id: owner });
    if (current === listGeneration.current) setSessions(result.sessions);
  }, [scopedRequest, owner]);
  useEffect(() => {
    ++generation.current;
    setSessionId(owner === initialOwner ? initialSession || "" : "");
    setDetail(undefined);
    setEvents([]);
    cursor.current = 0;
    loadSessions().catch((e) => setError(String(e)));
  }, [loadSessions, owner, initialOwner, initialSession]);
  useEffect(() => {
    const current = ++generation.current;
    cursor.current = 0;
    setEvents([]);
    setDetail(undefined);
    if (!sessionId) return;
    let stopped = false;
    let inFlight = false;
    async function poll() {
      if (inFlight) return;
      inFlight = true;
      try {
        let more = true;
        while (more && !stopped) {
          const value = await scopedRequest("query.session", {
            id: sessionId,
            cursor: cursor.current,
            limit: 100,
          });
          if (stopped || current !== generation.current) return;
          setDetail(value);
          setEvents((old) => [
            ...old,
            ...value.events.filter(
              (e: Row) => !old.some((row) => row.id === e.id),
            ),
          ]);
          cursor.current = value.next_cursor;
          more = value.events.length === 100;
        }
      } catch (e) {
        if (!stopped && current === generation.current) setError(String(e));
      } finally {
        inFlight = false;
      }
    }
    void poll();
    const timer = window.setInterval(poll, 1000);
    return () => {
      stopped = true;
      window.clearInterval(timer);
    };
  }, [sessionId, scopedRequest]);
  async function action(work: () => Promise<void>) {
    setBusy(true);
    setError("");
    try {
      await work();
    } catch (e) {
      setError(String(e));
    } finally {
      setBusy(false);
    }
  }
  async function open() {
    if (!selectedOwner) return;
    await action(async () => {
      const result = await command(
        "session.open",
        owner,
        selectedOwner.revision,
        {
          agent,
          ...(instanceId ? { agent_instance_id: instanceId } : {}),

        },
      );
      await loadSessions();
      setSessionId(result.id);
    });
  }
  async function send(event: FormEvent) {
    event.preventDefault();
    if (!detail || !input.trim()) return;
    await action(async () => {
      await command("conversation.send", sessionId, detail.session.revision, {
        client_message_id: crypto.randomUUID(),
        input,
      });
      setInput("");
    });
  }
  const projected: Row[] = [];
  for (const event of events) {
    const previous = event.provider_item
      ? projected.find(
          (e) =>
            e.turn_id === event.turn_id &&
            e.provider_item === event.provider_item,
        )
      : undefined;
    if (previous)
      previous.content =
        event.mode === "delta"
          ? previous.content + event.content
          : event.content;
    else projected.push({ ...event });
  }
  return (
    <div className="panel conversation-panel">
      <div className="panel-heading">
        <div>
          <h2>角色讨论</h2>
          <p className="muted">
            连续讨论保存为独立 Session。结构化内容经采纳后进入工程，正式交付和
            Review 另行完成。
          </p>
        </div>
      </div>
      <div className="actions">
        <label>
          讨论范围
          <select
            aria-label="讨论范围"
            value={owner}
            onChange={(e) => setOwner(e.target.value)}
          >
            <option value={project.id}>项目 · {project.title}</option>
            {feature && (
              <option value={feature.id}>Feature · {feature.title}</option>
            )}
            {tasks.map((t) => (
              <option key={t.id} value={t.id}>
                Task · {t.title}
              </option>
            ))}
          </select>
        </label>
        <label>
          角色
          <select
            aria-label="讨论角色"
            value={agent}
            onChange={(e) => {
              setAgent(e.target.value);
              setInstanceId("");
            }}
          >
            {roles.map((a) => (
              <option key={a.id} value={a.id}>
                {labels[a.agent_profile.role]} · {a.name} v{a.version}
              </option>
            ))}
          </select>
        </label>
        {!composite && (
          <p className="notice">
            {detail?.session
              ? "历史会话保留原运行绑定；请选择完整 Agent 组合开启新会话。"
              : "请先在规范与身份中创建 Model、Harness 和完整 Agent 组合，再选择讨论实例。"}
          </p>
        )}
        {composite && (
          <>
            <label>
              新实例经验范围
              <select
                value={instanceScope}
                onChange={(e) => setInstanceScope(e.target.value)}
              >
                <option value="project">当前 Project</option>
                {composite.scope.kind === "global" && (
                  <option value="global">Global · 跨项目积累</option>
                )}
              </select>
            </label>
            <button
              disabled={busy}
              onClick={() =>
                action(async () => {
                  const instance = await command(
                    "agent.instance.create",
                    undefined,
                    undefined,
                    {
                      name: `${composite.name} · ${instanceScope === "global" ? "Global" : project.title}`,
                      agent_revision_id: agent,
                      scope:
                        instanceScope === "global"
                          ? { kind: "global" }
                          : { kind: "project", project_id: project.id },
                    },
                  );
                  setCatalog((old) => [...old, instance]);
                  setInstanceId(instance.id);
                })
              }
            >
              创建讨论实例
            </button>
            <label>
              AgentID
              <select
                aria-label="AgentID"
                value={instanceId}
                onChange={(e) => setInstanceId(e.target.value)}
              >
                <option value="">复用兼容实例 / 新建 Project 实例</option>
                {instances.map((r) => (
                  <option key={r.id} value={r.id}>
                    {r.name} · {r.scope.kind} · {r.id.slice(0, 8)}
                  </option>
                ))}
              </select>
            </label>
            <p>
              Model{" "}
              {catalog.find((r) => r.id === composite.data.model_ref)?.data
                .model || composite.data.model_ref}{" "}
              · Harness{" "}
              {
                catalog.find((r) => r.id === composite.data.harness_ref)?.data
                  .provider
              }{" "}
              /{" "}
              {
                catalog.find((r) => r.id === composite.data.harness_ref)?.data
                  .runtime_version
              }{" "}
              · Agent v{composite.version}
            </p>
            <details>
              <summary>Skills / Tools / Permission 及定义 Scope</summary>
              <pre>{JSON.stringify(composite, null, 2)}</pre>
            </details>
          </>
        )}
        <button disabled={busy || !selectedOwner || !composite} onClick={open}>
          开始 / 续用讨论
        </button>
      </div>
      {recommended && (
        <p className="notice">
          工作流推荐：{recommended.name} ·{" "}
          {roles.find((r) => r.id === recommendation)?.name}。目标：
          {recommended.interaction.goal}；产物：
          {recommended.interaction.output_kind}
        </p>
      )}
      <AgentExperience
        key={instanceId || detail?.session.id || owner}
        request={request}
        projectId={project.id}
        instanceId={
          instanceId ||
          detail?.session.runtime_binding?.agent_configuration
            ?.agent_instance_id ||
          ""
        }
        owner={selectedOwner || project}
        onRestored={(id) => {
          void loadSessions();
          setSessionId(id);
        }}
        onLocate={onLocateExperience}
      />
      <label>
        已有会话
        <select
          aria-label="讨论会话"
          value={sessionId}
          onChange={(e) => setSessionId(e.target.value)}
        >
          <option value="">选择会话</option>
          {sessions.map((s) => (
            <option key={s.id} value={s.id}>
              {s.runtime_binding.installation.key.provider} · {s.agent} ·{" "}
              {s.lifecycle} · {s.id.slice(0, 8)}
            </option>
          ))}
        </select>
      </label>
      {error && (
        <div className="error" role="alert">
          {error}
        </div>
      )}
      {detail && (
        <>
          <p className="muted">
            {detail.session.runtime_binding.installation.key.provider} ·{" "}
            {detail.session.runtime_binding.model} · {detail.session.lifecycle}{" "}
            · 原生会话 {detail.session.external_session || "尚未启动"}
          </p>
          <details>
            <summary>本次会话的冻结权限</summary>
            {detail.session.runtime_binding.permissions ? (
              <pre>
                {JSON.stringify(
                  detail.session.runtime_binding.permissions,
                  null,
                  2,
                )}
              </pre>
            ) : (
              <p>
                旧版执行未保存标准 Permission
                快照；继续使用原运行锁，执行保证以历史回执为准。
              </p>
            )}
          </details>
          {detail.executions
            .filter((e: Row) => e.status === "cleanup_failed")
            .map((e: Row) => (
              <p key={e.id} className="notice">
                执行实例 {e.id} 已停止，清理失败。
                <button
                  disabled={busy}
                  onClick={() =>
                    action(async () => {
                      await command("execution.retry_cleanup", e.id, undefined);
                    })
                  }
                >
                  重试清理
                </button>
              </p>
            ))}
          {detail.session.predecessor && (
            <p>
              继承自 {detail.session.predecessor}，上下文快照{" "}
              {detail.session.snapshot_id}
            </p>
          )}
          <div role="log" aria-label="讨论记录" className="conversation-log">
            {projected.map((e) => (
              <article key={e.id} className={`conversation-message ${e.kind}`}>
                <strong>{e.kind}</strong>
                <pre>{e.content}</pre>
              </article>
            ))}
          </div>
          {active && (
            <p role="status">
              当前轮次：{active.status} · 输入
              {active.consumed ? "已消费" : "尚未确认消费"}{" "}
              <button
                disabled={busy}
                onClick={() =>
                  action(async () => {
                    await command(
                      "conversation.cancel",
                      active.id,
                      active.revision,
                    );
                  })
                }
              >
                停止当前轮次
              </button>
            </p>
          )}
          {detail.interactions
            .filter((i: Row) => i.status === "pending")
            .map((i: Row) => (
              <div className="notice" key={i.id}>
                <h3>
                  {i.kind === "tool_approval"
                    ? "运行时工具审批"
                    : "Agent 澄清问题"}
                </h3>
                <p>{i.reason}</p>
                {i.tool && <pre>{i.tool}</pre>}
                {i.questions.map((q: Row) => (
                  <label key={q.id}>
                    {q.question}
                    {q.detail && <pre>{q.detail}</pre>}
                    {q.options.length > 0 && (
                      <select
                        aria-label={q.question + "选项"}
                        multiple={q.multi_select}
                        value={
                          q.multi_select
                            ? selections[q.id] || []
                            : selections[q.id]?.[0] || ""
                        }
                        onChange={(e) =>
                          setSelections((old) => ({
                            ...old,
                            [q.id]: Array.from(e.target.selectedOptions)
                              .map((o) => o.value)
                              .filter(Boolean),
                          }))
                        }
                      >
                        {!q.multi_select && (
                          <option value="">请选择或填写回答</option>
                        )}
                        {q.options.map((option: string) => (
                          <option key={option} value={option}>
                            {option}
                          </option>
                        ))}
                      </select>
                    )}
                    <input
                      aria-label={q.question}
                      value={answers[q.id] || ""}
                      onChange={(e) =>
                        setAnswers((old) => ({
                          ...old,
                          [q.id]: e.target.value,
                        }))
                      }
                    />
                  </label>
                ))}
                <div className="actions">
                  {(i.kind === "tool_approval"
                    ? ["allowed_once", "rejected", "cancelled"]
                    : ["answer", "cancelled"]
                  ).map((decision) => (
                    <button
                      key={decision}
                      disabled={busy}
                      onClick={() =>
                        action(async () => {
                          const answer =
                            i.kind === "tool_approval"
                              ? { kind: "tool_approval", outcome: decision }
                              : {
                                  kind: "user_questions",
                                  cancelled: decision === "cancelled",
                                  answers:
                                    decision === "cancelled"
                                      ? []
                                      : i.questions.map((q: Row) => ({
                                          id: q.id,
                                          selected: selections[q.id] || [],
                                          custom: answers[q.id]?.trim() || null,
                                        })),
                                };
                          await command(
                            "interaction.answer",
                            i.id,
                            i.revision,
                            {
                              session_id: sessionId,
                              turn_id: i.turn_id,
                              answer,
                            },
                          );
                        })
                      }
                    >
                      {
                        (
                          {
                            allowed_once: "仅允许本次",
                            rejected: "拒绝",
                            cancelled: "取消",
                            answer: "提交回答",
                          } as Row
                        )[decision]
                      }
                    </button>
                  ))}
                </div>
              </div>
            ))}
          <form onSubmit={send}>
            <label>
              发送给角色
              <textarea
                aria-label="讨论输入"
                value={input}
                onChange={(e) => setInput(e.target.value)}
                required
                maxLength={65536}
              />
            </label>
            <button
              className="primary"
              disabled={
                busy ||
                !!active ||
                ["implementation", "testing", "review"].includes(
                  detail.session.runtime_binding.work_type,
                ) ||
                [
                  "needs_reconciliation",
                  "requires_successor",
                  "closed",
                ].includes(detail.session.lifecycle)
              }
            >
              发送
            </button>
          </form>
          <details>
            <summary>结构化提案与采纳</summary>
            <p className="muted">
              请让 Agent 输出 ProposalContent
              JSON，或在此编辑内容。采纳会校验来源轮次、目标版本和输入修订。TaskPlan
              在规划 Review 通过后实例化。
            </p>
            <textarea
              aria-label="结构化提案 JSON"
              value={proposalJson}
              onChange={(e) => setProposalJson(e.target.value)}
              placeholder={
                '{"kind":"requirement","title":"...","body":"...","acceptance":[{"key":"AC-1","description":"..."}]}'
              }
            />
            <div className="actions">
              <button
                disabled={busy || !lastCompleted}
                onClick={() =>
                  action(async () => {
                    await command("proposal.extract", undefined, undefined, {
                      turn_id: lastCompleted.id,
                      target: owner,
                    });
                  })
                }
              >
                从完成的回答提取
              </button>
              <button
                disabled={busy || !lastCompleted || !proposalJson.trim()}
                onClick={() =>
                  action(async () => {
                    await command(
                      editing ? "proposal.revise" : "proposal.create",
                      editing?.id,
                      editing?.revision,
                      {
                        turn_id: lastCompleted.id,
                        target: owner,
                        content: JSON.parse(proposalJson),
                      },
                    );
                    setEditing(undefined);
                    setProposalJson("");
                  })
                }
              >
                {editing ? "保存草稿的新修订" : "保存编辑后的提案"}
              </button>
            </div>
            {detail.proposals.map((p: Row) => (
              <article key={p.id}>
                <h3>
                  {p.content.kind} · {p.status}
                </h3>
                <p className="muted">
                  目标版本 {p.expected_revision} · 输入修订{" "}
                  {p.dependencies.join(", ") || "无"} · 来源 Turn {p.turn_id}
                </p>
                {p.supersedes && (
                  <details>
                    <summary>查看修订前内容</summary>
                    <pre>
                      {JSON.stringify(
                        detail.proposals.find((v: Row) => v.id === p.supersedes)
                          ?.content || { id: p.supersedes },
                        null,
                        2,
                      )}
                    </pre>
                  </details>
                )}
                <pre>{JSON.stringify(p.content, null, 2)}</pre>
                {["requirement", "design", "test_plan"].includes(
                  p.content.kind,
                ) && (
                  <p className="muted">
                    采纳会生成文档新修订；工程输入变化会使相关执行与证据失效。
                  </p>
                )}
                <div className="actions">
                  <button
                    disabled={busy || p.status !== "draft"}
                    onClick={() => {
                      setEditing({ id: p.id, revision: p.revision });
                      setProposalJson(JSON.stringify(p.content, null, 2));
                    }}
                  >
                    编辑为新修订
                  </button>
                  <button
                    disabled={busy || p.status !== "draft"}
                    onClick={() =>
                      action(async () => {
                        await command("proposal.adopt", p.id, p.revision);
                        await refresh();
                      })
                    }
                  >
                    采纳
                  </button>
                  <button
                    disabled={busy || p.status !== "draft"}
                    onClick={() =>
                      action(async () => {
                        await command("proposal.reject", p.id, p.revision);
                      })
                    }
                  >
                    拒绝
                  </button>
                  {p.content.kind === "task_plan" && (
                    <button
                      disabled={
                        busy ||
                        p.status !== "accepted" ||
                        Object.keys(p.task_instances).length > 0
                      }
                      onClick={() =>
                        action(async () => {
                          await command(
                            "proposal.instantiate",
                            p.id,
                            p.revision,
                          );
                          await refresh();
                        })
                      }
                    >
                      实例化 Tasks
                    </button>
                  )}
                </div>
              </article>
            ))}
          </details>
        </>
      )}
    </div>
  );
}

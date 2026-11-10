import {
  ActionButton,
  ActionForm,
  ActionSelect,
  useActionFeedback,
} from "./ActionFeedback";
import { ExecutionTaskDetails } from "./WorkflowTaskPreview";
import { loadCatalog, MarkdownField, MarkdownPreview } from "./configuration-ui";
import { AgentExperience } from "./AgentExperience";
import { FormEvent, useCallback, useEffect, useRef, useState } from "react";

type Row = Record<string, any>;
type Request = (method: string, params?: Row) => Promise<any>;
type Props = {
  project?: Row;
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
const globalOwner = { id: "global", title: "全局 / 跨项目", revision: 0 };
const labels: Record<string, string> = {
  analyst: "需求分析师",
  architect: "架构师",
  developer: "开发",
  tester: "测试",
  reviewer: "评审",
};

function structuredContent(content: string) {
  if (!/^[\s]*[\[{]/.test(content)) return false;
  try {
    const value = JSON.parse(content);
    return value !== null && typeof value === "object";
  } catch {
    return false;
  }
}

export function RoleConversation({
  project = globalOwner,
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
  const {
    error,
    setError,
    capture: captureFeedback,
    beginFeedback,
    endFeedback,
  } = useActionFeedback();

  const [owner, setOwner] = useState(initialOwner || project.id);
  const [catalog, setCatalog] = useState<Row[]>([]);
  const [instanceId, setInstanceId] = useState("");
  const [agent, setAgent] = useState("");
  const [editing, setEditing] = useState<Row>();
  const [sessions, setSessions] = useState<Row[]>([]);
  const [sessionId, setSessionId] = useState("");
  const [detail, setDetail] = useState<Row>();
  const currentBinding = detail?.current_runtime_binding || detail?.session.runtime_binding;
  const [events, setEvents] = useState<Row[]>([]);
  const [input, setInput] = useState("");
  const [busy, setBusy] = useState(false);

  const [proposalJson, setProposalJson] = useState("");
  const [answers, setAnswers] = useState<Record<string, string>>({});
  const [selections, setSelections] = useState<Record<string, string[]>>({});
  const pendingFocus = useRef<{ owner: string; session: string } | null>(null);
  const manualSession = useRef(false);
  const [sessionAnchor, setSessionAnchor] = useState(initialSession || "");
  function clearSelectedSession() {
    manualSession.current = false;
    generation.current += 1;
    listGeneration.current += 1;
    setSessions([]);
    setSessionAnchor("");
    setSessionId("");
    setDetail(undefined);
    setEvents([]);
    setInput("");
    cursor.current = 0;
  }
  const generation = useRef(0);
  const cursor = useRef(0);
  const listGeneration = useRef(0);
  const global = owner === "global";
  const selectedOwner = global
    ? globalOwner
    : owner === project.id
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
      ? (Object.values(
          projectConfiguration?.data.role_agent_bindings ||
            {},
        ).find(
          (id) => catalog.find((r) => r.id === id)?.data.role === "analyst",
        ) as string | undefined)
      : undefined;
  const composite = catalog.find((r) => r.id === agent && r.kind === "agent");

  useEffect(() => {
    if (!featureLock || owner === project.id || global || sessionId) return;
    if (!Object.values(featureLock.data.agent_refs).includes(agent)) {
      setAgent(recommendation || "");
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
        currentBinding?.agent_configuration
          ?.agent_instance_id ||
          detail.agent_instance_id ||
          "",
      );
    }
  }, [detail?.session.id]);

  const choices: Row[] = [
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
      (detail?.session.agent === a.id ||
        (featureLock && owner !== project.id && !global
          ? Object.values(featureLock.data.agent_refs).includes(a.id)
          : !choices.some((d) => d.supersedes === a.id))),
  );
  useEffect(() => {
    if (sessionId || composite || !catalog.length) return;
    const candidates = catalog.filter(
      (r) =>
        r.kind == "agent" &&
        (!featureLock ||
          owner === project.id ||
          global ||
          Object.values(featureLock.data.agent_refs).includes(r.id)),
    );
    const selected =
      candidates.find((r) => r.id === recommendation) ||
      candidates.find(
        (r) => !candidates.some((next) => next.supersedes === r.id),
      );
    if (selected) setAgent(selected.id);
  }, [catalog, owner, recommendation, sessionId, composite?.id]);
  const instances = catalog.filter(
    (r) =>
      r.kind === "agent_instance" &&
      r.data.agent_revision_id === agent &&
      (!global || r.scope.kind === "global"),
  );
  useEffect(() => {
    if (!sessionId && !instances.some((r) => r.id === instanceId))
      setInstanceId(instances[0]?.id || "");
  }, [catalog, agent, global, sessionId, instanceId]);
  useEffect(() => {
    let current = true;
    loadCatalog(request, global ? undefined : project)
      .then((rows) => {
        if (current) setCatalog(rows);
      })
      .catch((e) => {
        if (current) setError(String(e));
      });
    return () => {
      current = false;
    };
  }, [request, project.id, global]);
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
  const formalOutput = Boolean(
    detail?.formal_execution ||
    (detail?.output_contract?.id &&
      detail.output_contract.id !== "proposal-content/v1"),
  );
  const scopedRequest = useCallback(
    (method: string, params: Row = {}) =>
      request(method, {
        ...params,
        ...(global ? { scope: "global" } : { project_id: project.id }),
      }),
    [request, project.id, global],
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
  const recommendedStepId = runtime?.data.steps.find((s: Row) => s.key === recommended?.key)?.id;
  const loadSessions = useCallback(async () => {
    const current = ++listGeneration.current;
    const result = await scopedRequest("query.sessions", {
      owner_id: owner,
      agent_instance_id: instanceId || undefined,
      workflow_session_id: sessionAnchor || undefined,
      workflow_step_instance_id: sessionAnchor ? undefined : recommendedStepId,
      agent: agent || undefined,
    });
    if (current === listGeneration.current) {
      setSessions(result.sessions);
      if (!manualSession.current && (instanceId || agent || sessionAnchor)) {
        const latest = result.sessions.find((s: Row) => instanceId ? (s.agent_instance_id || s.runtime_binding?.agent_configuration?.agent_instance_id) === instanceId : agent ? s.agent === agent : !!sessionAnchor);
        setSessionId(latest?.id || "");
      }
    }
  }, [scopedRequest, owner, instanceId, sessionAnchor, agent, recommendedStepId]);
  useEffect(() => {
    ++generation.current;
    ++listGeneration.current;
    const focus =
      pendingFocus.current?.owner === owner ? pendingFocus.current : null;
    pendingFocus.current = null;
    manualSession.current = false;
    setSessionAnchor(focus?.session || (owner === initialOwner ? initialSession || "" : ""));
    setSessionId(
      focus?.session || (owner === initialOwner ? initialSession || "" : ""),
    );
    setDetail(undefined);
    setSessions([]);
    setEvents([]);
    cursor.current = 0;
  }, [scopedRequest, owner, initialOwner, initialSession]);
  useEffect(() => {
    void loadSessions().catch((e) => setError(String(e)));
  }, [loadSessions]);
  useEffect(() => {
    const current = ++generation.current;
    cursor.current = 0;
    setEvents([]);
    setDetail(undefined);
    if (!sessionId) return;
    let stopped = false;
    let inFlight = false;
    async function poll() {
      captureFeedback();

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
    captureFeedback();

    (beginFeedback(), setBusy(true));
    setError("");
    try {
      await work();
    } catch (e) {
      setError(String(e));
    } finally {
      (endFeedback(), setBusy(false));
    }
  }
  async function open(forceNew = false) {
    captureFeedback();

    if (!selectedOwner) return;
    const context = detail?.workflow_context || sessions.find((s) => s.id === sessionAnchor)?.workflow_context;
    const stepId = context?.step_instance_id || runtime?.data.steps.find((s: Row) => s.key === recommended?.key)?.id;
    manualSession.current = true;
    await action(async () => {
      const result = await command(
        "session.open",
        owner,
        selectedOwner.revision,
        {
          agent,
          ...(instanceId ? { agent_instance_id: instanceId } : {}),
          ...(stepId ? {workflow_step_instance_id: stepId} : {}),
          ...(context?.recipe_node ? {workflow_recipe_node: context.recipe_node} : {}),
          ...(forceNew ? {force_new: true} : {}),
        },
      );
      await loadSessions();
      setSessionId(result.id);
    });
  }
  async function send(event: FormEvent) {
    captureFeedback();

    event.preventDefault();
    if (!detail || !input.trim()) return;
    await action(async () => {
      const result = await command(
        "conversation.send",
        sessionId,
        detail.session.revision,
        {
          client_message_id: crypto.randomUUID(),
          input,
        },
      );
      if (
        result.effective_session_id &&
        result.effective_session_id !== sessionId
      ) {
        await loadSessions();
        setSessionId(result.effective_session_id);
      }
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
            与选定 Agent 直接沟通，并查看它积累的工作与快照。Agent
            的经验范围由其定义确定；全局讨论可跨项目保留。
          </p>
        </div>
      </div>
      <div className="actions">
        <label>
          讨论范围
          <ActionSelect
            aria-label="讨论范围"
            disabled={busy}
            value={owner}
            onChange={(e) => {
              captureFeedback();
              return setOwner(e.target.value);
            }}
          >
            <option value="global">全局 / 跨项目</option>
            {project.id !== "global" && (
              <option value={project.id}>项目 · {project.title}</option>
            )}
            {feature && (
              <option value={feature.id}>Feature · {feature.title}</option>
            )}
            {tasks.map((t) => (
              <option key={t.id} value={t.id}>
                Task · {t.title}
              </option>
            ))}
          </ActionSelect>
        </label>
        <label>
          角色
          <ActionSelect
            aria-label="讨论角色"
            disabled={busy}
            value={agent}
            onChange={(e) => {
              captureFeedback();

              clearSelectedSession();
              setAgent(e.target.value);
              setInstanceId("");
            }}
          >
            {roles.map((a) => (
              <option key={a.id} value={a.id}>
                {labels[a.agent_profile.role]} · {a.name} v{a.version}
              </option>
            ))}
          </ActionSelect>
        </label>
        {!composite && (
          <p className="notice">
            {detail?.session
              ? "历史会话保留原运行绑定；请选择完整 Agent 组合开启新会话。"
              : "请先在基础组件中准备资源，并在角色与 Agent 中组合所需能力，再选择讨论实例。"}
          </p>
        )}
        {composite && (
          <>
            <label>
              Agent
              <ActionSelect
                aria-label="Agent"
                disabled={busy}
                value={instanceId}
                onChange={(e) => {
                  captureFeedback();
                  clearSelectedSession();
                  setInstanceId(e.target.value);
                }}
              >
                <option value="" disabled>
                  请选择已创建的 Agent
                </option>
                {instances.map((r) => (
                  <option key={r.id} value={r.id}>
                    {r.name} · {r.scope.kind} · {r.id.slice(0, 8)}
                  </option>
                ))}
              </ActionSelect>
            </label>
            <div className="actions">
              <ActionButton type="button" disabled={busy} onClick={() => action(async () => {
                const created = await request("agent.instance.create", {
                  project_id: global ? undefined : project.id,
                  command_id: crypto.randomUUID(),
                  payload: {
                    name: `${composite.name} · ${global ? "全局" : "项目"}讨论 ${instances.length + 1}`,
                    agent_revision_id: composite.id,
                    ...(!global && owner !== project.id && featureLock?.data.agent_refs && Object.values(featureLock.data.agent_refs).includes(composite.id) ? { frozen_feature_id: feature?.id } : {}),
                    scope: global ? { kind: "global" } : { kind: "project", project_id: project.id },
                  },
                });
                clearSelectedSession();
                setCatalog(await loadCatalog(request, global ? undefined : project));
                setInstanceId(created.id);
              })}>新建讨论身份</ActionButton>
              {!instances.length && <p className="muted">先创建讨论身份，即可独立保留该 Agent 的会话与经验。</p>}
            </div>
            {instanceId && (
              <p className="muted">
                该 Agent 的经验范围：
                {instances.find((r) => r.id === instanceId)?.scope.kind ===
                "global"
                  ? "全局 / 跨项目"
                  : "当前项目"}
                ；已有工作按此身份保留。
              </p>
            )}
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
        <ActionButton
          disabled={busy || !selectedOwner || !composite || !instanceId}
          onClick={() => open()}
        >
          开始 / 续用讨论
        </ActionButton>
        <ActionButton disabled={busy || !selectedOwner || !composite || !instanceId || formalOutput || !!active}
          onClick={() => open(true)}>新建 Session</ActionButton>
      </div>
      {recommended && (
        <p className="notice">
          工作流推荐：{recommended.name} ·{" "}
          {roles.find((r) => r.id === recommendation)?.name}。目标：
          {recommended.task_instructions}；产物：
          {recommended.interaction.output_kind}
        </p>
      )}
      <AgentExperience
        revision={lastCompleted?.id}
        key={instanceId || detail?.session.id || owner}
        request={request}
        projectId={global ? "global" : project.id}
        instanceId={
          instanceId ||
          detail?.session.runtime_binding?.agent_configuration
            ?.agent_instance_id ||
          ""
        }
        owner={selectedOwner || project}
        onRestored={(id) => {
          manualSession.current = true;
          void loadSessions();
          setSessionId(id);
        }}
        onLocate={(entry) => {
          if (entry.project_id === "00000000-0000-4000-8000-000000000000") {
            if (global) { manualSession.current = true; setSessionId(entry.session_id); }
            else {
              pendingFocus.current = {
                owner: "global",
                session: entry.session_id,
              };
              setOwner("global");
            }
          } else onLocateExperience?.(entry);
        }}
      />
      <label>
        已有会话
        <ActionSelect
          aria-label="讨论会话"
          disabled={busy}
          value={sessionId}
          onChange={(e) => {
            captureFeedback();
            manualSession.current = true;
            return setSessionId(e.target.value);
          }}
        >
          <option value="">选择会话</option>
          {sessions
            .filter(
              (s) =>
                s.id === sessionId ||
                (instanceId
                  ? (s.agent_instance_id ||
                      s.runtime_binding?.agent_configuration
                        ?.agent_instance_id) === instanceId
                  : s.agent === agent),
            )
            .map((s) => (
              <option key={s.id} value={s.id}>
                {s.runtime_binding.installation.key.provider} · {s.agent} ·{" "}
                {s.lifecycle} · {s.id.slice(0, 8)}
              </option>
            ))}
        </ActionSelect>
      </label>
      {error && (
        <div className="error" role="alert">
          {error}
        </div>
      )}
      {detail && (
        <>
          {detail.session.predecessor ? (
            <p className="muted">
              本次讨论继承了前序上下文。
              <ActionButton
                type="button"
                onClick={() => {
                  captureFeedback();
                  manualSession.current = true;
                  return setSessionId(detail.session.predecessor);
                }}
              >
                查看前序讨论
              </ActionButton>
            </p>
          ) : null}
          {sessions
            .filter((s) => s.predecessor === sessionId)
            .map((s, index) => (
              <ActionButton
                type="button"
                key={s.id}
                onClick={() => {
                  captureFeedback();
                  manualSession.current = true;
                  return setSessionId(s.id);
                }}
              >
                查看后续讨论 {index + 1}
              </ActionButton>
            ))}
          <p className="muted">
            {currentBinding.installation.key.provider} ·{" "}
            {currentBinding.model_connection?.provider ||
              currentBinding.agent_configuration
                ?.model_provider ||
              (currentBinding.installation.key.provider ===
              "dsh"
                ? "deepseek"
                : "openai")}{" "}
            · {currentBinding.model} ·{" "}
            {detail.session.lifecycle} · 原生会话{" "}
            {detail.session.external_session || "尚未启动"}
          </p>
          {currentBinding.model_connection && (
            <p className="muted">
              模型地址{" "}
              <code>
                {currentBinding.model_connection.endpoint}
              </code>{" "}
              · 容器地址{" "}
              <code>
                {
                  currentBinding.model_connection
                    .worker_endpoint
                }
              </code>
            </p>
          )}
          <details>
            <summary>本次会话的冻结权限</summary>
            {detail.permission_status ? (
              <div>
                <p>
                  权限状态：
                  {(
                    {
                      adopted: "已采用当前绑定",
                      pending: "当前工作结束后切换",
                      ready_to_switch: "下一轮将切换到新绑定",
                      unconfigured: "目标权限未就绪",
                      needs_reconciliation: "权限切换需要核对停止回执",
                    } as Record<string, string>
                  )[detail.permission_status.status] ||
                    detail.permission_status.status}
                </p>
                {detail.permission_status.reason ? (
                  <p role="alert">{detail.permission_status.reason}</p>
                ) : null}
                {detail.permission_status.pending_command ? (
                  <ActionButton
                    disabled={busy}
                    onClick={() => {
                      captureFeedback();
                      return action(async () => {
                        const result = await scopedRequest(
                          "conversation.send",
                          detail.permission_status.pending_command,
                        );
                        await loadSessions();
                        setSessionId(result.effective_session_id || sessionId);
                        setInput("");
                      });
                    }}
                  >
                    恢复权限切换与原输入
                  </ActionButton>
                ) : null}
              </div>
            ) : null}
            {currentBinding.permissions ? (
              <pre>
                {JSON.stringify(
                  currentBinding.permissions,
                  null,
                  2,
                )}
              </pre>
            ) : (
              <p>
                当前会话缺少版本化 Permission
                快照；继续使用原运行锁，执行保证以历史回执为准。
              </p>
            )}
          </details>
          {!global && (
            <ExecutionTaskDetails
              key={detail.session.id}
              request={request}
              projectId={project.id}
              turns={detail.turns || []}
            />
          )}
          {detail.executions
            .filter((e: Row) => e.status === "cleanup_failed")
            .map((e: Row) => (
              <p key={e.id} className="notice">
                执行实例 {e.id} 已停止，清理失败。
                <ActionButton
                  disabled={busy}
                  onClick={() => {
                    captureFeedback();
                    return action(async () => {
                      await command("execution.retry_cleanup", e.id, undefined);
                    });
                  }}
                >
                  重试清理
                </ActionButton>
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
                {["user", "assistant"].includes(e.kind) && !structuredContent(e.content)
                  ? <MarkdownPreview text={e.content} />
                  : <pre>{e.content}</pre>}
              </article>
            ))}
          </div>
          {active && (
            <p role="status">
              当前轮次：{active.status} · 输入
              {active.consumed ? "已消费" : "尚未确认消费"}{" "}
              <ActionButton
                disabled={busy}
                onClick={() => {
                  captureFeedback();
                  return action(async () => {
                    await command(
                      "conversation.cancel",
                      active.id,
                      active.revision,
                    );
                  });
                }}
              >
                停止当前轮次
              </ActionButton>
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
                      <ActionSelect
                        aria-label={q.question + "选项"}
                        multiple={q.multi_select}
                        value={
                          q.multi_select
                            ? selections[q.id] || []
                            : selections[q.id]?.[0] || ""
                        }
                        onChange={(e) => {
                          captureFeedback();
                          return setSelections((old) => ({
                            ...old,
                            [q.id]: Array.from(e.target.selectedOptions)
                              .map((o) => o.value)
                              .filter(Boolean),
                          }));
                        }}
                      >
                        {!q.multi_select && (
                          <option value="">请选择或填写回答</option>
                        )}
                        {q.options.map((option: string) => (
                          <option key={option} value={option}>
                            {option}
                          </option>
                        ))}
                      </ActionSelect>
                    )}
                    <input
                      aria-label={q.question}
                      value={answers[q.id] || ""}
                      onChange={(e) => {
                        captureFeedback();
                        return setAnswers((old) => ({
                          ...old,
                          [q.id]: e.target.value,
                        }));
                      }}
                    />
                  </label>
                ))}
                <div className="actions">
                  {(i.kind === "tool_approval"
                    ? ["allowed_once", "rejected", "cancelled"]
                    : ["answer", "cancelled"]
                  ).map((decision) => (
                    <ActionButton
                      key={decision}
                      disabled={busy}
                      onClick={() => {
                        captureFeedback();
                        return action(async () => {
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
                        });
                      }}
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
                    </ActionButton>
                  ))}
                </div>
              </div>
            ))}
          <ActionForm onSubmit={send}>
            <MarkdownField
              label="讨论输入"
              disabled={formalOutput}
              value={input}
              onChange={(value) => {
                captureFeedback();
                setInput(value);
              }}
              required
              maxLength={65536}
              rows={5}
            />
            <ActionButton
              className="primary"
              disabled={
                busy ||
                formalOutput ||
                !!active ||
                ["implementation", "testing", "review"].includes(
                  currentBinding.work_type,
                ) ||
                [
                  "needs_reconciliation",
                  "requires_successor",
                  "closed",
                ].includes(detail.session.lifecycle)
              }
            >
              发送
            </ActionButton>
          </ActionForm>
          {formalOutput && (
            <p className="muted">
              该会话用于正式执行或校验。请在“执行与交付”的对应步骤查看模型结论并执行人类
              Approve。
              {detail.session.feature_id && (
                <a
                  href={`/?${new URLSearchParams({
                    tab: "execution",
                    detail: "overview",
                    project_id: detail.session.project_id,
                    feature_id: detail.session.feature_id,
                    ...(detail.session.task_id
                      ? { task_id: detail.session.task_id }
                      : {}),
                  })}`}
                >
                  返回当前工作流
                </a>
              )}
            </p>
          )}
          <details hidden={global || formalOutput}>
            <summary>结构化提案与采纳</summary>
            <p className="muted">
              请让 Agent 输出 ProposalContent
              JSON，或在此编辑内容。采纳会校验来源轮次、目标版本和输入修订。TaskPlan
              在规划 Review 通过后实例化。
            </p>
            <textarea
              aria-label="结构化提案 JSON"
              value={proposalJson}
              onChange={(e) => {
                captureFeedback();
                return setProposalJson(e.target.value);
              }}
              placeholder={
                '{"kind":"requirement","title":"...","body":"...","acceptance":[{"key":"AC-1","description":"..."}]}'
              }
            />
            <div className="actions">
              <ActionButton
                disabled={busy || !lastCompleted}
                onClick={() => {
                  captureFeedback();
                  return action(async () => {
                    await command("proposal.extract", undefined, undefined, {
                      turn_id: lastCompleted.id,
                      target: owner,
                    });
                  });
                }}
              >
                从完成的回答提取
              </ActionButton>
              <ActionButton
                disabled={busy || !lastCompleted || !proposalJson.trim()}
                onClick={() => {
                  captureFeedback();
                  return action(async () => {
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
                  });
                }}
              >
                {editing ? "保存草稿的新修订" : "保存编辑后的提案"}
              </ActionButton>
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
                  <ActionButton
                    disabled={busy || p.status !== "draft"}
                    onClick={() => {
                      captureFeedback();

                      setEditing({ id: p.id, revision: p.revision });
                      setProposalJson(JSON.stringify(p.content, null, 2));
                    }}
                  >
                    编辑为新修订
                  </ActionButton>
                  <ActionButton
                    disabled={busy || p.status !== "draft"}
                    onClick={() => {
                      captureFeedback();
                      return action(async () => {
                        await command("proposal.adopt", p.id, p.revision);
                        await refresh();
                      });
                    }}
                  >
                    采纳
                  </ActionButton>
                  <ActionButton
                    disabled={busy || p.status !== "draft"}
                    onClick={() => {
                      captureFeedback();
                      return action(async () => {
                        await command("proposal.reject", p.id, p.revision);
                      });
                    }}
                  >
                    拒绝
                  </ActionButton>
                  {p.content.kind === "task_plan" && (
                    <ActionButton
                      disabled={
                        busy ||
                        p.status !== "accepted" ||
                        Object.keys(p.task_instances).length > 0
                      }
                      onClick={() => {
                        captureFeedback();
                        return action(async () => {
                          await command(
                            "proposal.instantiate",
                            p.id,
                            p.revision,
                          );
                          await refresh();
                        });
                      }}
                    >
                      实例化 Tasks
                    </ActionButton>
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

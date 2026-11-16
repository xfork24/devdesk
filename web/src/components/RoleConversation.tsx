import { ReadableValue } from "./reading";
import { ConversationContent } from "./ConversationContent";
import { ActionButton, ActionForm, ActionSelect } from "./ActionFeedback";
import { useActionFeedback } from "./action-feedback";
import { ExecutionTaskDetails } from "./WorkflowTaskPreview";
import { loadCatalog } from "./configuration-state";
import { MarkdownField } from "./configuration-ui";
import { type Request, type Row } from "./rpc-types";
import { AgentExperience } from "./AgentExperience";
import {
  FormEvent,
  useCallback,
  useEffect,
  useMemo,
  useRef,
  useState,
} from "react";

type Props = {
  project?: Row;
  feature?: Row;
  tasks: Row[];
  agents: Row[];
  configurations?: Row[];
  onLocateExperience?: (entry: Row) => void;
  initialOwner?: string;
  initialSession?: string;
  request: Request;
  refresh: () => Promise<void>;
};
const globalOwner = { id: "global", title: "全局 / 跨项目", revision: 0 };
const createAgentInstanceOption = "__create_agent_instance__";

export function RoleConversation({
  project = globalOwner,
  feature,
  tasks,
  agents,
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
  const [sessionId, setSessionId] = useState(initialSession || "");
  const [detail, setDetail] = useState<Row>();
  const currentBinding =
    detail?.current_runtime_binding || detail?.session.runtime_binding;
  const [events, setEvents] = useState<Row[]>([]);
  const [input, setInput] = useState("");
  const [experienceRefs, setExperienceRefs] = useState<Row[]>([]);
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
    setExperienceRefs([]);
    cursor.current = 0;
  }
  const generation = useRef(0);
  const cursor = useRef(0);
  const listGeneration = useRef(0);
  const detailGeneration = useRef(0);
  const global = owner === "global";
  const selectedOwner = global
    ? globalOwner
    : owner === project.id
      ? project
      : owner === feature?.id
        ? feature
        : tasks.find((t) => t.id === owner);
  const workLock = configurations
    .filter((r) => r.type === "work_configuration" && r.definition_id === owner)
    .at(-1);
  const runtime = configurations
    .filter(
      (r) =>
        r.type === "workflow_instance" &&
        r.data.owner_id === owner &&
        r.work_root !== false,
    )
    .at(-1);
  const plan = runtime?.data.plan.document;
  const recommendedExecution = runtime?.data.steps.find(
    (s: Row) =>
      ["running", "waiting_input", "waiting_confirmation"].includes(s.status) &&
      s.agent_revision_id,
  );
  const recommendation = recommendedExecution?.agent_revision_id;
  const recommended = plan?.steps.find(
    (s: Row) => s.key === recommendedExecution?.step,
  );
  const boundAgents = useMemo(
    () =>
      runtime?.data.plan?.definitions
        ? Object.values(runtime.data.plan.definitions as Row)
            .filter((row: Row) => row.type === "agent")
            .map((row: Row) => row.id as string)
        : (Object.values(
            runtime?.data.role_agent_bindings ||
              workLock?.data.role_agent_bindings ||
              {},
          ) as string[]),
    [
      runtime?.data.plan?.definitions,
      runtime?.data.role_agent_bindings,
      workLock?.data.role_agent_bindings,
    ],
  );
  const composite = catalog.find((r) => r.id === agent && r.type === "agent");
  // Agent instances pin the exact revision they were created with; a Role
  // selection offers the latest revision of that definition. Match instances
  // by agent definition so superseded revisions still load their instances.
  const definitionOf = useCallback(
    (id: string | undefined) => {
      if (!id) return "";
      return (
        catalog.find((r) => r.id === id && r.type === "agent")?.definition_id ||
        id.slice(0, id.lastIndexOf(":"))
      );
    },
    [catalog],
  );
  const selectedDefinition = definitionOf(agent);
  const instances = useMemo(
    () =>
      catalog.filter(
        (r) =>
          r.type === "agent_instance" &&
          definitionOf(r.data.agent_revision_id) === selectedDefinition,
      ),
    [catalog, definitionOf, selectedDefinition],
  );
  const pinnedRevision = instances.find((r) => r.id === instanceId)?.data
    .agent_revision_id;
  // Sessions execute the revision pinned by the instance, not the latest one.
  const running = catalog.find(
    (r) => r.id === (pinnedRevision || agent) && r.type === "agent",
  );
  const capability = catalog.find((r) => r.id === running?.capability_ref);

  useEffect(() => {
    if (!workLock || owner === project.id || global || sessionId) return;
    if (!boundAgents.includes(agent)) {
      setAgent(recommendation || "");
      setInstanceId("");
    }
  }, [
    workLock,
    runtime?.data.active_plan_revision_id,
    owner,
    recommendation,
    sessionId,
    project.id,
    global,
    boundAgents,
    agent,
  ]);
  useEffect(() => {
    let current = true;
    if (!agent || catalog.some((r) => r.id === agent)) return;
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
  }, [agent, request, project.id, catalog]);
  useEffect(() => {
    if (detail?.session.id && detailGeneration.current === generation.current) {
      setAgent(detail.session.agent);
      setInstanceId(
        currentBinding?.agent_configuration?.agent_instance_id ||
          detail.agent_instance_id ||
          "",
      );
    }
  }, [
    currentBinding?.agent_configuration?.agent_instance_id,
    detail?.agent_instance_id,
    detail?.session.agent,
    detail?.session.id,
  ]);

  const choices: Row[] = [
    ...agents,
    ...catalog
      .filter((r) => r.type == "agent" && !agents.some((a) => a.id === r.id))
      .map((r) => ({
        ...r,
        agent_profile: {
          role: catalog.find((role) => role.id === r.role_ref)?.definition_id,
        },
      })),
  ];
  const roles = choices.filter(
    (a) =>
      a.agent_profile &&
      (catalog.some((r) => r.id === a.id && r.type === "agent") ||
        detail?.session.agent === a.id) &&
      (detail?.session.agent === a.id ||
        (workLock && owner !== project.id && !global
          ? boundAgents.includes(a.id)
          : !choices.some((d) => d.supersedes === a.id))),
  );
  useEffect(() => {
    if (sessionId || composite || !catalog.length) return;
    const candidates = catalog.filter(
      (r) =>
        r.type == "agent" &&
        (!workLock ||
          owner === project.id ||
          global ||
          boundAgents.includes(r.id)),
    );
    const selected =
      candidates.find((r) => r.id === recommendation) ||
      candidates.find(
        (r) => !candidates.some((next) => next.supersedes === r.id),
      );
    if (selected)
      setAgent((current) =>
        candidates.some((candidate) => candidate.id === current)
          ? current
          : selected.id,
      );
  }, [
    catalog,
    owner,
    runtime?.data.active_plan_revision_id,
    workLock?.id,
    recommendation,
    sessionId,
    composite,
    workLock,
    project.id,
    global,
    boundAgents,
  ]);
  useEffect(() => {
    if (!sessionId && !instances.some((r) => r.id === instanceId))
      setInstanceId(instances[0]?.id || "");
  }, [instances, sessionId, instanceId]);
  useEffect(() => {
    let current = true;
    (async () => {
      const rows = await loadCatalog(request, global ? undefined : project?.id);
      if (global) {
        let offset = 0;
        for (;;) {
          const page = await request("query.discussion_agents", {
            offset,
            limit: 100,
          });
          for (const row of page.records)
            if (!rows.some((r) => r.id === row.id)) rows.push(row);
          offset += page.records.length;
          if (offset >= page.total || !page.records.length) break;
        }
      }
      return rows;
    })()
      .then((rows) => {
        if (current) setCatalog(rows);
      })
      .catch((e) => {
        if (current) setError(String(e));
      });
    return () => {
      current = false;
    };
  }, [request, project?.id, global, setError]);
  useEffect(() => {
    if (recommendation && !sessionId) {
      setAgent(recommendation);
      setInstanceId("");
    }
  }, [recommendation, owner, sessionId]);
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
  const controller = detail?.controller;
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
  const recommendedStepId = recommendedExecution?.id;
  const loadSessions = useCallback(async () => {
    const current = ++listGeneration.current;
    const result = await scopedRequest("query.sessions", {
      owner_id: owner,
      agent_instance_id: sessionAnchor ? undefined : instanceId || undefined,
      workflow_session_id: sessionAnchor || undefined,
      step_execution_id: sessionAnchor ? undefined : recommendedStepId,
      agent: sessionAnchor ? undefined : agent || undefined,
    });
    if (current === listGeneration.current) {
      setSessions(result.sessions);
      // A deep link pins the requested session before role/instance discovery.
      // Neither a recommended worker nor a newer session may replace it.
      if (sessionAnchor) {
        if (!manualSession.current) setSessionId(sessionAnchor);
        return;
      }
      if (!manualSession.current && (instanceId || agent || sessionAnchor)) {
        const latest = result.sessions.find((s: Row) =>
          instanceId
            ? (s.agent_instance_id ||
                s.runtime_binding?.agent_configuration?.agent_instance_id) ===
              instanceId
            : agent
              ? s.agent === agent
              : !!sessionAnchor,
        );
        setSessionId(latest?.id || "");
      }
    }
  }, [
    scopedRequest,
    owner,
    instanceId,
    sessionAnchor,
    agent,
    recommendedStepId,
  ]);
  useEffect(() => {
    ++generation.current;
    ++listGeneration.current;
    const focus =
      pendingFocus.current?.owner === owner ? pendingFocus.current : null;
    pendingFocus.current = null;
    manualSession.current = false;
    setSessionAnchor(
      focus?.session || (owner === initialOwner ? initialSession || "" : ""),
    );
    setSessionId(
      focus?.session || (owner === initialOwner ? initialSession || "" : ""),
    );
    setDetail(undefined);
    setSessions([]);
    setEvents([]);
    setExperienceRefs([]);
    cursor.current = 0;
  }, [scopedRequest, owner, initialOwner, initialSession]);
  useEffect(() => {
    void loadSessions().catch((e) => setError(String(e)));
  }, [loadSessions, setError]);
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
          detailGeneration.current = current;
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
  }, [sessionId, scopedRequest, captureFeedback, setError]);
  async function action(work: () => Promise<void>) {
    captureFeedback();

    beginFeedback();
    setBusy(true);
    setError("");
    try {
      await work();
    } catch (e) {
      setError(String(e));
    } finally {
      endFeedback();
      setBusy(false);
    }
  }
  async function createAgentInstance() {
    if (!composite) return;
    await action(async () => {
      const created = await request("agent.instance.create", {
        project_id: global ? undefined : project.id,
        command_id: crypto.randomUUID(),
        payload: {
          name: composite.name,
          agent_revision_id: composite.id,
          ...(!global &&
          owner !== project.id &&
          runtime?.data.role_agent_bindings &&
          boundAgents.includes(composite.id)
            ? { frozen_work_id: owner }
            : {}),
          scope: global ? "global" : `project:${project.id}`,
        },
      });
      clearSelectedSession();
      setCatalog(await loadCatalog(request, global ? undefined : project?.id));
      setInstanceId(created.id);
    });
  }
  async function open(forceNew = false) {
    captureFeedback();

    if (!selectedOwner) return;
    if (recommendedExecution && agent === recommendation) {
      if (recommendedExecution.session_id) {
        setSessionId(recommendedExecution.session_id);
        return;
      }
      setError("步骤会话正在由 Workflow 调度，请稍后查看执行状态。");
      return;
    }
    manualSession.current = true;
    await action(async () => {
      const result = await command(
        "session.open",
        owner,
        selectedOwner.revision,
        {
          // The backend freezes the composition pinned by the instance.
          agent: pinnedRevision || agent,
          ...(instanceId ? { agent_instance_id: instanceId } : {}),
          ...(forceNew ? { force_new: true } : {}),
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
      if (controller) {
        await command(
          "orchestration.input",
          controller.id,
          controller.revision,
          {
            epoch: controller.data.epoch,
            input,
          },
        );
        setInput("");
        await refresh();
        return;
      }
      const result = await command(
        "conversation.send",
        sessionId,
        detail.session.revision,
        {
          client_message_id: crypto.randomUUID(),
          input,
          ...(experienceRefs.length
            ? {
                experience_refs: experienceRefs.map((entry) => ({
                  source_project_id: entry.project_id,
                  snapshot_id: entry.snapshot_id,
                })),
              }
            : {}),
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
      setExperienceRefs([]);
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
    if (previous) {
      previous.source_events.push(event);
      previous.content =
        event.mode === "delta"
          ? previous.content + event.content
          : event.content;
    } else projected.push({ ...event, source_events: [event] });
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
                {a.name} v{a.version}
              </option>
            ))}
          </ActionSelect>
        </label>
        {!composite && (
          <p className="notice">
            {detail?.session
              ? "历史会话保留原运行绑定；请选择完整 Agent 组合开启新会话。"
              : "请先在基础组件中准备资源，并在角色与代理中组合所需能力，再选择讨论实例。"}
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
                  if (e.target.value === createAgentInstanceOption) {
                    void createAgentInstance();
                    return;
                  }
                  clearSelectedSession();
                  setInstanceId(e.target.value);
                }}
              >
                <option value="" disabled>
                  请选择已创建的 Agent
                </option>
                {instances.map((r) => (
                  <option key={r.id} value={r.id}>
                    {r.name.replace(/\s*·\s*(?:项目|全局)讨论\s*\d+$/u, "")} ·{" "}
                    {r.origin_project_name ||
                      (r.scope === "global" ? "全局身份" : "项目身份")}{" "}
                    · v
                    {
                      catalog.find((c) => c.id === r.data.agent_revision_id)
                        ?.version
                    }{" "}
                    · {r.id.replace(/^agent-instance\./u, "").slice(0, 8)}
                  </option>
                ))}
                <option value={createAgentInstanceOption}>新建Agent实例</option>
              </ActionSelect>
            </label>
            {!instances.length && (
              <p className="muted">
                在 Agent 下拉列表中选择“新建Agent实例”，即可独立保留会话与经验。
              </p>
            )}
            {instanceId && (
              <p className="muted">
                该 Agent 的经验范围：
                {instances.find((r) => r.id === instanceId)?.scope === "global"
                  ? "全局 / 跨项目"
                  : instances.find((r) => r.id === instanceId)
                      ?.origin_project_name || "来源项目"}
                ；已有工作按此身份保留。
              </p>
            )}
            <p>
              Model{" "}
              {catalog.find((r) => r.id === capability?.model_ref)?.data
                .model || capability?.model_ref}{" "}
              · Harness{" "}
              {
                catalog.find((r) => r.id === capability?.harness_ref)?.data
                  .provider
              }{" "}
              /{" "}
              {
                catalog.find((r) => r.id === capability?.harness_ref)?.data
                  .runtime_version
              }{" "}
              · Agent v{running?.version ?? composite.version}
            </p>
            <details className="agent-capabilities">
              <summary>Agent 完整能力</summary>
              <div className="agent-capability-lists">
                {(
                  [
                    ["skills", "Skills"],
                    ["tools", "Tools"],
                    ["spec_refs", "Agent Specs"],
                  ] as const
                ).map(([key, label]) => (
                  <div key={key}>
                    <strong>{label}</strong>
                    <ul>
                      {(capability?.[key] || []).map((id: string) => (
                        <li key={id}>
                          {catalog.find((row) => row.id === id)?.name || id}
                          {currentBinding?.agent_configuration?.integration_lock
                            ?.unbound_reasons?.[id] && (
                            <span>
                              {" · "}
                              {
                                currentBinding.agent_configuration
                                  .integration_lock.unbound_reasons[id]
                              }
                            </span>
                          )}
                        </li>
                      ))}
                    </ul>
                  </div>
                ))}
              </div>
              <p>Agent 经验：检索与读取，可在当前讨论中按需使用。</p>
            </details>
            <details>
              <summary>Role / Capability 及定义 Scope</summary>
              <pre>{JSON.stringify(running || composite, null, 2)}</pre>
            </details>
          </>
        )}
        <ActionButton
          disabled={busy || !selectedOwner || !composite || !instanceId}
          onClick={() => open()}
        >
          开始 / 续用讨论
        </ActionButton>
        <ActionButton
          disabled={
            busy ||
            !selectedOwner ||
            !composite ||
            !instanceId ||
            formalOutput ||
            !!active
          }
          onClick={() => open(true)}
        >
          新建 Session
        </ActionButton>
      </div>
      {recommended && (
        <p className="notice">
          工作流推荐：{recommended.name} ·{" "}
          {roles.find((r) => r.id === recommendation)?.name}。目标：
          {recommended.task_instructions}；产物：
          {Object.keys(recommended.outputs || {}).join("、") || "未声明"}
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
        onReference={
          formalOutput
            ? undefined
            : (entry) =>
                setExperienceRefs((old) =>
                  old.some(
                    (r) =>
                      r.snapshot_id === entry.snapshot_id &&
                      r.project_id === entry.project_id,
                  )
                    ? old
                    : [...old, entry],
                )
        }
        onRestored={(id) => {
          manualSession.current = true;
          void loadSessions();
          setSessionId(id);
        }}
        onLocate={(entry) => {
          if (entry.project_id === "00000000-0000-4000-8000-000000000000") {
            if (global) {
              manualSession.current = true;
              setSessionId(entry.session_id);
            } else {
              pendingFocus.current = {
                owner: "global",
                session: entry.session_id,
              };
              setOwner("global");
            }
          } else onLocateExperience?.(entry);
        }}
      />
      {experienceRefs.length > 0 && (
        <div className="notice" aria-label="当前讨论引用的经验">
          {experienceRefs.map((entry) => (
            <p key={`${entry.project_id}:${entry.snapshot_id}`}>
              {entry.project_name} / {entry.feature_name || "讨论"}
            </p>
          ))}
          <ActionButton onClick={() => setExperienceRefs([])}>
            清除经验引用
          </ActionButton>
        </div>
      )}
      {detail?.experience_reads?.length > 0 && (
        <details>
          <summary>Agent 在本次讨论中读取的经验</summary>
          {(detail?.experience_reads || []).map((entry: Row) => (
            <p key={entry.id}>
              {entry.source
                ? `${entry.source.project_name} / ${entry.source.feature_name || "讨论"}`
                : "检索历史经验"}
              {" · "}
              {entry.status === "succeeded"
                ? "已读取"
                : entry.status === "prepared"
                  ? "读取中"
                  : "读取失败"}
              {entry.error?.message ? `：${entry.error.message}` : ""}
            </p>
          ))}
        </details>
      )}
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
              currentBinding.agent_configuration?.model_provider ||
              (currentBinding.installation.key.provider === "dsh"
                ? "deepseek"
                : "openai")}{" "}
            · {currentBinding.model} · {detail.session.lifecycle} · 原生会话{" "}
            {detail.session.external_session || "尚未启动"}
          </p>
          {currentBinding.model_connection && (
            <p className="muted">
              模型地址 <code>{currentBinding.model_connection.endpoint}</code> ·
              容器地址{" "}
              <code>{currentBinding.model_connection.worker_endpoint}</code>
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
              <ReadableValue value={currentBinding.permissions} />
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
              <ConversationContent
                key={e.id}
                event={e}
                projectId={global ? undefined : project.id}
                sessionId={sessionId}
                ownerId={owner}
              />
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
              disabled={formalOutput && !controller}
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
                (controller
                  ? !["active", "budget_exhausted"].includes(
                      controller.data.status,
                    )
                  : formalOutput ||
                    !!active ||
                    [
                      "needs_reconciliation",
                      "requires_successor",
                      "closed",
                    ].includes(detail.session.lifecycle))
              }
            >
              发送
            </ActionButton>
          </ActionForm>
          {controller && (
            <p className="muted">
              消息会交给项目经理处理；需求澄清可直接在此回答，经理会转交你的原话并推进流程。
            </p>
          )}
          {formalOutput && !controller && (
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
              选择已配置的交付类型并保存结构化材料。采纳会校验来源轮次、目标版本和内容结构。
            </p>
            <textarea
              aria-label="结构化提案 JSON"
              value={proposalJson}
              onChange={(e) => {
                captureFeedback();
                return setProposalJson(e.target.value);
              }}
              placeholder={
                '{"delivery_ref":"交付类型修订 ID","title":"标题","value":"材料正文","input_refs":[]}'
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
                  {p.content.title} · {p.status}
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
                </div>
              </article>
            ))}
          </details>
        </>
      )}
    </div>
  );
}

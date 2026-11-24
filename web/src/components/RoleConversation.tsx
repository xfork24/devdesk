import { displayText, shortId } from "../display-text";
import { useReadRequest } from "./use-read-request";
import { ReadableValue } from "./reading";
import { ConversationContent } from "./ConversationContent";
import { ActionButton, ActionForm, ActionSelect } from "./ActionFeedback";
import { useActionFeedback } from "./action-feedback";
import { ExecutionTaskDetails } from "./WorkflowTaskPreview";
import { loadCatalog } from "./configuration-state";
import { MarkdownField } from "./configuration-ui";
import { type Request, type Row } from "./rpc-types";
import { RoleExperience } from "./RoleExperience";
import { experienceKey } from "./experience-reference";
import { configurationHref } from "./configuration-links";
import { ConfigurationLink } from "./ConfigurationLink";
import { WorkflowReferences } from "./WorkflowReferences";
import { useReading } from "./reading-state";
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
  configurations?: Row[];
  onLocateExperience?: (entry: Row) => void;
  onDiscussionDeleted?: (session: string, turn?: string) => void;
  initialOwner?: string;
  initialSession?: string;
  initialTurn?: string;
  request: Request;
  refresh: () => Promise<void>;
};
const globalOwner = { id: "global", title: "全局 / 跨项目", revision: 0 };
const migratedRoles = new Set([
  "analyst",
  "architect",
  "developer",
  "project-manager",
  "reviewer",
  "tester",
]);

function roleIdentity(ref: string | undefined) {
  return ref?.replace(/:[1-9]\d*$/, "") || "";
}

export function RoleConversation({
  project = globalOwner,
  feature,
  tasks,
  configurations = [],
  onLocateExperience,
  onDiscussionDeleted,
  initialOwner,
  initialSession,
  initialTurn,
  request: parentRequest,
  refresh,
}: Props) {
  const request = useReadRequest(parentRequest);
  const {
    error,
    setError,
    capture: captureFeedback,
    beginFeedback,
    endFeedback,
  } = useActionFeedback();

  const [owner, setOwner] = useState(initialOwner || project.id);
  const [catalog, setCatalog] = useState<Row[]>([]);
  const [selectedRole, setSelectedRole] = useState("");
  const [agent, setAgent] = useState("");
  const [editing, setEditing] = useState<Row>();
  const [sessions, setSessions] = useState<Row[]>([]);
  const [sessionId, setSessionId] = useState(initialSession || "");
  const [turnId, setTurnId] = useState(initialTurn || "");
  useEffect(() => {
    setTurnId(sessionId === initialSession ? initialTurn || "" : "");
  }, [sessionId, initialSession, initialTurn]);
  const [detail, setDetail] = useState<Row>();
  const workflowContext = detail?.workflow_context;
  const executionSource = useReading(
    request,
    "query.workflow_executions",
    {
      project_id: project.id,
      owner_id: detail?.session.owner_id,
      instance_id: workflowContext?.instance_id,
      execution_id: workflowContext?.step_instance_id,
    },
    Boolean(workflowContext?.step_instance_id && project.id !== "global"),
  );
  const currentBinding =
    detail?.current_runtime_binding || detail?.session.runtime_binding;
  const [events, setEvents] = useState<Row[]>([]);
  const [input, setInput] = useState("");
  const [approvalNotice, setApprovalNotice] = useState("");
  const messageDrafts = useRef(
    new Map<
      string,
      {
        input: string;
        revision: number;
        target: string;
        epoch?: number;
        approvalContext?: { id: string; revision: number }[];
        commandId: string;
        clientId: string;
        sent?: string;
      }
    >(),
  );

  const [experienceRefs, setExperienceRefs] = useState<Row[]>([]);
  const [busy, setBusy] = useState(false);

  const [proposalJson, setProposalJson] = useState("");
  const [answers, setAnswers] = useState<Record<string, string>>({});
  const [selections, setSelections] = useState<Record<string, string[]>>({});
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
  const composer = useRef<HTMLFormElement>(null);
  const [historyRefresh, refreshHistory] = useState(0);
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
  const runtime = configurations
    .filter(
      (r) =>
        r.type === "workflow_instance" &&
        r.data.owner_id === owner &&
        r.work_root !== false,
    )
    .at(-1);
  const plan = runtime?.data.plan?.document;
  const recommendedExecution = runtime?.data.steps?.find(
    (s: Row) =>
      ["running", "waiting_input", "waiting_confirmation"].includes(s.status) &&
      s.agent_revision_id,
  );
  const recommendation = recommendedExecution?.agent_revision_id;
  const recommended = plan?.steps?.find(
    (s: Row) => s.key === recommendedExecution?.step,
  );
  const composite = catalog.find((r) => r.id === agent && r.type === "agent");
  const capability = catalog.find((r) => r.id === composite?.capability_ref);
  const [exactCapability, setExactCapability] = useState<Row>();
  const capabilityRef = composite?.capability_ref;
  useEffect(() => {
    if (!capabilityRef || capability?.skills) return;
    const controller = new AbortController();
    request(
      "query.configuration_records",
      {
        refs: [capabilityRef],
        project_id: global ? undefined : project.id,
        local: !global,
      },
      { signal: controller.signal },
    )
      .then(({ records }) => {
        if (!controller.signal.aborted) setExactCapability(records[0]);
      })
      .catch(() => {});
    return () => controller.abort();
  }, [capabilityRef, capability?.skills, request, global, project.id]);
  const displayedCapability =
    exactCapability?.id === capabilityRef ? exactCapability : capability;
  const roles = useMemo(
    () => catalog.filter((r) => r.type === "role" && !r.archived),
    [catalog],
  );
  const roleOf = useCallback(
    (record: Row | undefined) => {
      const identity = roleIdentity(record?.role_ref);
      const legacy = identity.startsWith("builtin-role-")
        ? identity.slice("builtin-role-".length)
        : "";
      const packaged = `software-development.role-${legacy}`;
      return migratedRoles.has(legacy) &&
        roles.some((r) => r.definition_id === packaged)
        ? packaged
        : identity;
    },
    [roles],
  );
  const roleId =
    roleOf(composite) || selectedRole || roles[0]?.definition_id || "";
  const pinned = detail?.session.agent === agent || Boolean(sessionAnchor);
  const roleChoices =
    roles.some((r) => r.definition_id === roleId) || !roleId || !pinned
      ? roles
      : [...roles, { definition_id: roleId, name: roleId }];
  const agentChoices = catalog.filter(
    (row) =>
      row.type === "agent" &&
      roleOf(row) === roleId &&
      ((row.id === agent && pinned) ||
        (!row.archived && (!global || row.scope === "global"))),
  );
  const canOpen =
    composite &&
    !composite.archived &&
    agentChoices.some((row) => row.id === composite.id);
  useEffect(() => {
    if (!agent || catalog.some((r) => r.id === agent)) return;
    let current = true;
    request("query.agent_revision", {
      id: agent,
      project_id: global ? undefined : project.id,
    })
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
  }, [agent, request, project.id, global, catalog]);
  useEffect(() => {
    if (detail?.session.id && detailGeneration.current === generation.current)
      setAgent(detail.session.agent);
  }, [detail?.session.agent, detail?.session.id]);
  useEffect(() => {
    const controller = new AbortController();
    loadCatalog(
      request,
      global ? undefined : project.id,
      false,
      undefined,
      false,
      controller.signal,
    )
      .then((rows) => {
        if (!controller.signal.aborted) setCatalog(rows);
      })
      .catch((e) => {
        if (!controller.signal.aborted) setError(String(e));
      });
    return () => controller.abort();
  }, [request, project.id, global, setError]);
  useEffect(() => {
    if (sessionId || sessionAnchor || !catalog.length) return;
    if (agent && catalog.some((r) => r.id === agent && !r.archived)) return;
    const candidates = catalog.filter(
      (row) =>
        row.type === "agent" &&
        !row.archived &&
        (!global || row.scope === "global") &&
        roles.some((role) => role.definition_id === roleOf(row)) &&
        (!selectedRole || roleOf(row) === selectedRole),
    );
    const selected =
      candidates.find((row) => row.id === recommendation) ||
      candidates.find(
        (row) =>
          row.definition_id ===
          "software-development.agent-gpt-6-2-sol-project-manager",
      ) ||
      candidates[0];
    if (selected) setAgent(selected.id);
  }, [
    agent,
    catalog,
    global,
    recommendation,
    roleOf,
    roles,
    selectedRole,
    sessionAnchor,
    sessionId,
  ]);
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
  const acceptsExperience = !formalOutput && !controller;
  const requiresContinuation =
    detail?.permission_status?.requires_successor === true ||
    ["requires_successor", "closed", "needs_reconciliation"].includes(
      detail?.session.lifecycle,
    );
  const selectedTurn = detail?.turns.find((turn: Row) => turn.id === turnId);
  const continuationAgent =
    currentBinding?.agent_configuration?.agent_instance_id ||
    detail?.agent_instance_id;
  const continuationReady =
    (!active || active.status === "needs_reconciliation") &&
    Boolean(continuationAgent);
  const canDeleteHistory = acceptsExperience && !active;
  const anchoredTurn = sessionId === initialSession ? initialTurn : undefined;
  const messageKey = `${project.id}:${owner}:${sessionId}`;
  useEffect(() => {
    setInput(messageDrafts.current.get(messageKey)?.input ?? "");
  }, [messageKey]);
  const changeInput = (value: string) => {
    const previous = messageDrafts.current.get(messageKey);
    if (detail && !previous)
      messageDrafts.current.set(messageKey, {
        input: value,
        revision: controller?.revision ?? detail.session.revision,
        target: controller?.id ?? sessionId,
        epoch: controller?.data.epoch,
        approvalContext: (detail.approval_context || []).map(
          (decision: Row) => ({ id: decision.id, revision: decision.revision }),
        ),
        commandId: crypto.randomUUID(),
        clientId: crypto.randomUUID(),
      });
    else if (previous) previous.input = value;
    setInput(value);
  };

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
    commandId: string = crypto.randomUUID(),
  ) =>
    scopedRequest(method, {
      command_id: commandId,
      target,
      expected_revision,
      payload,
    });
  const recommendedStepId =
    agent === recommendation ? recommendedExecution?.id : undefined;
  const loadSessions = useCallback(
    async (anchor = sessionAnchor, removed?: string) => {
      if (!anchor && !agent) return;
      const current = ++listGeneration.current;
      const result = await scopedRequest("query.sessions", {
        owner_id: owner,
        workflow_session_id: anchor || undefined,
        step_execution_id: anchor ? undefined : recommendedStepId,
        agent: anchor ? undefined : agent || undefined,
      });
      if (current === listGeneration.current) {
        const visible = (result.sessions as Row[]).filter(
          (row) => row.id !== removed,
        );
        setSessions(visible);
        // A deep link pins the requested session before role/instance discovery.
        // Neither a recommended worker nor a newer session may replace it.
        if (anchor) {
          if (!manualSession.current) setSessionId(anchor);
          return;
        }
        if (!manualSession.current) {
          const latest = visible.find((s: Row) => s.agent === agent);
          setSessionId(latest?.id || "");
        }
      }
    },
    [scopedRequest, owner, sessionAnchor, agent, recommendedStepId],
  );
  useEffect(() => {
    ++generation.current;
    ++listGeneration.current;
    manualSession.current = false;
    const initial = owner === initialOwner ? initialSession || "" : "";
    setSessionAnchor(initial);
    setSessionId(initial);
    setDetail(undefined);
    setSessions([]);
    setEvents([]);
    setExperienceRefs([]);
    cursor.current = 0;
  }, [global, project.id, owner, initialOwner, initialSession]);
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
            anchor_turn_id: anchoredTurn,
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
  }, [
    sessionId,
    scopedRequest,
    captureFeedback,
    setError,
    historyRefresh,
    anchoredTurn,
  ]);
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
  async function open(forceNew = false) {
    captureFeedback();

    if (!selectedOwner || !canOpen) return;
    if (recommendedExecution && agent === recommendation) {
      if (recommendedExecution.session_id) {
        setSessionId(recommendedExecution.session_id);
        return;
      }
      setError("步骤会话正在由 Workflow 调度，请稍后查看执行状态。");
      return;
    }
    if (!forceNew && detail?.session.agent === agent) {
      if (requiresContinuation) return continueDiscussion();
      composer.current?.scrollIntoView({ block: "nearest" });
      return;
    }
    manualSession.current = true;
    await action(async () => {
      const result = await command(
        "session.open",
        owner,
        selectedOwner.revision,
        {
          agent,
          ...(forceNew ? { force_new: true } : {}),
        },
      );
      await loadSessions();
      if (sessionId && result.id !== sessionId) setExperienceRefs([]);
      setSessionId(result.id);
    });
  }
  function continueDiscussion(turn?: Row) {
    if (!detail || !continuationReady) return;
    if (!turn && !requiresContinuation && acceptsExperience) {
      setTurnId("");
      composer.current?.scrollIntoView({ block: "nearest" });
      return;
    }
    const source = detail.session;
    return action(async () => {
      const result = await command(
        "session.open",
        source.owner_id,
        source.revision,
        {
          agent_instance_id: continuationAgent,
          continue_from: {
            session_id: source.id,
            ...(turn ? { turn_id: turn.id, turn_revision: turn.revision } : {}),
          },
        },
      );
      if (input.trim()) {
        messageDrafts.current.set(`${project.id}:${owner}:${result.id}`, {
          input,
          revision: result.revision,
          target: result.id,
          commandId: crypto.randomUUID(),
          clientId: crypto.randomUUID(),
        });
        messageDrafts.current.delete(messageKey);
      }
      manualSession.current = true;
      setTurnId("");
      setSessionAnchor(result.id);
      setSessionId(result.id);
    });
  }

  function deleteDiscussion(turn?: Row) {
    if (!detail || !canDeleteHistory) return;
    const source = detail.session;
    const title = turn
      ? `轮次 ${shortId(turn.id)}`
      : `会话 ${shortId(source.id)}`;
    if (!window.confirm(`从角色讨论中删除${title}？`)) return;
    return action(async () => {
      await command(
        turn ? "conversation.delete_turn" : "session.delete",
        source.id,
        source.revision,
        turn ? { turn_id: turn.id, turn_revision: turn.revision } : {},
      );
      generation.current += 1;
      setTurnId("");
      if (turn) {
        refreshHistory((version) => version + 1);
      } else {
        messageDrafts.current.delete(messageKey);
        manualSession.current = false;
        setSessionId("");
        setSessionAnchor("");
        setSessions((rows) => rows.filter((row) => row.id !== source.id));
        setExperienceRefs([]);
        await loadSessions("", source.id);
      }
      onDiscussionDeleted?.(source.id, turn?.id);
    });
  }

  async function send(event: FormEvent) {
    captureFeedback();

    event.preventDefault();
    if (!detail || !input.trim()) return;
    const basis = messageDrafts.current.get(messageKey) ?? {
      input,
      revision: controller?.revision ?? detail.session.revision,
      target: controller?.id ?? sessionId,
      epoch: controller?.data.epoch,
      approvalContext: (detail.approval_context || []).map((decision: Row) => ({
        id: decision.id,
        revision: decision.revision,
      })),
      commandId: crypto.randomUUID(),
      clientId: crypto.randomUUID(),
      sent: undefined as string | undefined,
    };
    const references = acceptsExperience ? experienceRefs : [];
    const fingerprint = JSON.stringify({ input, references });
    if (basis.sent && basis.sent !== fingerprint) {
      basis.commandId = crypto.randomUUID();
      basis.clientId = crypto.randomUUID();
    }
    basis.sent = fingerprint;
    messageDrafts.current.set(messageKey, basis);
    await action(async () => {
      if (controller) {
        const result = await command(
          "orchestration.input",
          basis.target,
          basis.revision,
          {
            epoch: basis.epoch,
            input,
            approval_context: basis.approvalContext || [],
          },
          basis.commandId,
        );
        setApprovalNotice(
          result.approval?.status === "approved"
            ? "已登记人工批准并处理当前结果，后续步骤由流程驱动继续执行。"
            : "",
        );
        messageDrafts.current.delete(messageKey);
        setInput("");
        await refresh();
        return;
      }
      const result = await command(
        "conversation.send",
        basis.target,
        basis.revision,
        {
          client_message_id: basis.clientId,
          input,
          ...(references.length
            ? {
                experience_refs: references.map((entry) => ({
                  source_project_id: entry.project_id,
                  snapshot_id: entry.snapshot_id,
                  ...(entry.excerpt ? { excerpt: entry.excerpt } : {}),
                })),
              }
            : {}),
        },
        basis.commandId,
      );
      if (
        result.effective_session_id &&
        result.effective_session_id !== sessionId
      ) {
        await loadSessions();
        setSessionId(result.effective_session_id);
      }
      messageDrafts.current.delete(messageKey);
      setInput("");
    });
  }
  function removeReference(entry: Row) {
    setExperienceRefs((old) =>
      old.filter((row) => experienceKey(row) !== experienceKey(entry)),
    );
  }
  const references = acceptsExperience && experienceRefs.length > 0 && (
    <section
      className="notice experience-references"
      aria-label="当前讨论引用的经验"
    >
      <strong>当前讨论引用的经验</strong>
      <ul>
        {experienceRefs.map((entry) => {
          const title = `${entry.project_name} / ${entry.feature_name || "讨论"}`;
          return (
            <li key={experienceKey(entry)}>
              <span>{displayText(title)}</span>
              <ActionButton
                disabled={busy}
                aria-label={`清除引用 ${title}`}
                onClick={() => removeReference(entry)}
              >
                清除引用
              </ActionButton>
            </li>
          );
        })}
      </ul>
    </section>
  );
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
            选择角色及其 Agent 开始讨论，并查看该角色积累的经验。
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
              clearSelectedSession();
              setSelectedRole("");
              setAgent("");
              setOwner(e.target.value);
            }}
          >
            <option value="global">全局 / 跨项目</option>
            {project.id !== "global" && (
              <option value={project.id}>
                项目 · {displayText(project.title)}
              </option>
            )}
            {feature && (
              <option value={feature.id}>
                Feature · {displayText(feature.title)}
              </option>
            )}
            {tasks.map((t) => (
              <option key={t.id} value={t.id}>
                Task · {displayText(t.title)}
              </option>
            ))}
          </ActionSelect>
        </label>
        <label>
          角色
          <ActionSelect
            aria-label="讨论角色"
            disabled={busy || !roleChoices.length}
            value={roleId}
            onChange={(e) => {
              captureFeedback();
              clearSelectedSession();
              setSelectedRole(e.target.value);
              setAgent("");
            }}
          >
            <option value="" disabled>
              请选择角色
            </option>
            {roleChoices.map((role) => (
              <option key={role.definition_id} value={role.definition_id}>
                {displayText(role.name)}
              </option>
            ))}
          </ActionSelect>
        </label>
        <label>
          Agent
          <ActionSelect
            aria-label="Agent"
            disabled={busy || !agentChoices.length}
            value={agent}
            onChange={(e) => {
              captureFeedback();
              clearSelectedSession();
              setSelectedRole(roleId);
              setAgent(e.target.value);
            }}
          >
            <option value="" disabled>
              请选择 Agent
            </option>
            {agentChoices.map((row) => (
              <option key={row.id} value={row.id}>
                {displayText(row.name)} v{displayText(row.version)}
              </option>
            ))}
          </ActionSelect>
        </label>
        <div className="discussion-agent-info" aria-label="Agent信息">
          {composite && (
            <section aria-label="Agent能力">
              <h3>Agent能力</h3>
              <div className="agent-information-content">
                <p>
                  {["model_ref", "harness_ref"]
                    .map((key) => {
                      const id = displayedCapability?.[key];
                      return catalog.find((row) => row.id === id)?.name || id;
                    })
                    .filter(Boolean)
                    .join(" · ")}
                </p>
                <div className="agent-capability-lists">
                  {(
                    [
                      ["skills", "Skills"],
                      ["tools", "Tools"],
                      ["spec_refs", "Agent Specs"],
                    ] as const
                  ).map(([key, label]) => (
                    <div key={key}>
                      <strong>{displayText(label)}</strong>
                      <ul>
                        {(displayedCapability?.[key] || []).map(
                          (id: string) => (
                            <li key={id}>
                              {displayText(
                                catalog.find((row) => row.id === id)?.name ||
                                  id,
                              )}
                              <ConfigurationLink
                                href={configurationHref(
                                  catalog.find((row) => row.id === id) || {
                                    id,
                                    type:
                                      key === "skills"
                                        ? "skill"
                                        : key === "tools"
                                          ? "tool"
                                          : "spec",
                                  },
                                  global ? undefined : project.id,
                                )}
                                label={`查看 ${catalog.find((row) => row.id === id)?.name || id}`}
                              />
                              {currentBinding?.agent_configuration
                                ?.integration_lock?.unbound_reasons?.[id] && (
                                <span>
                                  {" · "}
                                  {displayText(
                                    currentBinding.agent_configuration
                                      .integration_lock.unbound_reasons[id],
                                  )}
                                </span>
                              )}
                            </li>
                          ),
                        )}
                      </ul>
                    </div>
                  ))}
                </div>
              </div>
            </section>
          )}
          <RoleExperience
            revision={lastCompleted?.id}
            key={`${global ? "global" : project.id}:${roleId}`}
            request={request}
            projectId={global ? "global" : project.id}
            roleId={roleId}
            currentInstance={continuationAgent}
            owner={selectedOwner || project}
            references={experienceRefs}
            onReference={
              !acceptsExperience || busy
                ? undefined
                : (entry) => {
                    const key = experienceKey(entry);
                    setExperienceRefs((old) =>
                      old.some((row) => experienceKey(row) === key)
                        ? old.filter((row) => experienceKey(row) !== key)
                        : [...old, entry],
                    );
                  }
            }
            onDeleted={removeReference}
            onRestored={(id) => {
              manualSession.current = true;
              setExperienceRefs([]);
              setSessionAnchor(id);
              setSessionId(id);
            }}
            onLocate={onLocateExperience}
          />
        </div>
        <ActionButton
          disabled={busy || !selectedOwner || !canOpen}
          onClick={() => open()}
        >
          开始 / 续用讨论
        </ActionButton>
        <ActionButton
          disabled={
            busy || !selectedOwner || !canOpen || formalOutput || !!active
          }
          onClick={() => open(true)}
        >
          新建 Session
        </ActionButton>
      </div>
      {recommended && (
        <p className="notice">
          工作流推荐：{displayText(recommended.name)} ·{" "}
          {displayText(catalog.find((r) => r.id === recommendation)?.name)}
          。目标：
          {displayText(recommended.task_instructions)}；产物：
          {displayText(
            Object.keys(recommended.outputs || {}).join("、") || "未声明",
          )}
        </p>
      )}
      {!detail && references}
      {detail?.experience_reads?.length > 0 && (
        <details>
          <summary>Agent 在本次讨论中读取的经验</summary>
          {(detail?.experience_reads || []).map((entry: Row) => (
            <p key={entry.id}>
              {displayText(
                entry.source
                  ? `${entry.source.project_name} / ${entry.source.feature_name || "讨论"}`
                  : entry.method === "agent.experience.read"
                    ? "读取经验快照"
                    : "检索历史经验",
              )}
              {" · "}
              {displayText(
                entry.status === "succeeded"
                  ? "已读取"
                  : ["prepared", "running"].includes(entry.status)
                    ? "读取中"
                    : "读取失败",
              )}
              {displayText(
                entry.error?.message ? `：${entry.error.message}` : "",
              )}
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
            setExperienceRefs([]);
            return setSessionId(e.target.value);
          }}
        >
          <option value="">选择会话</option>
          {sessions
            .filter((s) => s.id === sessionId || s.agent === agent)
            .map((s) => (
              <option key={s.id} value={s.id}>
                {displayText(s.runtime_binding.installation.key.provider)} ·{" "}
                {displayText(s.agent)} · {displayText(s.lifecycle)} ·{" "}
                {displayText(shortId(s.id))}
              </option>
            ))}
        </ActionSelect>
      </label>
      {error && (
        <div className="error" role="alert">
          {displayText(error)}
        </div>
      )}
      {detail && (
        <>
          <section aria-label="会话与流程关联">
            <p>
              会话 ID：<code>{displayText(detail.session.id)}</code>
            </p>
            <label>
              讨论轮次
              <ActionSelect
                aria-label="讨论轮次"
                value={turnId}
                onChange={(event) => setTurnId(event.target.value)}
              >
                <option value="">全部轮次</option>
                {turnId &&
                  !(detail.turns || []).some(
                    (turn: Row) => turn.id === turnId,
                  ) && (
                    <option value={turnId}>
                      定位轮次 · {displayText(turnId)}
                    </option>
                  )}
                {(detail.turns || []).map((turn: Row) => (
                  <option key={turn.id} value={turn.id}>
                    {displayText(turn.id)} · {displayText(turn.status)}
                  </option>
                ))}
              </ActionSelect>
            </label>
            <div className="actions">
              <ActionButton
                disabled={busy || !continuationReady}
                onClick={() => continueDiscussion()}
              >
                继续此会话
              </ActionButton>
              {selectedTurn && (
                <ActionButton
                  disabled={busy || !continuationReady}
                  onClick={() => continueDiscussion(selectedTurn)}
                >
                  从此轮次继续
                </ActionButton>
              )}
              {acceptsExperience && (
                <>
                  <ActionButton
                    disabled={busy || !canDeleteHistory}
                    onClick={() => deleteDiscussion()}
                  >
                    删除会话
                  </ActionButton>
                  {selectedTurn && (
                    <ActionButton
                      disabled={busy || !canDeleteHistory}
                      onClick={() => deleteDiscussion(selectedTurn)}
                    >
                      删除此轮次
                    </ActionButton>
                  )}
                </>
              )}
            </div>
            {requiresContinuation && (
              <p className="notice">
                此会话需要通过后继会话续聊。点击继续此会话会继承已保存的上下文。
              </p>
            )}
            {workflowContext?.step_instance_id ? (
              <>
                <p className="muted">
                  此会话绑定流程步骤；这里查看 Agent
                  对话，上下文页查看同一轮次的冻结输入，执行记录页核对决定与回执。
                </p>
                <WorkflowReferences
                  source={{
                    ...executionSource.value?.record,
                    ...workflowContext,
                    execution_id: workflowContext.step_instance_id,
                    session_id: detail.session.id,
                    turn_id: turnId || executionSource.value?.record?.turn_id,
                    outputs: (
                      executionSource.value?.record?.outputs || []
                    ).filter(
                      (output: Row) => !turnId || output.turn_id === turnId,
                    ),
                  }}
                  projectId={project.id}
                  ownerId={detail.session.feature_id || project.id}
                  current="discussion"
                />
              </>
            ) : (
              <p className="muted">
                {displayText(
                  controller
                    ? "项目经理编排会话：用于统筹和人类交互，不是工作步骤的执行会话。"
                    : "独立角色讨论：未绑定流程步骤，因此没有对应的流程执行或步骤上下文。",
                )}
              </p>
            )}
          </section>
          {detail.session.predecessor &&
          detail.predecessor_available !== false ? (
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
            {displayText(currentBinding.installation.key.provider)} ·{" "}
            {displayText(
              currentBinding.model_connection?.provider ||
                currentBinding.agent_configuration?.model_provider ||
                (currentBinding.installation.key.provider === "dsh"
                  ? "deepseek"
                  : "openai"),
            )}{" "}
            · {displayText(currentBinding.model)} ·{" "}
            {displayText(detail.session.lifecycle)} · 原生会话{" "}
            {displayText(detail.session.external_session || "尚未启动")}
          </p>
          {currentBinding.model_connection && (
            <p className="muted">
              模型地址{" "}
              <code>
                {displayText(currentBinding.model_connection.endpoint)}
              </code>{" "}
              · 容器地址{" "}
              <code>
                {displayText(currentBinding.model_connection.worker_endpoint)}
              </code>
            </p>
          )}
          <details>
            <summary>本次会话的冻结权限</summary>
            {detail.permission_status ? (
              <div>
                <p>
                  权限状态：
                  {displayText(
                    (
                      {
                        adopted: "已采用当前绑定",
                        pending: "当前工作结束后切换",
                        ready_to_switch: "下一轮将切换到新绑定",
                        unconfigured: "目标权限未就绪",
                        needs_reconciliation: "权限切换需要核对停止回执",
                      } as Record<string, string>
                    )[detail.permission_status.status] ||
                      detail.permission_status.status,
                  )}
                </p>
                {detail.permission_status.reason ? (
                  <p role="alert">
                    {displayText(detail.permission_status.reason)}
                  </p>
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
                执行实例 {displayText(e.id)} 已停止，清理失败。
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
              继承自 {displayText(detail.session.predecessor)}，上下文快照{" "}
              {displayText(detail.session.snapshot_id)}
            </p>
          )}
          {references}
          <div role="log" aria-label="讨论记录" className="conversation-log">
            {projected
              .filter((event) => !turnId || event.turn_id === turnId)
              .map((e) => (
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
              当前轮次：{displayText(active.status)} · 输入
              {displayText(active.consumed ? "已消费" : "尚未确认消费")}{" "}
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
                  {displayText(
                    i.kind === "tool_approval"
                      ? "运行时工具审批"
                      : "Agent 澄清问题",
                  )}
                </h3>
                <p>{displayText(i.reason)}</p>
                {i.tool && <pre>{displayText(i.tool)}</pre>}
                {i.questions.map((q: Row) => (
                  <label key={q.id}>
                    {displayText(q.question)}
                    {q.detail && <pre>{displayText(q.detail)}</pre>}
                    {q.options.length > 0 && (
                      <ActionSelect
                        aria-label={displayText(q.question + "选项")}
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
                            {displayText(option)}
                          </option>
                        ))}
                      </ActionSelect>
                    )}
                    <input
                      aria-label={displayText(q.question)}
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
                      {displayText(
                        (
                          {
                            allowed_once: "仅允许本次",
                            rejected: "拒绝",
                            cancelled: "取消",
                            answer: "提交回答",
                          } as Row
                        )[decision],
                      )}
                    </ActionButton>
                  ))}
                </div>
              </div>
            ))}
          <ActionForm ref={composer} onSubmit={send}>
            <MarkdownField
              label="讨论输入"
              disabled={formalOutput && !controller}
              value={input}
              onChange={(value) => {
                captureFeedback();
                changeInput(value);
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
                  : formalOutput || !!active || requiresContinuation)
              }
            >
              发送
            </ActionButton>
          </ActionForm>
          {controller && (
            <p className="muted">
              消息会交给项目经理处理。仅有一个待审批事项时，明确回复“确认”或“确认PRD，继续执行后续步骤”会登记为你对当前版本的批准；多个事项请到项目经理页分别确认。
            </p>
          )}
          {controller && detail.approval_context?.length > 0 && (
            <p role="status">
              当前待审批：
              {displayText(
                detail.approval_context
                  .map(
                    (decision: Row) =>
                      decision.data.request.interaction?.title || "流程审批",
                  )
                  .join("、"),
              )}
            </p>
          )}
          {controller && approvalNotice && (
            <p role="status">{approvalNotice}</p>
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
                {displayText(editing ? "保存草稿的新修订" : "保存编辑后的提案")}
              </ActionButton>
            </div>
            {detail.proposals.map((p: Row) => (
              <article key={p.id}>
                <h3>
                  {displayText(p.content.title)} · {displayText(p.status)}
                </h3>
                <p className="muted">
                  目标版本 {displayText(p.expected_revision)} · 输入修订{" "}
                  {displayText(p.dependencies.join(", ") || "无")} · 来源 Turn{" "}
                  {displayText(p.turn_id)}
                </p>
                {p.supersedes && (
                  <details>
                    <summary>查看修订前内容</summary>
                    <pre>
                      {displayText(
                        JSON.stringify(
                          detail.proposals.find(
                            (v: Row) => v.id === p.supersedes,
                          )?.content || { id: p.supersedes },
                          null,
                          2,
                        ),
                      )}
                    </pre>
                  </details>
                )}
                <pre>{displayText(JSON.stringify(p.content, null, 2))}</pre>
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

import { displayText } from "../display-text";
import { ManagerInteraction } from "./ManagerInteraction";
import { RuntimeApproval } from "./RuntimeApproval";
import { DiscussionLink, ReadingLink } from "./reading";
import { StandardDiff } from "./StandardManager";
import { useCallback, useEffect, useRef, useState } from "react";
import { ActionButton } from "./ActionFeedback";
import { MarkdownPreview, RevisionSelect } from "./configuration-ui";
import { loadCatalog } from "./configuration-state";
import { type Request, type Row } from "./rpc-types";

type Props = {
  request: Request;
  instance: Row;
  projectId: string;
  refresh: () => Promise<void>;
  workspaceOnly?: boolean;
  shared?: WorkflowCollaboration;
  onRead?: (next: WorkflowCollaboration) => void;
  nested?: boolean;
  embedded?: boolean;
};

export type WorkflowCollaboration = { data: Row; reload: () => Promise<void> };

export function WorkflowController({
  request,
  instance,
  projectId,
  refresh,
  workspaceOnly = false,
  shared,
  onRead,
  nested = false,
  embedded = false,
}: Props) {
  const [localData, setData] = useState<Row>();
  const candidate = shared?.data || localData;
  const data =
    candidate?._projectId === projectId &&
    candidate?._rootId === (instance.data.root_instance_id || instance.id)
      ? candidate
      : undefined;
  const [catalog, setCatalog] = useState<Row[]>([]);
  const [catalogError, setCatalogError] = useState("");
  const [managerAgent, setManagerAgent] = useState("");
  const [managerPermission, setManagerPermission] = useState("");
  const [input, setInput] = useState("");
  const drafts = useRef(new Map<string, { record?: Row; owner: Row }>());
  const attempts = useRef(
    new Map<string, { fingerprint: string; id: string }>(),
  );
  const [busy, setBusy] = useState(false);
  const [error, setError] = useState("");
  const [readError, setReadError] = useState("");
  const epoch = useRef(0);
  const invalidate = useCallback(() => {
    epoch.current += 1;
  }, []);
  const rootId = instance.data.root_instance_id || instance.id;
  const load: () => Promise<void> = useCallback(async () => {
    const ticket = ++epoch.current;
    const [next, driver] = await Promise.all([
      request("query.orchestration", {
        ...(workspaceOnly
          ? { owner_id: projectId, workspace_only: true }
          : { root_instance_id: rootId }),
        project_id: projectId,
      }),
      workspaceOnly
        ? Promise.resolve({ driver: null })
        : request("query.workflow_driver", {
            id: rootId,
            project_id: projectId,
          }),
    ]);
    if (ticket === epoch.current) {
      const value = {
        ...next,
        _projectId: projectId,
        _rootId: rootId,
        driver: driver.driver,
        driverWaiting: driver.waiting || [],
        approvals: driver.approvals || [],
        runtimeApprovals: driver.runtime_approvals || [],
      };
      setData(value);
      onRead?.({ data: value, reload: load });
      setReadError("");
    }
  }, [request, rootId, projectId, workspaceOnly, onRead]);
  useEffect(() => {
    if (nested) return;
    let active = true;
    let timer: ReturnType<typeof setTimeout>;
    setData(undefined);
    setError("");
    setReadError("");
    async function poll() {
      try {
        await load();
      } catch (e) {
        if (active) setReadError(String(e));
      } finally {
        if (active) timer = setTimeout(() => void poll(), 2500);
      }
    }
    void poll();
    return () => {
      active = false;
      clearTimeout(timer);
      invalidate();
    };
  }, [invalidate, load, nested]);
  useEffect(() => {
    if (!workspaceOnly) return;
    let active = true;
    setCatalog([]);
    setCatalogError("");
    void loadCatalog(request, projectId)
      .then((records) => {
        if (active) setCatalog(records);
      })
      .catch((e) => {
        if (active) setCatalogError(String(e));
      });
    return () => {
      active = false;
    };
  }, [request, projectId, workspaceOnly]);
  const scopedDraft = (kind: string) => `${projectId}:${instance.id}:${kind}`;
  function captureDraft(kind: string) {
    const key = scopedDraft(kind);
    if (!drafts.current.has(key))
      drafts.current.set(key, {
        record: data?.record ? structuredClone(data.record) : undefined,
        owner: structuredClone(instance),
      });
  }
  async function submitDraft(method: string, params: Row) {
    const key = `${scopedDraft(method)}:${params.target}`;
    const fingerprint = JSON.stringify(params);
    if (attempts.current.get(key)?.fingerprint !== fingerprint)
      attempts.current.set(key, { fingerprint, id: crypto.randomUUID() });
    return request(method, {
      ...params,
      command_id: attempts.current.get(key)!.id,
    });
  }
  async function act(method: string) {
    setBusy(true);
    setError("");
    try {
      const original = drafts.current.get(scopedDraft(method)) || {
        record: data?.record,
        owner: instance,
      };
      let record = original.record;
      if (!record) {
        const opened = await request(
          workspaceOnly ? "manager.open" : "orchestration.open",
          {
            project_id: projectId,
            command_id: crypto.randomUUID(),
            target: original.owner.id,
            expected_revision: original.owner.revision,
            payload: {
              ...(managerAgent ? { agent_ref: managerAgent } : {}),
              ...(managerPermission
                ? { harness_permission_ref: managerPermission }
                : {}),
            },
          },
        );
        const loaded = await request("query.orchestration", {
          id: opened.id,
          project_id: projectId,
        });
        record = loaded.record;
      }
      await submitDraft(method, {
        project_id: projectId,
        target: record.id,
        expected_revision: record.revision,
        payload: {
          epoch: record.data.epoch,
          ...(method === "orchestration.input" ? { input } : {}),
          ...(method === "orchestration.resume"
            ? {
                ...(managerAgent ? { agent_ref: managerAgent } : {}),
                ...(managerPermission
                  ? { harness_permission_ref: managerPermission }
                  : {}),
              }
            : {}),
        },
      });
      if (method === "orchestration.input") setInput("");
      drafts.current.delete(scopedDraft(method));
      attempts.current.delete(scopedDraft(method));
      await Promise.all([(shared?.reload || load)(), refresh()]);
    } catch (e) {
      setError(String(e));
    } finally {
      setBusy(false);
    }
  }
  async function adoptManager() {
    setBusy(true);
    setError("");
    try {
      const original = drafts.current.get(scopedDraft("manager.open")) || {
        record: data?.record,
        owner: instance,
      };
      await submitDraft("manager.open", {
        project_id: projectId,
        target: projectId,
        expected_revision: original.owner.revision,
        payload: {
          controller_id: original.record?.id,
          expected_controller_revision: original.record?.revision || 0,
          epoch: original.record?.data.epoch || 0,
          ...(managerAgent ? { agent_ref: managerAgent } : {}),
          ...(managerPermission
            ? { harness_permission_ref: managerPermission }
            : {}),
        },
      });
      drafts.current.delete(scopedDraft("manager.open"));
      attempts.current.delete(scopedDraft("manager.open"));
      await load();
    } catch (e) {
      setError(String(e));
    } finally {
      setBusy(false);
    }
  }
  async function resolveDecision(
    decision: Row,
    approved: boolean,
    opinion: string,
  ) {
    setBusy(true);
    setError("");
    try {
      await submitDraft("workflow.decision.resolve", {
        project_id: projectId,
        target: decision.id,
        expected_revision: decision.revision,
        payload: {
          approved,
          reason:
            opinion.trim() ||
            (approved ? "用户批准当前确切请求" : "用户拒绝当前请求"),
        },
      });
      await Promise.all([(shared?.reload || load)(), refresh()]);
    } finally {
      setBusy(false);
    }
  }
  async function confirmStep(
    approval: Row,
    approved: boolean,
    opinion: string,
  ) {
    setBusy(true);
    try {
      await submitDraft(
        approved ? approval.operation : "workflow.step.rework",
        {
          project_id: projectId,
          target: approval.instance_id,
          expected_revision: approval.revision,
          payload: {
            ...approval.arguments,
            reason: opinion.trim() || "用户确认当前版本结果",
          },
        },
      );
      await Promise.all([(shared?.reload || load)(), refresh()]);
    } finally {
      setBusy(false);
    }
  }
  const controller = data?.record?.data;
  const inScope = (id: string | undefined) =>
    id ? id === instance.id : instance.id === rootId;
  const decisions: Row[] = (data?.decisions || []).filter((decision: Row) =>
    inScope(
      decision.definition_id || decision.data.request.arguments.instance_id,
    ),
  );
  const approvals: Row[] = (data?.approvals || []).filter((approval: Row) =>
    inScope(approval.instance_id),
  );
  const runtimeApprovals: Row[] = (data?.runtimeApprovals || []).filter(
    (item: Row) => inScope(item.instance_id),
  );
  const notices: Row[] = (data?.interactions || []).filter((notice: Row) =>
    inScope(notice.data.instance_id),
  );
  const inputRequests = ((data?.driverWaiting || []) as Row[]).filter(
    (item) => item.status === "waiting_input" && inScope(item.instance_id),
  );
  const currentInputNotice = notices.find((notice) =>
    (notice.data.interaction?.evidence || []).some((reference: Row) =>
      inputRequests.some(
        (item) => item.execution_id === reference.execution_id,
      ),
    ),
  )?.id;
  const interactive =
    workspaceOnly || !["completed", "cancelled"].includes(instance.data.status);
  const managerRole = catalog.find(
    (row) => row.type === "role" && row.id === controller?.role_ref,
  );
  const managerAgents = catalog.filter(
    (row) =>
      row.type === "agent" &&
      (row.role_ref === controller?.role_ref ||
        (managerRole &&
          catalog.some(
            (role) =>
              role.type === "role" &&
              role.id === row.role_ref &&
              role.definition_id === managerRole.definition_id,
          ))),
  );
  const statuses: Record<string, string> = {
    active: "协作中",
    paused: "已接管",
    failed: "暂不可用",
    needs_reconciliation: "需要核对",
    budget_exhausted: "等待恢复",
    closed: "已结束",
  };
  const waitingLabels: Record<string, string> = {
    waiting_input: "等待你的回答",
    waiting_confirmation: "等待确认当前产物",
    awaiting_acceptance: "等待接纳结果",
    failed: "执行失败",
    needs_reconciliation: "执行结果需要核对",
    queued: "等待执行条件",
  };
  const interactionCards = (
    <>
      {decisions.map((decision: Row) => (
        <ManagerInteraction
          key={decision.id}
          basis={`${decision.id}:${decision.revision}`}
          interaction={{
            ...(data?.approvals?.find(
              (approval: Row) =>
                approval.arguments.execution_id ===
                decision.data.request.arguments.execution_id,
            )?.interaction || {}),
            ...(decision.data.request.interaction || {}),
          }}
          projectId={projectId}
          ownerId={controller?.owner_id || projectId}
          status={
            decision.current === false && decision.data.status === "pending"
              ? "stale"
              : decision.data.status
          }
          busy={busy}
          onDecide={(approved, opinion) =>
            resolveDecision(decision, approved, opinion)
          }
        />
      ))}
      {runtimeApprovals.map((interaction: Row) => (
        <RuntimeApproval
          key={interaction.id}
          request={request}
          interaction={interaction}
          projectId={projectId}
          ownerId={instance.data.owner_id || projectId}
          refresh={async () => {
            await Promise.all([(shared?.reload || load)(), refresh()]);
          }}
        />
      ))}
      {approvals
        .filter(
          (approval: Row) =>
            !(data?.decisions || []).some(
              (decision: Row) =>
                decision.current !== false &&
                decision.data.status === "pending" &&
                decision.data.request.arguments.execution_id ===
                  approval.arguments.execution_id,
            ),
        )
        .map((approval: Row) => (
          <ManagerInteraction
            key={approval.arguments.execution_id}
            basis={JSON.stringify([
              approval.instance_id,
              approval.operation,
              approval.arguments,
            ])}
            interaction={approval.interaction}
            projectId={projectId}
            ownerId={approval.owner_id}
            status="pending"
            busy={busy}
            requireReason={approval.operation === "workflow.step.approve"}
            onDecide={(approved, opinion) =>
              confirmStep(approval, approved, opinion)
            }
          />
        ))}
      {notices.map((notice: Row) => (
        <ManagerInteraction
          key={notice.id}
          interaction={notice.data.interaction}
          projectId={projectId}
          ownerId={controller?.owner_id || projectId}
          busy={busy}
          awaitingInput={notice.id === currentInputNotice}
        />
      ))}
    </>
  );
  return (
    <section
      aria-label={displayText(
        workspaceOnly ? "项目经理工作台" : "项目经理协作",
      )}
      className={embedded ? "manager-collaboration" : "panel"}
    >
      <h3>{displayText(workspaceOnly ? "项目经理工作台" : "项目经理协作")}</h3>
      {!workspaceOnly && (
        <p className="muted">
          在此处理当前流程的待确认事项。主工作流与子流程由同一位项目经理协调，接管或恢复项目经理会影响整个
          Feature。
        </p>
      )}
      {!data && <p role="status">正在读取经理状态…</p>}
      {!workspaceOnly && data?.driver?.data.reason && (
        <p role="status">
          {displayText(
            typeof data.driver.data.reason === "string"
              ? data.driver.data.reason
              : data.driver.data.reason.message,
          )}
        </p>
      )}
      {(data?.driverWaiting || []).some((item: Row) =>
        inScope(item.instance_id),
      ) && (
        <section
          className="manager-blockers"
          aria-label="当前阻塞点"
          aria-live="polite"
        >
          <h4>当前阻塞点</h4>
          <p className="muted">
            需要确认的产物或计划在下方处理；需要补充信息时，可通过“与项目经理继续讨论”回复。
          </p>
          <ul aria-label="调度等待条件">
            {((data?.driverWaiting || []) as Row[])
              .filter((item) => inScope(item.instance_id))
              .map((item) => {
                const gate = (Object.values(item.actions || {}) as Row[]).find(
                  (action) => item.waiting_for?.includes(action.code),
                );
                return (
                  <li key={item.execution_id}>
                    <details
                      className="manager-disclosure"
                      open={
                        item.status === "waiting_input" && !currentInputNotice
                      }
                    >
                      <summary>
                        <strong>
                          {displayText(
                            item.step_name || item.path || item.step,
                          )}{" "}
                          ·{" "}
                          {displayText(
                            waitingLabels[item.status] || item.status,
                          )}
                        </strong>
                      </summary>
                      <div className="manager-disclosure-body">
                        <p>
                          {displayText(
                            gate?.reason ||
                              item.reason ||
                              "正在等待执行条件就绪。",
                          )}
                        </p>
                        <ReadingLink
                          detail="records"
                          projectId={projectId}
                          ownerId={
                            item.owner_id || instance.data.owner_id || projectId
                          }
                          target={{
                            instance_id: item.instance_id,
                            execution_id: item.execution_id,
                          }}
                        >
                          查看执行详情
                        </ReadingLink>
                      </div>
                    </details>
                  </li>
                );
              })}
          </ul>
        </section>
      )}
      {workspaceOnly && (
        <details open>
          <summary>工作台配置与沟通</summary>
          <div className="manager-configuration-row">
            <RevisionSelect
              label={
                workspaceOnly ? "工作台 Agent 精确修订" : "经理 Agent 精确修订"
              }
              records={managerAgents}
              value={managerAgent}
              onChange={(value) => {
                captureDraft("orchestration.resume");
                captureDraft("manager.open");
                setManagerAgent(value);
              }}
            />
            <RevisionSelect
              label={workspaceOnly ? "工作台授权上下文" : "经理授权上下文"}
              records={catalog.filter(
                (row) => row.type === "permission_profile",
              )}
              value={managerPermission}
              onChange={(value) => {
                captureDraft("orchestration.resume");
                captureDraft("manager.open");
                setManagerPermission(value);
              }}
            />
            <ActionButton
              disabled={
                busy ||
                !data ||
                (!managerAgent && !managerPermission) ||
                (!workspaceOnly && data?.record?.data.status === "active")
              }
              onClick={() =>
                void (workspaceOnly
                  ? adoptManager()
                  : act("orchestration.resume"))
              }
            >
              {displayText(workspaceOnly ? "采用工作台配置" : "采用并恢复经理")}
            </ActionButton>
          </div>
          <p className="muted">
            默认使用项目配置中的编排
            Agent。更换组合前先暂停当前经理会话；后继会话使用所选配置。
          </p>
        </details>
      )}
      <p>
        {displayText(
          controller
            ? statuses[controller.status] || controller.status
            : data
              ? workspaceOnly
                ? "发送消息后开始讨论"
                : "项目经理会话尚未建立"
              : "",
        )}
      </p>
      {controller?.reason && (
        <p role="status">{displayText(controller.reason)}</p>
      )}
      {controller?.session_id && (
        <p>
          <DiscussionLink
            projectId={projectId}
            ownerId={controller.owner_id}
            sessionId={controller.session_id}
          >
            与项目经理继续讨论
          </DiscussionLink>
        </p>
      )}
      {workspaceOnly && interactive && (
        <form
          onSubmit={(e) => {
            e.preventDefault();
            void act("orchestration.input");
          }}
        >
          <label>
            {displayText(workspaceOnly ? "给项目经理的指令" : "回复项目经理")}
            <textarea
              aria-label={displayText(
                workspaceOnly ? "给项目经理的指令" : "回复项目经理",
              )}
              value={input}
              onChange={(e) => {
                captureDraft("orchestration.input");
                setInput(e.target.value);
              }}
              placeholder="回答经理的问题，或提供你的业务意见"
            />
          </label>
          <div className="actions">
            <ActionButton
              type="submit"
              disabled={
                busy ||
                !data ||
                !input.trim() ||
                ["completed", "cancelled"].includes(instance.data.status)
              }
            >
              {displayText(workspaceOnly ? "发送" : "发送回复")}
            </ActionButton>
          </div>
        </form>
      )}
      {interactive && controller && (
        <ActionButton
          type="button"
          disabled={busy}
          onClick={() =>
            void act(
              controller.status === "active"
                ? "orchestration.takeover"
                : "orchestration.resume",
            )
          }
        >
          {displayText(
            controller.status === "active" ? "人工接管" : "恢复项目经理",
          )}
        </ActionButton>
      )}
      {readError && (
        <p role="status" className="notice" title={displayText(readError)}>
          暂时无法刷新经理状态，正在重试；当前显示为上次读取的状态。
        </p>
      )}
      {error && (
        <p className="error" role="alert">
          {displayText(error)}
        </p>
      )}
      {catalogError && (
        <p className="error" role="alert">
          配置选项暂不可用：{displayText(catalogError)}
        </p>
      )}
      {interactionCards}
      {!!data?.messages?.length && (
        <details className="manager-disclosure manager-log">
          <summary>
            <strong>项目经理日志</strong>
            <span>{displayText(data.messages.length)} 条</span>
          </summary>
          <div className="manager-disclosure-body">
            {data.messages
              .slice()
              .reverse()
              .map((m: Row) => (
                <div key={m.id} className="notice">
                  <MarkdownPreview text={m.content} />
                </div>
              ))}
          </div>
        </details>
      )}
    </section>
  );
}

export function WorkflowPlanChanges(props: Props) {
  const [activated, setActivated] = useState(false);
  const [exact, setExact] = useState<Row>();
  const [loading, setLoading] = useState(false);
  const [error, setError] = useState("");
  const epoch = useRef(0);
  useEffect(
    () => () => {
      epoch.current += 1;
    },
    [],
  );
  const loadExact = async () => {
    const ticket = ++epoch.current;
    setLoading(true);
    setError("");
    try {
      const detail = await props.request("query.workflow_instance", {
        project_id: props.projectId,
        id: props.instance.id,
      });
      if (ticket === epoch.current) setExact(detail.record);
    } catch (error) {
      if (ticket === epoch.current) setError(String(error));
    } finally {
      if (ticket === epoch.current) setLoading(false);
    }
  };
  return (
    <details
      className={props.embedded ? "manager-plan-changes" : "panel"}
      onToggle={(event) => {
        if (event.currentTarget.open && !activated) {
          setActivated(true);
          if (props.instance.data.plan?.document) setExact(props.instance);
          else void loadExact();
        }
      }}
    >
      <summary>调整当前运行计划</summary>
      {activated && loading && <p role="status">正在读取确切执行计划…</p>}
      {error && <p role="alert">{error}</p>}
      {activated && !exact && !loading && (
        <ActionButton onClick={() => void loadExact()}>
          重新读取执行计划
        </ActionButton>
      )}
      {exact && (
        <WorkflowPlanChangeEditor
          {...props}
          key={exact.revision}
          instance={exact}
          refresh={async () => {
            await props.refresh();
            await loadExact();
          }}
        />
      )}
    </details>
  );
}

function WorkflowPlanChangeEditor({
  request,
  instance,
  projectId,
  refresh,
}: Props) {
  const runtime = instance.data;
  const plan = runtime.plan.document;
  const [kind, setKind] = useState("edit");
  const [selected, setSelected] = useState("");
  const [task, setTask] = useState("");
  const [dependencies, setDependencies] = useState<string[]>([]);
  const [reason, setReason] = useState("");
  const [material, setMaterial] = useState("");
  const [sourcePort, setSourcePort] = useState("");
  const [newName, setNewName] = useState("");
  const [requirement, setRequirement] = useState("completion");
  const [preview, setPreview] = useState<Row>();
  const [busy, setBusy] = useState(false);
  const [error, setError] = useState("");
  const commands = useRef(
    new Map<string, { fingerprint: string; id: string }>(),
  );
  function submit(method: string, payload: Row) {
    const params = {
      project_id: projectId,
      target: instance.id,
      expected_revision: instance.revision,
      payload,
    };
    const fingerprint = JSON.stringify(params);
    if (commands.current.get(method)?.fingerprint !== fingerprint)
      commands.current.set(method, { fingerprint, id: crypto.randomUUID() });
    return request(method, {
      ...params,
      command_id: commands.current.get(method)!.id,
    });
  }
  useEffect(() => {
    setPreview(undefined);
  }, [runtime.active_plan_revision_id]);
  function choose(value: string) {
    setSelected(value);
    setPreview(undefined);
    const step = plan.steps.find((s: Row) => s.key === value);
    setTask(step?.task_instructions || "");
    setDependencies(step?.dependencies || []);
  }
  async function inspectChange() {
    setBusy(true);
    setError("");
    setPreview(undefined);
    try {
      const step = plan.steps.find((s: Row) => s.key === selected);
      let operation: Row;
      if (kind === "bypass") {
        const substitutions: Row = {};
        if (material.trim()) {
          const [key, port] = sourcePort.split(".");
          const definition = plan.steps.find((s: Row) => s.key === key)
            ?.outputs?.[port];
          if (!definition) throw new Error("请选择替代材料对应的交付项");
          const imported = await submit("workflow.materials.import", {
            plan_revision_id: runtime.active_plan_revision_id,
            delivery_ref: definition.delivery_ref,
            value: material,
            source: "用户提供的替代材料",
            reason,
          });
          substitutions[sourcePort] = {
            type: "material",
            material_ref: imported.id,
          };
        }
        operation = { type: "bypass_stage", stage: selected, substitutions };
      } else if (kind === "remove")
        operation = { type: "remove_step", node_id: step.node_id };
      else if (kind === "waive")
        operation = {
          type: "waive",
          node_id: step.node_id,
          requirement,
        };
      else
        operation = {
          type: "upsert_step",
          step: {
            ...step,
            ...(kind === "add"
              ? {
                  key: `step_${crypto.randomUUID().slice(0, 8)}`,
                  node_id: "",
                  name: newName.trim(),
                }
              : {}),
            task_instructions:
              step.executor.type === "agent" ? task : undefined,
            dependencies,
          },
        };
      const changeset = {
        plan_revision_id: runtime.active_plan_revision_id,
        reason,
        operations: [operation],
      };
      const next = await request("query.workflow_change_preview", {
        project_id: projectId,
        id: instance.id,
        changeset,
      });
      setPreview(next);
    } catch (e) {
      setError(String(e));
    } finally {
      setBusy(false);
    }
  }
  async function apply() {
    if (!preview) return;
    setBusy(true);
    setError("");
    try {
      await submit("workflow.change.apply", {
        plan_revision_id: runtime.active_plan_revision_id,
        changeset: preview.changeset,
        preview_hash: preview.preview_hash,
      });
      setPreview(undefined);
      await refresh();
    } catch (e) {
      setError(String(e));
    } finally {
      setBusy(false);
    }
  }
  const choices = kind === "bypass" ? plan.stages : plan.steps;
  return (
    <>
      <label>
        调整方式
        <select
          aria-label="计划调整方式"
          value={kind}
          onChange={(e) => {
            setKind(e.target.value);
            choose("");
          }}
        >
          <option value="edit">修改任务及依赖</option>
          <option value="add">基于现有步骤新增</option>
          <option value="bypass">绕过阶段</option>
          <option value="remove">移除步骤</option>
          <option value="waive">例外接纳完成条件</option>
        </select>
      </label>
      <label>
        {kind === "bypass" ? "阶段" : "步骤"}
        <select
          aria-label="计划调整对象"
          value={selected}
          onChange={(e) => choose(e.target.value)}
        >
          <option value="">请选择</option>
          {choices.map((s: Row) => (
            <option key={s.node_id || s.key} value={s.key}>
              {s.name}
            </option>
          ))}
        </select>
      </label>
      {kind === "add" && (
        <label>
          新增步骤名称
          <input
            aria-label="新增步骤名称"
            value={newName}
            onChange={(e) => {
              setNewName(e.target.value);
              setPreview(undefined);
            }}
          />
        </label>
      )}
      {kind === "waive" && (
        <label>
          豁免要求
          <select
            aria-label="豁免要求"
            value={requirement}
            onChange={(e) => {
              setRequirement(e.target.value);
              setPreview(undefined);
            }}
          >
            <option value="completion">当前完成条件</option>
            <option value="tests">候选测试要求</option>
            <option value="confirmation">人工确认要求</option>
          </select>
        </label>
      )}
      {["edit", "add"].includes(kind) && selected && (
        <>
          <label>
            任务说明
            <textarea
              value={task}
              onChange={(e) => {
                setTask(e.target.value);
                setPreview(undefined);
              }}
            />
          </label>
          <fieldset>
            <legend>前置依赖</legend>
            {plan.steps
              .filter((s: Row) => s.key !== selected)
              .map((s: Row) => (
                <label key={s.key}>
                  <input
                    type="checkbox"
                    checked={dependencies.includes(s.key)}
                    onChange={(e) => {
                      setDependencies((old) =>
                        e.target.checked
                          ? [...old, s.key]
                          : old.filter((key) => key !== s.key),
                      );
                      setPreview(undefined);
                    }}
                  />
                  {s.name}
                </label>
              ))}
          </fieldset>
        </>
      )}
      {kind === "bypass" && (
        <>
          <label>
            替代交付项
            <select
              value={sourcePort}
              onChange={(e) => {
                setSourcePort(e.target.value);
                setPreview(undefined);
              }}
            >
              <option value="">无需替代材料</option>
              {plan.steps
                .filter((s: Row) => s.stage === selected)
                .flatMap((s: Row) =>
                  Object.keys(s.outputs || {}).map((port) => (
                    <option key={`${s.key}.${port}`} value={`${s.key}.${port}`}>
                      {s.name} · {port}
                    </option>
                  )),
                )}
            </select>
          </label>
          <label>
            替代材料正文
            <textarea
              value={material}
              onChange={(e) => {
                setMaterial(e.target.value);
                setPreview(undefined);
              }}
            />
          </label>
        </>
      )}
      <label>
        调整理由
        <textarea
          aria-label="计划调整理由"
          value={reason}
          onChange={(e) => {
            setReason(e.target.value);
            setPreview(undefined);
          }}
        />
      </label>
      <ActionButton
        type="button"
        disabled={
          busy ||
          !selected ||
          !reason.trim() ||
          (kind === "add" && !newName.trim())
        }
        onClick={() => void inspectChange()}
      >
        预览影响
      </ActionButton>
      {preview?.standard_diff ? (
        <StandardDiff diff={preview.standard_diff} />
      ) : null}
      {preview && (
        <div role="status">
          <p>
            受影响步骤：{preview.affected.join("、") || "无"}；需要停止：
            {preview.must_stop.length} 项。
          </p>
          <p>
            绕过：{preview.bypassed.join("、") || "无"}。原执行记录和报告保留。
          </p>
          {!!preview.budget_updates?.length && (
            <p>
              延长子流程预算：{preview.budget_updates.join("、")}
              。保留当前调用和原合同。
            </p>
          )}
          <ActionButton
            type="button"
            disabled={busy}
            onClick={() => void apply()}
          >
            应用此计划调整
          </ActionButton>
        </div>
      )}
      {error && (
        <p className="error" role="alert">
          {error}
        </p>
      )}
    </>
  );
}

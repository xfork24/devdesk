import { ManagerInteraction } from "./ManagerInteraction";
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
};

export function WorkflowController({
  request,
  instance,
  projectId,
  refresh,
  workspaceOnly = false,
}: Props) {
  const [data, setData] = useState<Row>();
  const [catalog, setCatalog] = useState<Row[]>([]);
  const [catalogError, setCatalogError] = useState("");
  const [managerAgent, setManagerAgent] = useState("");
  const [managerPermission, setManagerPermission] = useState("");
  const [input, setInput] = useState("");
  const [busy, setBusy] = useState(false);
  const [error, setError] = useState("");
  const epoch = useRef(0);
  const invalidate = useCallback(() => {
    epoch.current += 1;
  }, []);
  const load = useCallback(async () => {
    const ticket = ++epoch.current;
    const [next, driver] = await Promise.all([
      request("query.orchestration", {
        ...(workspaceOnly
          ? { owner_id: projectId, workspace_only: true }
          : { root_instance_id: instance.id }),
        project_id: projectId,
      }),
      workspaceOnly
        ? Promise.resolve({ driver: null })
        : request("query.workflow_driver", {
            id: instance.id,
            project_id: projectId,
          }),
    ]);
    if (ticket === epoch.current)
      setData({
        ...next,
        driver: driver.driver,
        driverWaiting: driver.waiting || [],
      });
  }, [request, instance.id, projectId, workspaceOnly]);
  useEffect(() => {
    let active = true;
    let timer: ReturnType<typeof setTimeout>;
    setData(undefined);
    setError("");
    async function poll() {
      try {
        await load();
      } catch (e) {
        if (active) setError(String(e));
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
  }, [invalidate, load]);
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
  async function act(method: string) {
    setBusy(true);
    setError("");
    try {
      let record = data?.record;
      if (!record) {
        const opened = await request(
          workspaceOnly ? "manager.open" : "orchestration.open",
          {
            project_id: projectId,
            command_id: crypto.randomUUID(),
            target: instance.id,
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
      await request(method, {
        project_id: projectId,
        command_id: crypto.randomUUID(),
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
      await Promise.all([load(), refresh()]);
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
      await request("manager.open", {
        project_id: projectId,
        command_id: crypto.randomUUID(),
        target: projectId,
        payload: {
          ...(managerAgent ? { agent_ref: managerAgent } : {}),
          ...(managerPermission
            ? { harness_permission_ref: managerPermission }
            : {}),
        },
      });
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
      await request("workflow.decision.resolve", {
        project_id: projectId,
        command_id: crypto.randomUUID(),
        target: decision.id,
        expected_revision: decision.revision,
        payload: {
          approved,
          reason:
            opinion.trim() ||
            (approved ? "用户批准当前确切请求" : "用户拒绝当前请求"),
        },
      });
      await Promise.all([load(), refresh()]);
    } catch (e) {
      setError(String(e));
    } finally {
      setBusy(false);
    }
  }
  const controller = data?.record?.data;
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
  return (
    <section
      aria-label={workspaceOnly ? "项目经理工作台" : "项目经理协作"}
      className="panel"
    >
      <h3>{workspaceOnly ? "项目经理工作台" : "项目经理协作"}</h3>
      {!data && <p role="status">正在读取经理状态…</p>}
      {!workspaceOnly && data?.driver?.data.reason && (
        <p role="status">
          {typeof data.driver.data.reason === "string"
            ? data.driver.data.reason
            : data.driver.data.reason.message}
        </p>
      )}
      {data?.driverWaiting?.length > 0 && (
        <section
          className="manager-blockers"
          aria-label="当前阻塞点"
          aria-live="polite"
        >
          <h4>当前阻塞点</h4>
          <p className="muted">
            可直接在下方回复项目经理，由经理转交你的回答并协调后续执行。需要确认的产物或计划会在这里请求你的决定。
          </p>
          <ul aria-label="调度等待条件">
            {((data?.driverWaiting || []) as Row[]).map((item) => {
              const gate = (Object.values(item.actions || {}) as Row[]).find(
                (action) => item.waiting_for?.includes(action.code),
              );
              return (
                <li key={item.execution_id}>
                  <strong>
                    {item.step_name || item.path || item.step} ·{" "}
                    {waitingLabels[item.status] || item.status}
                  </strong>
                  <p>
                    {gate?.reason || item.reason || "正在等待执行条件就绪。"}
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
              onChange={(value) => setManagerAgent(value)}
            />
            <RevisionSelect
              label={workspaceOnly ? "工作台授权上下文" : "经理授权上下文"}
              records={catalog.filter(
                (row) => row.type === "permission_profile",
              )}
              value={managerPermission}
              onChange={(value) => setManagerPermission(value)}
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
              {workspaceOnly ? "采用工作台配置" : "采用并恢复经理"}
            </ActionButton>
          </div>
          <p className="muted">
            默认使用项目配置中的编排
            Agent。更换组合前先暂停当前经理会话；后继会话使用所选配置。
          </p>
        </details>
      )}
      <p>
        {controller
          ? statuses[controller.status] || controller.status
          : data
            ? workspaceOnly
              ? "发送消息后开始讨论"
              : "项目经理会话尚未建立"
            : ""}
      </p>
      {controller?.reason && <p role="status">{controller.reason}</p>}
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
      <form
        onSubmit={(e) => {
          e.preventDefault();
          void act("orchestration.input");
        }}
      >
        <label>
          {workspaceOnly ? "给项目经理的指令" : "回复项目经理"}
          <textarea
            aria-label={workspaceOnly ? "给项目经理的指令" : "回复项目经理"}
            value={input}
            onChange={(e) => setInput(e.target.value)}
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
            {workspaceOnly ? "发送" : "发送回复"}
          </ActionButton>
          {controller && (
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
              {controller.status === "active" ? "人工接管" : "恢复项目经理"}
            </ActionButton>
          )}
        </div>
      </form>
      {error && (
        <p className="error" role="alert">
          {error}
        </p>
      )}
      {catalogError && (
        <p className="error" role="alert">
          配置选项暂不可用：{catalogError}
        </p>
      )}
      {(data?.decisions || []).map((decision: Row) => (
        <ManagerInteraction
          key={decision.id}
          interaction={decision.data.request.interaction || {}}
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
      {(data?.interactions || []).map((notice: Row) => (
        <ManagerInteraction
          key={notice.id}
          interaction={notice.data.interaction}
          projectId={projectId}
          ownerId={controller?.owner_id || projectId}
          busy={busy}
        />
      ))}
      {(data?.messages || [])
        .slice()
        .reverse()
        .map((m: Row) => (
          <div key={m.id} className="notice">
            <MarkdownPreview text={m.content} />
          </div>
        ))}
    </section>
  );
}

export function WorkflowPlanChanges({
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
          const imported = await request("workflow.materials.import", {
            project_id: projectId,
            command_id: crypto.randomUUID(),
            target: instance.id,
            expected_revision: instance.revision,
            payload: {
              plan_revision_id: runtime.active_plan_revision_id,
              delivery_ref: definition.delivery_ref,
              value: material,
              source: "用户提供的替代材料",
              reason,
            },
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
      const fresh = await request("query.workflow_instance", {
        project_id: projectId,
        id: instance.id,
      });
      await request("workflow.change.apply", {
        project_id: projectId,
        command_id: crypto.randomUUID(),
        target: instance.id,
        expected_revision: fresh.record.revision,
        payload: {
          plan_revision_id: runtime.active_plan_revision_id,
          changeset: preview.changeset,
          preview_hash: preview.preview_hash,
        },
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
    <details className="panel">
      <summary>调整当前运行计划</summary>
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
    </details>
  );
}

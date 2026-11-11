import { StandardDiff } from "./StandardManager";
import { useCallback, useEffect, useRef, useState } from "react";
import { ActionButton } from "./ActionFeedback";
import { MarkdownPreview, type Request, type Row } from "./configuration-ui";

type Props = {
  request: Request;
  instance: Row;
  projectId: string;
  refresh: () => Promise<void>;
};

export function WorkflowController({
  request,
  instance,
  projectId,
  refresh,
}: Props) {
  const [data, setData] = useState<Row>();
  const [input, setInput] = useState("");
  const [busy, setBusy] = useState(false);
  const [error, setError] = useState("");
  const epoch = useRef(0);
  const load = useCallback(async () => {
    const ticket = ++epoch.current;
    const next = await request("query.orchestration", {
      root_instance_id: instance.id,
      project_id: projectId,
    });
    if (ticket === epoch.current) setData(next);
  }, [request, instance.id, projectId]);
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
      epoch.current++;
    };
  }, [load]);
  async function act(method: string) {
    setBusy(true);
    setError("");
    try {
      let record = data?.record;
      if (!record) {
        const opened = await request("orchestration.open", {
          project_id: projectId,
          command_id: crypto.randomUUID(),
          target: instance.id,
          payload: {},
        });
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
  async function resolveDecision(decision: Row, approved: boolean) {
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
          reason: approved ? "用户批准当前确切请求" : "用户拒绝当前请求",
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
  const actionStates: Record<string, string> = {
    prepared: "已登记",
    waiting_quiescence: "正在停止受影响的执行",
    applied: "新计划已生效",
    active: "已委托",
    pending: "等待人工决定",
    stopped: "已停止",
    completed: "已完成",
  };
  const statuses: Record<string, string> = {
    active: "协作中",
    paused: "已接管",
    failed: "暂不可用",
    needs_reconciliation: "需要核对",
    budget_exhausted: "等待恢复",
    closed: "已结束",
  };
  return (
    <section aria-label="项目经理协作" className="panel">
      <h3>项目经理协作</h3>
      <p>
        {controller
          ? statuses[controller.status] || controller.status
          : "发送指令后启动项目经理"}
      </p>
      {controller?.reason && <p role="status">{controller.reason}</p>}
      <form
        onSubmit={(e) => {
          e.preventDefault();
          void act("orchestration.input");
        }}
      >
        <label>
          给项目经理的指令
          <textarea
            aria-label="给项目经理的指令"
            value={input}
            onChange={(e) => setInput(e.target.value)}
            placeholder="例如：采用我提供的设计，调整后续任务并继续执行"
          />
        </label>
        <div className="actions">
          <ActionButton
            type="submit"
            disabled={
              busy ||
              !input.trim() ||
              ["completed", "cancelled"].includes(instance.data.status)
            }
          >
            发送并推进
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
      {(data?.decisions || [])
        .filter((r: Row) => r.data.status === "pending")
        .map((r: Row) => (
          <article key={r.id} aria-label="待决定的操作">
            <p>{r.name}</p>
            <p>
              请求操作：{r.data.request.operation} · 计划{" "}
              {r.data.plan_revision_id}
            </p>
            <details>
              <summary>查看确切请求</summary>
              <pre>{JSON.stringify(r.data.request.arguments, null, 2)}</pre>
            </details>
            <ActionButton
              disabled={busy}
              onClick={() => void resolveDecision(r, true)}
            >
              批准此请求
            </ActionButton>
            <ActionButton
              disabled={busy}
              onClick={() => void resolveDecision(r, false)}
            >
              拒绝此请求
            </ActionButton>
          </article>
        ))}
      {(data?.calls || []).slice(0, 5).map((r: Row) => (
        <p key={r.id}>
          {r.data.method} ·{" "}
          {r.data.status === "succeeded"
            ? actionStates[r.data.receipt?.status] || "已接纳"
            : r.data.status === "failed"
              ? r.data.error?.message
              : "已登记"}
        </p>
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

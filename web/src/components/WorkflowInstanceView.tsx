import { useEffect, useRef, useState } from "react";
type Row = Record<string, any>;
type Props = {
  request: (method: string, params?: Row) => Promise<any>;
  record: Row;
  projectId: string;
  refresh: () => Promise<void>;
  onDiscussion: (id: string, owner: string) => void;
};
const stateLabels: Record<string, string> = {
  pending: "等待前置",
  waiting_dependencies: "等待前置",
  waiting_condition: "等待条件",
  active: "执行中",
  completed: "已完成",
  skipped: "已跳过",
  waiting_input: "等待讨论 / 采纳",
  waiting_confirmation: "等待人确认",
  blocked: "已阻塞",
  failed: "失败",
  cancelled: "已取消",
};
export function WorkflowInstanceView({
  request,
  record,
  projectId,
  refresh,
  onDiscussion,
}: Props) {
  const [data, setData] = useState<Row>(),
    [error, setError] = useState(""),
    [busy, setBusy] = useState(false),
    [selected, setSelected] = useState("");
  const generation = useRef(0);
  useEffect(() => {
    const g = ++generation.current;
    request("query.workflow_instance", { id: record.id, project_id: projectId })
      .then((d) => {
        if (g === generation.current) setData(d);
      })
      .catch((e) => {
        if (g === generation.current) setError(String(e));
      });
    return () => {
      ++generation.current;
    };
  }, [request, record.id, record.revision, projectId]);
  async function command(
    method: string,
    target: string,
    revision: number,
    payload: Row = {},
  ) {
    setBusy(true);
    setError("");
    try {
      await request(method, {
        project_id: projectId,
        command_id: crypto.randomUUID(),
        target,
        expected_revision: revision,
        payload,
      });
      await refresh();
    } catch (e) {
      setError(String(e));
    } finally {
      setBusy(false);
    }
  }
  const runtime = data?.record.data,
    active = runtime?.steps.find(
      (s: Row) => !["completed", "skipped", "pending"].includes(s.status),
    );
  return (
    <section className="panel">
      <div className="panel-heading">
        <div>
          <h2>当前工作流</h2>
          <p>
            {active
              ? `${data?.plan.steps.find((s: Row) => s.key === active.key)?.name} · ${stateLabels[active.status]}`
              : stateLabels[record.data.status] || record.data.status}
          </p>
        </div>
        {!["completed", "cancelled"].includes(record.data.status) && (
          <button
            disabled={busy}
            onClick={() =>
              command(
                record.data.paused ? "workflow.resume" : "workflow.pause",
                record.id,
                record.revision,
              )
            }
          >
            {record.data.paused ? "恢复流程" : "暂停流程"}
          </button>
        )}
      </div>
      {!data && !error && <p role="status">正在读取流程与配置锁…</p>}
      <div className="workflow-runtime">
        {runtime?.steps.map((step: Row) => {
          const definition = data?.plan.steps.find(
            (s: Row) => s.key === step.key,
          );
          return (
            <article
              key={step.id}
              className={`workflow-node status-${step.status}`}
            >
              <button
                aria-expanded={selected === step.id}
                onClick={() => setSelected(selected === step.id ? "" : step.id)}
              >
                <strong>{definition?.name || step.key}</strong>
                <span>
                  {stateLabels[step.status] || step.status} · 第 {step.round} 轮
                </span>
              </button>
              {step.reason && <p>{step.reason}</p>}
              {step.outputs.session && (
                <button
                  onClick={() =>
                    onDiscussion(step.outputs.session, runtime.owner_id)
                  }
                >
                  继续讨论 / 查看提案
                </button>
              )}
              {(
                data?.actions.find((a: Row) => a.id === step.id)?.allowed || []
              ).map((method: string) => (
                <button
                  key={method}
                  disabled={busy || runtime.paused}
                  onClick={() =>
                    command(
                      method,
                      step.id,
                      step.revision,
                      method === "workflow.step.confirm"
                        ? { input_hash: step.input_hash }
                        : method === "workflow.step.rework"
                          ? {
                              from_step:
                                data?.actions.find((a: Row) => a.id === step.id)
                                  ?.rework_entries?.[0] || step.key,
                            }
                          : {},
                    )
                  }
                >
                  {method.endsWith("confirm")
                    ? "确认结果后继续"
                    : method.endsWith("retry")
                      ? "技术重试"
                      : `从${data?.plan.steps.find((definition: Row) => definition.key === data?.actions.find((action: Row) => action.id === step.id)?.rework_entries?.[0])?.name || "本步骤"}返工`}
                </button>
              ))}
              {selected === step.id && (
                <>
                  <p>
                    职责 {definition?.participant_slot || "协调器"} ·{" "}
                    {definition?.advance_mode === "automatic"
                      ? "自动推进"
                      : "结果完成后等待人确认"}
                  </p>
                  <p>{definition?.interaction.goal}</p>
                  <details>
                    <summary>该轮输入、证据和输出引用</summary>
                    <pre>{JSON.stringify(step, null, 2)}</pre>
                  </details>
                </>
              )}
            </article>
          );
        })}
      </div>
      <details>
        <summary>Feature 冻结配置（只读）</summary>
        <p>
          Workflow {data?.configuration.workflow_ref} · Standard{" "}
          {data?.configuration.standard_ref} · 来源 Project 配置{" "}
          {data?.configuration.project_configuration_id}
        </p>
        <pre>{JSON.stringify(data?.configuration, null, 2)}</pre>
      </details>
      {runtime?.task_members.length > 0 && (
        <p>
          TaskGroup generation {runtime.generation} · 固定{" "}
          {runtime.task_members.length} 个成员
        </p>
      )}
      {error && (
        <p className="error" role="alert">
          {error}
        </p>
      )}
    </section>
  );
}

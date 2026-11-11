import { useEffect, useState } from "react";
import { ActionButton } from "./ActionFeedback";
import {
  loadCatalog,
  RevisionSelect,
  type Request,
  type Row,
} from "./configuration-ui";
import { StandardDiff } from "./StandardManager";

/** One consumer UI for every work kind; the Standard itself has no work owner. */
export function WorkStandard({
  request,
  projectId,
  owner,
  instance,
  refresh,
}: {
  request: Request;
  projectId: string;
  owner: Row;
  instance?: Row;
  refresh: () => Promise<void>;
}) {
  const [records, setRecords] = useState<Row[]>([]),
    [configuration, setConfiguration] = useState<Row>(),
    [history, setHistory] = useState<Row[]>([]);
  const [selected, setSelected] = useState(""),
    [workflow, setWorkflow] = useState(""),
    [reason, setReason] = useState("");
  const [preview, setPreview] = useState<Row>(),
    [error, setError] = useState(""),
    [busy, setBusy] = useState(false);
  const [bindings, setBindings] = useState<Row>({}),
    [permission, setPermission] = useState(""),
    [requirements, setRequirements] = useState<Row>();
  const [historyDiff, setHistoryDiff] = useState<Row>();
  const [compareChoices, setCompareChoices] = useState<Row[]>([]);
  const [choiceTotal, setChoiceTotal] = useState(0);
  const [compareOwner, setCompareOwner] = useState("");
  const [workComparison, setWorkComparison] = useState<Row>();
  const [selectionVersion, setSelectionVersion] = useState(0);
  const runtime = instance?.data;
  const activeStandard =
    runtime?.plan.standard_ref || configuration?.data.standard_ref;
  useEffect(() => {
    let active = true;
    void Promise.all([
      loadCatalog(request, { id: projectId }, true),
      request("query.work_configuration", {
        project_id: projectId,
        owner_id: owner.id,
      }),
      request("query.project_configuration", { project_id: projectId }),
    ])
      .then(([rows, data, defaults]) => {
        if (active) {
          setRecords(rows);
          setConfiguration(data.configuration);
          setHistory(data.history || []);
          setSelected(
            instance?.data.plan.standard_ref ||
              data.configuration?.data.standard_ref ||
              "",
          );
          setWorkflow(
            instance?.data.workflow_ref ||
              data.configuration?.data.workflow_ref ||
              "",
          );
          setPreview(undefined);
          setWorkComparison(undefined);
          setBindings(
            instance?.data.role_agent_bindings ||
              data.configuration?.data.role_agent_bindings ||
              defaults.configuration?.data.role_agent_bindings ||
              {},
          );
          setPermission(
            instance?.data.plan.permission_context_ref ||
              data.configuration?.data.permission_context_ref ||
              defaults.configuration?.data.permission_context_ref ||
              "",
          );
        }
      })
      .catch((e) => {
        if (active) setError(String(e));
      });
    return () => {
      active = false;
    };
  }, [
    request,
    projectId,
    owner.id,
    owner.revision,
    instance?.data.active_plan_revision_id,
    selectionVersion,
  ]);
  useEffect(() => {
    let active = true;
    setRequirements(undefined);
    if (workflow)
      void request("query.workflow_requirements", {
        id: workflow,
        project_id: projectId,
      })
        .then((value) => {
          if (active) setRequirements(value);
        })
        .catch((e) => {
          if (active) setError(String(e));
        });
    return () => {
      active = false;
    };
  }, [workflow, request, projectId]);
  const snapshot = records.find((r) => r.id === selected);
  const roots: string[] =
    snapshot?.workflow_refs || snapshot?.data.root_workflow_refs || [];
  async function run(action: () => Promise<void>) {
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
  const ended = runtime && ["completed", "cancelled"].includes(runtime.status);
  return (
    <details className="work-standard">
      <summary>
        采用的规范 ·{" "}
        {records.find((r) => r.id === activeStandard)?.name ||
          activeStandard ||
          "待配置"}
      </summary>
      {activeStandard ? (
        <p>
          <a
            href={`?tab=workflows&config_tab=standard&project_id=${encodeURIComponent(projectId)}&standard_id=${encodeURIComponent(activeStandard)}`}
          >
            {activeStandard}
          </a>{" "}
          · 入口 {runtime?.workflow_ref || configuration?.data.workflow_ref}
        </p>
      ) : null}
      <p className="muted">
        快照标识所用规范；检查结果以本次执行和评审证据为准。
      </p>
      {error ? (
        <p role="alert" className="error">
          {error}
        </p>
      ) : null}
      {!ended ? (
        <>
          <RevisionSelect
            label="采用 Standard"
            records={records.filter(
              (r) =>
                r.kind === "standard" &&
                (!r.archived || r.id === activeStandard),
            )}
            value={selected}
            onChange={(value) => {
              setSelected(value);
              setWorkflow(
                records.find((r) => r.id === value)?.data
                  .root_workflow_refs[0] || "",
              );
              setPreview(undefined);
            }}
          />
          <RevisionSelect
            label="采用的入口 Workflow"
            records={records.filter((r) => roots.includes(r.id))}
            value={workflow}
            onChange={(value) => {
              setWorkflow(value);
              setPreview(undefined);
            }}
          />
          <details>
            <summary>执行者与授权</summary>
            {(requirements?.roles || []).map((role: Row) => (
              <RevisionSelect
                key={role.role_id}
                label={`${role.role?.name || role.role_id} 的 Agent`}
                records={records.filter(
                  (r) => r.kind === "agent" && r.data.role_ref === role.role_id,
                )}
                value={bindings[role.role_id] || ""}
                onChange={(value) => {
                  setBindings((old) => ({ ...old, [role.role_id]: value }));
                  setPreview(undefined);
                }}
              />
            ))}
            <RevisionSelect
              label="工作执行授权"
              records={records.filter((r) => r.kind === "permission_context")}
              value={permission}
              onChange={(value) => {
                setPermission(value);
                setPreview(undefined);
              }}
            />
          </details>
          {instance ? (
            <label>
              规范切换原因
              <input
                value={reason}
                onChange={(e) => {
                  setReason(e.target.value);
                  setPreview(undefined);
                }}
              />
            </label>
          ) : null}
          <ActionButton
            disabled={
              busy || !selected || !workflow || (!!instance && !reason.trim())
            }
            onClick={() =>
              void run(async () => {
                if (instance) {
                  const changeset = {
                    plan_revision_id: runtime.active_plan_revision_id,
                    reason,
                    operations: [
                      {
                        type: "adopt_standard",
                        standard_ref: selected,
                        workflow_ref: workflow,
                        permission_context_ref: permission || undefined,
                      },
                      { type: "set_bindings", bindings },
                    ],
                  };
                  setPreview(
                    await request("query.workflow_change_preview", {
                      project_id: projectId,
                      id: instance.id,
                      changeset,
                    }),
                  );
                } else {
                  await request("work.configuration.update", {
                    project_id: projectId,
                    command_id: crypto.randomUUID(),
                    target: owner.id,
                    expected_revision: owner.revision,
                    payload: {
                      standard_ref: selected,
                      workflow_ref: workflow,
                      role_agent_bindings: bindings,
                      permission_context_ref: permission || undefined,
                    },
                  });
                  await refresh();
                  setSelectionVersion((version) => version + 1);
                }
              })
            }
          >
            {instance ? "预演规范切换" : "保存采用的规范"}
          </ActionButton>
          {preview ? (
            <>
              <StandardDiff diff={preview.standard_diff} />
              <p>受影响步骤：{preview.affected.join("、") || "无"}</p>
              <ActionButton
                disabled={busy}
                onClick={() =>
                  void run(async () => {
                    const fresh = await request("query.workflow_instance", {
                      project_id: projectId,
                      id: instance!.id,
                    });
                    await request("workflow.change.apply", {
                      project_id: projectId,
                      command_id: crypto.randomUUID(),
                      target: instance!.id,
                      expected_revision: fresh.record.revision,
                      payload: {
                        plan_revision_id: runtime.active_plan_revision_id,
                        changeset: preview.changeset,
                        preview_hash: preview.preview_hash,
                      },
                    });
                    setPreview(undefined);
                    await refresh();
                  })
                }
              >
                应用规范切换
              </ActionButton>
            </>
          ) : null}
        </>
      ) : null}
      <details onToggle={(event) => {
        if (event.currentTarget.open && !compareChoices.length)
          void run(async () => {
            const value = await request("query.work_standard_choices", {project_id: projectId, limit: 100});
            setCompareChoices(value.records);
            setChoiceTotal(value.total);
          });
      }}>
        <summary>与其他工作比较规范</summary>
        <label>比较工作
          <select aria-label="比较工作" value={compareOwner} onChange={(event) => {setCompareOwner(event.target.value);setWorkComparison(undefined);}}>
            <option value="">请选择工作</option>
            {compareChoices.filter((row) => row.id !== owner.id).map((row) => <option key={row.id} value={row.id}>{row.project_title} / {row.work.kind === "project" ? "Project" : row.work.kind === "feature" ? "Feature" : "Task"} · {row.title}</option>)}
          </select>
        </label>
        {compareChoices.length < choiceTotal ? <ActionButton disabled={busy} onClick={() => void run(async () => {
          const value = await request("query.work_standard_choices", {project_id:projectId,offset:compareChoices.length,limit:100});
          setCompareChoices((rows) => [...rows,...value.records]);
          setChoiceTotal(value.total);
        })}>加载更多工作</ActionButton> : null}
        <ActionButton disabled={busy || !compareOwner} onClick={() => void run(async () => {
          setWorkComparison(await request("query.work_standard_diff",{project_id:projectId,before_owner_id:owner.id,after_owner_id:compareOwner}));
        })}>比较工作规范</ActionButton>
        {workComparison ? <>
          <p>双方实际入口的规范差异</p>
          <StandardDiff diff={workComparison.entry_diff} />
          <details><summary>完整快照差异与采用依据</summary>
            <StandardDiff diff={workComparison.diff} />
            {[workComparison.before,workComparison.after].map((selection:Row,index:number) => <section key={index}>
              <p>{index === 0 ? "当前工作" : "比较工作"} · {selection.standard_ref} · {selection.basis === "execution" ? "实际运行计划" : "尚未执行的配置"}</p>
              <p>子工作：{selection.children.length} · 运行例外：{selection.exceptions.length}</p>
              <ul>{selection.children.map((child:Row,index:number) => <li key={index}>{child.owner_id} · {child.standard_ref}</li>)}</ul>
            </section>)}
          </details>
        </> : null}
      </details>
      <details>
        <summary>规范采用历史（{history.length}）</summary>
        <ul>
          {history.map((plan: Row) => (
            <li key={plan.id}>
              {plan.data.standard_ref} · {plan.data.reason}
              <ActionButton
                disabled={busy || !activeStandard}
                onClick={() =>
                  void run(async () => {
                    setHistoryDiff(
                      await request("query.standard_diff", {
                        project_id: projectId,
                        before_ref: plan.data.standard_ref,
                        after_ref: activeStandard,
                      }),
                    );
                  })
                }
              >
                与当前规范比较
              </ActionButton>
            </li>
          ))}
        </ul>
        {historyDiff ? <StandardDiff diff={historyDiff} /> : null}
      </details>
    </details>
  );
}

import { displayText } from "../display-text";
import { useEffect, useId, useRef, useState } from "react";
import { ActionButton } from "./ActionFeedback";
import { loadCatalog } from "./configuration-state";
import { RevisionSelect } from "./configuration-ui";
import { type Request, type Row } from "./rpc-types";
import { StandardDiff } from "./StandardManager";

type WorkStandardProps = {
  request: Request;
  projectId: string;
  owner: Row;
  instance?: Row;
  refresh: () => Promise<void>;
};
type Panel = "standard" | "authorization";

/** The two disclosures share one frozen editing baseline and load only when opened. */
export function WorkStandard(props: WorkStandardProps) {
  return (
    <LazyWorkStandard key={`${props.projectId}:${props.owner.id}`} {...props} />
  );
}

function LazyWorkStandard(props: WorkStandardProps) {
  const [snapshot, setSnapshot] = useState<WorkStandardProps>();
  const [loading, setLoading] = useState(false);
  const [error, setError] = useState("");
  const [panels, setPanels] = useState({
    standard: false,
    authorization: false,
  });
  const panelId = useId();
  const lifetime = useRef(0);
  useEffect(
    () => () => {
      lifetime.current++;
    },
    [],
  );
  async function open() {
    if (snapshot || loading) return;
    const epoch = lifetime.current;
    const base = { ...props };
    setLoading(true);
    setError("");
    try {
      if (base.instance?.metadata_only) {
        const value = await props.request("query.workflow_management_summary", {
          project_id: props.projectId,
          id: base.instance.id,
        });
        base.instance = value.record;
      }
      if (base.instance?.data.configuration_truncated) {
        base.instance = (
          await props.request("query.workflow_instance", {
            project_id: props.projectId,
            id: base.instance.id,
          })
        ).record;
      }
      if (epoch === lifetime.current) setSnapshot(base);
    } catch (e) {
      if (epoch === lifetime.current) setError(String(e));
    } finally {
      if (epoch === lifetime.current) setLoading(false);
    }
  }
  const expanded = panels.standard || panels.authorization;
  return (
    <div className="work-standard">
      <div className="work-standard-toggles">
        {(["standard", "authorization"] as const).map((panel) => (
          <button
            key={panel}
            type="button"
            className="work-standard-toggle"
            aria-expanded={panels[panel]}
            aria-controls={`${panelId}-${panel}`}
            onClick={() => {
              if (!panels[panel]) void open();
              setPanels((current) => ({
                ...current,
                [panel]: !current[panel],
              }));
            }}
          >
            <span aria-hidden="true">{panels[panel] ? "▼" : "▶"}</span>
            {panel === "standard" ? "采用的规范" : "执行者与授权"}
          </button>
        ))}
      </div>
      <div hidden={!expanded}>
        {loading ? <p role="status">正在读取执行配置…</p> : null}
        {error ? <p role="alert">{error}</p> : null}
        {snapshot ? (
          <WorkStandardEditor
            {...snapshot}
            panels={panels}
            panelId={panelId}
            refresh={async () => {
              await props.refresh();
              setPanels({ standard: false, authorization: false });
              setSnapshot(undefined);
            }}
          />
        ) : null}
      </div>
    </div>
  );
}

/** Failed changes keep their original versions and drafts across both disclosures. */
function WorkStandardEditor({
  request,
  projectId,
  owner,
  instance,
  refresh,
  panels,
  panelId,
}: WorkStandardProps & { panels: Record<Panel, boolean>; panelId: string }) {
  const [records, setRecords] = useState<Row[]>([]);
  const [configuration, setConfiguration] = useState<Row>();
  const [selected, setSelected] = useState("");
  const [workflow, setWorkflow] = useState("");
  const [reason, setReason] = useState("");
  const [preview, setPreview] = useState<Row>();
  const [error, setError] = useState("");
  const [busy, setBusy] = useState(false);
  const [bindings, setBindings] = useState<Row>({});
  const [permission, setPermission] = useState("");
  const [requirements, setRequirements] = useState<Row>();
  const command = useRef<{ fingerprint: string; id: string }>(undefined);
  function commandId(value: Row) {
    const fingerprint = JSON.stringify(value);
    if (command.current?.fingerprint !== fingerprint)
      command.current = { fingerprint, id: crypto.randomUUID() };
    return command.current.id;
  }
  const instanceBindings = JSON.stringify(instance?.data.role_agent_bindings);
  const runtime = instance?.data;
  const activeStandard =
    runtime?.plan.standard_ref || configuration?.data.standard_ref;
  useEffect(() => {
    let active = true;
    void loadCatalog(request, projectId, true)
      .then((rows) => {
        if (active) setRecords(rows);
      })
      .catch((e) => {
        if (active) setError(String(e));
      });
    const work = request("query.work_configuration", {
      project_id: projectId,
      owner_id: owner.id,
    }).then((data) => {
      if (active) setConfiguration(data.configuration);
      return data;
    });
    void Promise.all([
      work,
      request("query.project_configuration", { project_id: projectId }),
    ])
      .then(([data, defaults]) => {
        if (!active) return;
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
        setBindings(
          (instanceBindings ? JSON.parse(instanceBindings) : undefined) ||
            data.configuration?.data.role_agent_bindings ||
            defaults.configuration?.data.role_agent_bindings ||
            {},
        );
        setPermission(
          instance?.data.plan.harness_permission_ref ||
            data.configuration?.data.harness_permission_ref ||
            defaults.configuration?.data.harness_permission_ref ||
            "",
        );
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
    instance?.data.plan.standard_ref,
    instance?.data.plan.harness_permission_ref,
    instance?.data.workflow_ref,
    instanceBindings,
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
    snapshot?.workflow_refs || snapshot?.root_workflow_refs || [];
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
    <>
      {error ? (
        <p role="alert" className="error">
          {displayText(error)}
        </p>
      ) : null}
      <section
        id={`${panelId}-standard`}
        className="work-standard-content"
        hidden={!panels.standard}
        aria-label="采用的规范内容"
      >
        {activeStandard ? (
          <p>
            <a
              href={`?tab=workflows&config_tab=standard&project_id=${encodeURIComponent(projectId)}&standard_id=${encodeURIComponent(activeStandard)}`}
            >
              {displayText(activeStandard)}
            </a>
            {" · 入口 "}
            {displayText(
              runtime?.workflow_ref || configuration?.data.workflow_ref,
            )}
          </p>
        ) : null}
        <p className="muted">
          快照标识所用规范；检查结果以本次执行和评审证据为准。
        </p>
        {!ended ? (
          <fieldset
            className="work-standard-fields"
            disabled={!instance || busy}
          >
            <RevisionSelect
              label="采用 Standard"
              records={records.filter(
                (r) =>
                  r.type === "standard" &&
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
          </fieldset>
        ) : null}
      </section>
      <section
        id={`${panelId}-authorization`}
        className="work-standard-content"
        hidden={!panels.authorization}
        aria-label="执行者与授权内容"
      >
        <fieldset
          className="work-standard-fields"
          disabled={!instance || ended || busy}
        >
          {(requirements?.roles || []).map((role: Row) => (
            <RevisionSelect
              key={role.role_id}
              label={`${role.role?.name || role.role_id} 的 Agent`}
              records={records.filter(
                (r) => r.type === "agent" && r.role_ref === role.role_id,
              )}
              value={bindings[role.role_id] || ""}
              onChange={(value) => {
                setBindings((old) => ({ ...old, [role.role_id]: value }));
                setPreview(undefined);
              }}
            />
          ))}
          <RevisionSelect
            label="Harness Profile"
            records={records.filter(
              (r) =>
                r.type === "permission_profile" &&
                r.data.target_kind === "harness",
            )}
            value={permission}
            onChange={(value) => {
              setPermission(value);
              setPreview(undefined);
            }}
          />
        </fieldset>
      </section>
      {instance && !ended ? (
        <div className="work-standard-actions">
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
          <ActionButton
            disabled={busy || !selected || !workflow || !reason.trim()}
            onClick={() =>
              void run(async () => {
                const changeset = {
                  plan_revision_id: runtime.active_plan_revision_id,
                  reason,
                  operations: [
                    {
                      type: "adopt_standard",
                      standard_ref: selected,
                      workflow_ref: workflow,
                      harness_permission_ref: permission || undefined,
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
              })
            }
          >
            预演规范切换
          </ActionButton>
          {preview ? (
            <>
              <StandardDiff diff={preview.standard_diff} />
              <p>
                受影响步骤：{displayText(preview.affected.join("、") || "无")}
              </p>
              <ActionButton
                disabled={busy}
                onClick={() =>
                  void run(async () => {
                    await request("workflow.change.apply", {
                      project_id: projectId,
                      command_id: commandId({
                        instance: instance.id,
                        revision: instance.revision,
                        preview,
                      }),
                      target: instance.id,
                      expected_revision: instance.revision,
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
        </div>
      ) : null}
    </>
  );
}

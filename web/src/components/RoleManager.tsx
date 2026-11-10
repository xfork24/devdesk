import {
  ActionButton,
  ActionForm,
  ActionSelect,
  useActionFeedback,
} from "./ActionFeedback";
import { useCallback, useEffect, useRef, useState } from "react";
import {
  dutyLabels,
  latest,
  MarkdownField,
  useDraft,
  type Row,
  type Request,
} from "./configuration-ui";
export function RoleManager({
  request,
  project,
  onChanged,
}: {
  request: Request;
  project?: Row;
  onChanged?: () => void;
}) {
  const {
    error,
    setError,
    message,
    setMessage,
    capture: captureFeedback,
    beginFeedback,
    endFeedback,
  } = useActionFeedback();

  const [rows, setRows] = useState<Row[]>([]),
    [projects, setProjects] = useState<Row[]>([]),
    [busy, setBusy] = useState(false);
  const empty = () => ({
    selected: "",
    name: "",
    instructions: "",
    execution_role: "analyst",
    visibility: { kind: "global", project_ids: [] as string[] },
  });
  const [draft, setDraft] = useDraft(
    `devdesk:v2:roles:${project?.id || "global"}`,
    empty,
  );
  const load = useCallback(async () => {
    const [roles, workspace] = await Promise.all([
      request("query.roles", {
        local: !!project,
        project_id: project?.id,
        limit: 100,
        include_history: true,
      }),
      request("query.workspace", {
        project_offset: 0,
        limit: 100,
      }),
    ]);
    setRows((roles.records || []).map((row:Row)=>({...row,template_source:roles.template_sources?.[row.id]})));
    setProjects(workspace.projects || []);
  }, [request, project?.id]);
  useEffect(() => {
    let active = true;
    void load().catch((e) => {
      if (active) setError(String(e));
    });
    return () => {
      active = false;
    };
  }, [load]);
  const choose = (row: Row) => {
    setDraft({
      selected: row.id,
      name: row.name,
      instructions: row.data.instructions,
      execution_role: row.data.execution_role,
      visibility: {
        kind:
          row.data.visibility?.kind ||
          (row.scope.kind === "global" ? "global" : "projects"),
        project_ids:
          row.data.visibility?.project_ids ||
          [row.scope.project_id].filter(Boolean),
      },
    });
    const params = new URLSearchParams(location.search);
    params.set("config_object", row.id);
    history.replaceState(null, "", `?${params}`);
  };
  const routeConsumed = useRef(false);
  useEffect(() => {
    if (routeConsumed.current || !rows.length) return;
    const id = new URLSearchParams(location.search).get("config_object");
    const row = rows.find((r) => r.id === id);
    if (row) {
      routeConsumed.current = true;
      if (draft.selected !== row.id) choose(row);
    }
  }, [rows]);
  const old = rows.find((r) => r.id === draft.selected);
  async function save(remove = false) {
    captureFeedback();

    (beginFeedback(), setBusy(true));
    setError("");
    setMessage("");
    try {
      const role = await request(
        remove ? "role.delete" : old ? "role.revise" : "role.create",
        {
          project_id: project?.id,
          command_id: crypto.randomUUID(),
          target: old?.id,
          expected_revision: old?.revision,
          payload: remove
            ? {}
            : {
                name: draft.name,
                scope: { kind: "global" },
                data: {
                  execution_role: draft.execution_role,
                  instructions: draft.instructions,
                  visibility:
                    draft.visibility.kind === "global"
                      ? { kind: "global" }
                      : {
                          kind: "projects",
                          project_ids: draft.visibility.project_ids,
                        },
                },
              },
        },
      );
      if (remove) setDraft(empty());
      else {
        setDraft((current) => ({ ...current, selected: role.id }));
        const params = new URLSearchParams(location.search);
        params.set("config_object", role.id);
        history.replaceState(null, "", `?${params}`);
      }
      await load();
      onChanged?.();
      setMessage(
        remove
          ? "角色已停用，历史保留。"
          : "Role 已保存；其他配置保持其已选修订。",
      );
    } catch (e) {
      setError(String(e));
    } finally {
      (endFeedback(), setBusy(false));
    }
  }
  return (
    <section className="panel role-manager" aria-label="角色管理">
      <div className="panel-heading">
        <div>
          <h2>角色 Role</h2>
          <p>定义职责和可见范围，再到 Agent tab 将角色与能力组合。</p>
        </div>
      </div>
      <div className="configuration-layout">
        <div className="configuration-list">
          <ActionButton
            className="new-resource"
            onClick={() => {
              captureFeedback();

              const params = new URLSearchParams(location.search);
              params.delete("config_object");
              history.replaceState(null, "", `?${params}`);
              setDraft(empty());
            }}
          >
            新建 Role
          </ActionButton>

          {latest(rows).map((row) => (
            <ActionButton
              key={row.id}
              className={draft.selected === row.id ? "selected" : ""}
              onClick={() => {
                captureFeedback();

                choose(row);
                setMessage("");
              }}
            >
              <strong>{row.name}</strong>
              <span>
                {dutyLabels[row.data.execution_role]} · v{row.version}
              </span>
            </ActionButton>
          ))}
        </div>
        <ActionForm
          onSubmit={(e) => {
            captureFeedback();

            e.preventDefault();
            void save();
          }}
        >
          <label>
            Role 名称
            <input
              required
              value={draft.name}
              onChange={(e) => {
                captureFeedback();
                return setDraft({ ...draft, name: e.target.value });
              }}
            />
          </label>
          <label>
            Role Scope
            <ActionSelect
              value={draft.visibility.kind}
              onChange={(e) => {
                captureFeedback();
                return setDraft({
                  ...draft,
                  visibility: { kind: e.target.value, project_ids: [] },
                });
              }}
            >
              <option value="global">Global · 所有项目</option>
              <option value="projects">指定项目</option>
            </ActionSelect>
          </label>
          {draft.visibility.kind === "projects" && (
            <label>
              可见项目
              <ActionSelect
                multiple
                required
                value={draft.visibility.project_ids}
                onChange={(e) => {
                  captureFeedback();
                  return setDraft({
                    ...draft,
                    visibility: {
                      kind: "projects",
                      project_ids: Array.from(
                        e.target.selectedOptions,
                        (o) => o.value,
                      ),
                    },
                  });
                }}
              >
                {projects.map((row) => (
                  <option value={row.id} key={row.id}>
                    {row.title}
                  </option>
                ))}
              </ActionSelect>
            </label>
          )}
          <MarkdownField
            key={draft.selected || "new"}
            label="职责说明"
            value={draft.instructions}
            required
            onChange={(instructions) => {
              captureFeedback();
              return setDraft({ ...draft, instructions });
            }}
          />
          <details>
            <summary>执行职责设置</summary>
            <label>
              执行职责
              <ActionSelect
                value={draft.execution_role}
                onChange={(e) => {
                  captureFeedback();
                  return setDraft({ ...draft, execution_role: e.target.value });
                }}
              >
                {Object.entries(dutyLabels).map(([id, label]) => (
                  <option value={id} key={id}>
                    {label}
                  </option>
                ))}
              </ActionSelect>
            </label>
            <p>用于动作和写入范围校验；Role 名称可以自由设置。</p>
          </details>
          <div className="actions">
            <ActionButton
              className="primary"
              disabled={
                busy || !draft.name.trim() || !draft.instructions.trim()
              }
            >
              {old ? "保存 Role 新版本" : "创建 Role"}
            </ActionButton>
            <ActionButton
              type="button"
              disabled={busy || !old}
              onClick={() => {
                captureFeedback();
                return void save(true);
              }}
            >
              删除 Role
            </ActionButton>
          </div>
        </ActionForm>
      </div>
      {error && (
        <p role="alert" className="error">
          {error}
        </p>
      )}
      {message && (
        <p role="status" className="notice">
          {message}
        </p>
      )}
    </section>
  );
}

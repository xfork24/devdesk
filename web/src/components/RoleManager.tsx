import { useReadRequest, ignoreCancelledRefresh } from "./use-read-request";
import { ActionButton, ActionForm, ActionSelect } from "./ActionFeedback";
import { useActionFeedback } from "./action-feedback";
import { useCallback, useEffect, useRef, useState } from "react";
import {
  latest,
  loadCatalog,
  loadConfigurationRecord,
  useDraft,
} from "./configuration-state";
import { MarkdownField } from "./configuration-ui";
import { type Row, type Request } from "./rpc-types";
export function RoleManager({
  request: parentRequest,
  project,
  onChanged,
}: {
  request: Request;
  project?: Row;
  onChanged?: () => void;
}) {
  const request = useReadRequest(parentRequest);
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
    visibility: { kind: "global", project_ids: [] as string[] },
  });
  const [draft, setDraft] = useDraft(
    `devdesk:v4:roles:${project?.id || "global"}`,
    empty,
  );
  const [detailLoading, setDetailLoading] = useState(false);
  const choice = useRef<AbortController | null>(null);
  useEffect(() => () => choice.current?.abort(), []);
  const load = useCallback(async () => {
    const [roles, workspace] = await Promise.all([
      loadCatalog(request, project?.id, false, "role"),
      request("query.project_directory", {
        project_offset: 0,
        limit: 100,
      }),
    ]);
    setRows(roles);
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
  }, [load, setError]);
  const choose = useCallback(
    async (row: Row) => {
      choice.current?.abort();
      const controller = new AbortController();
      choice.current = controller;
      setDetailLoading(true);
      if (!row.type || row.type === "role") {
        const params = new URLSearchParams(location.search);
        params.set("config_object", row.id);
        history.replaceState(null, "", `?${params}`);
      }
      try {
        row = await loadConfigurationRecord(
          request,
          row,
          project?.id,
          controller.signal,
        );
        if (controller.signal.aborted || row.type !== "role") return;
        setRows((rows) => [...rows.filter((r) => r.id !== row.id), row]);
        setDraft({
          selected: row.id,
          name: row.name,
          instructions: row.data.instructions,
          visibility: {
            kind:
              row.data.visibility?.kind ||
              (row.scope === "global" ? "global" : "projects"),
            project_ids:
              row.data.visibility?.project_ids ||
              [row.scope.split(":")[1]].filter(Boolean),
          },
        });
        const params = new URLSearchParams(location.search);
        params.set("config_object", row.id);
        history.replaceState(null, "", `?${params}`);
      } catch (error) {
        if (!controller.signal.aborted) setError(String(error));
      } finally {
        if (choice.current === controller) setDetailLoading(false);
      }
    },
    [setDraft, request, project?.id, setError],
  );
  const routeConsumed = useRef(false);
  useEffect(() => {
    if (routeConsumed.current || !rows.length) return;
    const id = new URLSearchParams(location.search).get("config_object");
    const row =
      rows.find((r) => r.id === id) || (id ? { id, _index: true } : undefined);
    if (row) {
      routeConsumed.current = true;
      if (draft.selected !== row.id) choose(row);
    }
  }, [choose, draft.selected, rows]);
  const old = rows.find((r) => r.id === draft.selected);
  async function save(remove = false) {
    captureFeedback();

    beginFeedback();
    setBusy(true);
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
                scope: "global",
                data: {
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
      await load().catch(ignoreCancelledRefresh);
      onChanged?.();
      setMessage(
        remove
          ? "角色已停用，历史保留。"
          : "Role 已保存；其他配置保持其已选修订。",
      );
    } catch (e) {
      setError(String(e));
    } finally {
      endFeedback();
      setBusy(false);
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
              choice.current?.abort();
              setDetailLoading(false);
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
              <span>v{row.version}</span>
            </ActionButton>
          ))}
        </div>
        <ActionForm
          className="form-grid"
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
              disabled={detailLoading}
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
            disabled={detailLoading}
            label="职责说明"
            value={draft.instructions}
            required
            onChange={(instructions) => {
              captureFeedback();
              return setDraft({ ...draft, instructions });
            }}
          />
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
              disabled={busy || detailLoading || !old}
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

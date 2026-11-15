import { ActionButton, ActionSelect, ActionInput } from "./ActionFeedback";
import { useActionFeedback } from "./action-feedback";
import { useEffect, useRef, useState } from "react";
import { DirectoryPicker } from "./DirectoryPicker";
import { type Request } from "./rpc-types";
import { PermissionProfileHistory } from "./PermissionProfileHistory";
import {
  profileView,
  profileSource,
  usePermissionResources,
  type PermissionProfile as Profile,
  type PermissionKind,
} from "./permission-resources-state";

type Props = {
  request: Request;
  refresh: () => Promise<void>;
  projectId?: string;
};

export function PermissionResources({ request, refresh, projectId }: Props) {
  const {
    message,
    setMessage,
    capture: captureFeedback,
    beginFeedback,
    endFeedback,
  } = useActionFeedback();

  const [linkedProfile] = useState(
    () => new URLSearchParams(location.search).get("permission_ref") || "",
  );
  const hydrated = useRef(false);
  const {
    catalog,
    error,
    reload: loadCatalog,
  } = usePermissionResources(
    request,
    projectId,
    undefined,
    linkedProfile || undefined,
  );
  const [selected, setSelected] = useState(linkedProfile);
  const [kind, setKind] = useState<PermissionKind>("harness");
  const [draft, setDraft] = useState("");
  const [busy, setBusy] = useState(false);
  const [choosingSource, setChoosingSource] = useState(false);
  const profile = catalog?.profiles.find((p) => p.id === selected);
  useEffect(() => {
    if (hydrated.current || !linkedProfile || !catalog) return;
    const profile = catalog.profiles.find((p) => p.id === linkedProfile);
    if (profile) {
      setKind(profile.target_kind);
      setDraft(JSON.stringify(profileSource(profile), null, 2));
      hydrated.current = true;
    }
  }, [catalog, linkedProfile]);
  async function reload() {
    await loadCatalog();
    await refresh();
  }
  async function action(work: () => Promise<void>) {
    captureFeedback();

    beginFeedback();
    setBusy(true);
    setMessage("");
    try {
      await work();
    } catch (e) {
      setMessage(e instanceof Error ? e.message : String(e));
    } finally {
      endFeedback();
      setBusy(false);
    }
  }
  const command = (method: string, payload: unknown, extra = {}) =>
    request(method, {
      command_id: crypto.randomUUID(),
      payload,
      project_id: projectId,
      ...extra,
    });
  function parseDraft() {
    const value = JSON.parse(draft);
    if (!value || typeof value !== "object" || Array.isArray(value))
      throw new Error("权限 JSON 必须是对象");
    for (const key of Object.keys(value)) {
      if (
        !["type", "identity", "revision", "name", "scope", "data"].includes(key)
      )
        throw new Error(`未知权限字段：${key}`);
    }
    if (value.type !== "permission_profile")
      throw new Error(
        "此编辑器使用 permission_profile 源定义与 schema_version 2",
      );
    for (const key of Object.keys(value.data)) {
      if (!["schema_version", "target_kind", "policy"].includes(key))
        throw new Error(`未知权限字段：${key}`);
    }
    return value;
  }
  function edit(p: Profile) {
    setSelected(p.id);
    setKind(p.target_kind);
    setDraft(JSON.stringify(profileSource(p), null, 2));
    const params = new URLSearchParams(location.search);
    params.set("permission_ref", p.id);
    history.replaceState(null, "", `?${params}`);
  }
  function fresh() {
    setSelected("");
    const params = new URLSearchParams(location.search);
    params.delete("permission_ref");
    history.replaceState(null, "", `?${params}`);
    setDraft(
      JSON.stringify(
        {
          type: "permission_profile",
          identity: `custom-${crypto.randomUUID()}`,
          revision: 1,
          name: "新权限",
          scope: "global",
          data: {
            schema_version: 2,
            target_kind: kind,
            policy: {
              filesystem: { mode: "read_only" },
              network: { mode: "enabled" },
              ...(kind === "harness"
                ? { tools: { mode: "all_available" }, approval: "never" }
                : {}),
            },
          },
        },
        null,
        2,
      ),
    );
  }
  if (!catalog)
    return (
      <section>
        <p role="status">{error || message || "正在读取版本化权限…"}</p>
      </section>
    );
  return (
    <>
      <section
        className="panel permission-definitions"
        aria-label="Permission 定义"
      >
        <div className="integration-content">
          <h3>Permission 定义</h3>
          <p className="muted">
            保存产生独立修订。应用后，追随目标的新讨论采用新权限，已有讨论在下一轮切换；工程冻结与固定选择保留原版本。
          </p>
          {error && (
            <p role="alert" className="error">
              {error}
            </p>
          )}
          <fieldset disabled={busy}>
            <legend>权限定义</legend>
            <div className="permission-definition-toolbar">
              <label>
                权限类型
                <ActionSelect
                  aria-label="权限类型"
                  value={kind}
                  onChange={(e) => {
                    setKind(e.target.value as PermissionKind);
                    setSelected("");
                    setDraft("");
                  }}
                >
                  <option value="harness">Harness</option>
                  <option value="sandbox">Sandbox</option>
                </ActionSelect>
              </label>
              <label>
                权限修订
                <ActionSelect
                  aria-label="版本化权限修订"
                  value={selected}
                  onChange={(e) => {
                    captureFeedback();

                    const p = catalog.profiles.find(
                      (p) => p.id === e.target.value,
                    );
                    if (p) edit(p);
                  }}
                >
                  <option value="">选择权限</option>
                  {catalog.profiles
                    .filter((p) => p.target_kind === kind)
                    .map((p) => (
                      <option key={p.id} value={p.id}>
                        {p.name} · {p.target_kind} · v{p.version}
                      </option>
                    ))}
                </ActionSelect>
              </label>
              <ActionButton type="button" onClick={fresh}>
                新建权限草稿
              </ActionButton>
            </div>
            <label>
              统一权限 JSON
              <textarea
                aria-label="统一权限 JSON"
                rows={14}
                value={draft}
                onChange={(e) => {
                  captureFeedback();
                  return setDraft(e.target.value);
                }}
              />
            </label>
            <div className="actions">
              <ActionButton
                type="button"
                disabled={!draft}
                onClick={() => {
                  captureFeedback();
                  return action(async () => {
                    const p = parseDraft();
                    const saved = await command("permission.profile.create", {
                      identity: p.identity,
                      name: p.name,
                      target_kind: p.data.target_kind,
                      policy: p.data.policy,
                    });
                    await reload();
                    edit(profileView(saved));
                    setMessage("已创建权限修订，尚未应用绑定。");
                  });
                }}
              >
                创建独立权限
              </ActionButton>
              <ActionButton
                type="button"
                disabled={!profile}
                onClick={() => {
                  captureFeedback();
                  return action(async () => {
                    const p = parseDraft();
                    const saved = await command(
                      "permission.profile.revise",
                      {
                        name: p.name,
                        target_kind: p.data.target_kind,
                        policy: p.data.policy,
                      },
                      {
                        target: profile!.id,
                        expected_revision: profile!.version,
                      },
                    );
                    await reload();
                    edit(profileView(saved));
                    setMessage("新修订已保存，原绑定保持不变。");
                  });
                }}
              >
                保存权限新修订
              </ActionButton>
              <ActionButton
                type="button"
                disabled={!profile}
                onClick={() => {
                  captureFeedback();
                  return action(async () => {
                    const bundle = await request("permission.profile.export", {
                      id: profile!.id,
                    });
                    const url = URL.createObjectURL(
                      new Blob([JSON.stringify(bundle, null, 2)], {
                        type: "application/json",
                      }),
                    );
                    const a = document.createElement("a");
                    a.href = url;
                    a.download = "permission-profiles.json";
                    a.click();
                    setTimeout(() => URL.revokeObjectURL(url), 1000);
                    setMessage("已导出权限及前序修订。");
                  });
                }}
              >
                导出权限
              </ActionButton>
            </div>
            <label>
              导入权限文件
              <ActionInput
                aria-label="导入权限文件"
                type="file"
                accept="application/json,.json"
                onChange={(e) => {
                  captureFeedback();

                  const file = e.target.files?.[0];
                  if (!file) return;
                  e.target.value = "";
                  void action(async () => {
                    if (file.size > 4 * 1024 * 1024)
                      throw new Error("导入文件超过 4MiB");
                    const value = JSON.parse(await file.text());
                    await command("permission.profile.import", {
                      profiles: value.profiles || [value],
                    });
                    await reload();
                    setMessage("权限已导入，尚未改变活动绑定。");
                  });
                }}
              />
            </label>
          </fieldset>
          <PermissionProfileHistory
            catalog={catalog}
            profileId={selected}
            onSelect={edit}
            disabled={busy}
          />
          <details>
            <summary>预定义来源与历史</summary>
            <p>
              当前预设目录（{catalog.source.is_builtin ? "内置" : "自定义"}）：
              <span
                className="permission-source-path"
                aria-label="当前预设目录"
              >
                {catalog.source.path}
              </span>
            </p>
            <p className="muted">
              仅从当前目录加载预设。切换目录后点击“重新加载预设”；“重新加载内置预设”会恢复内置目录并加载。已导入的权限版本与活动绑定保持不变。
            </p>
            <div className="actions">
              <ActionButton
                type="button"
                disabled={busy}
                onClick={() => {
                  captureFeedback();
                  setChoosingSource(true);
                }}
              >
                打开文件夹
              </ActionButton>
              <ActionButton
                type="button"
                disabled={busy}
                onClick={() => {
                  captureFeedback();
                  return action(async () => {
                    const result = await command(
                      "permission.profile.reload",
                      {},
                    );
                    await reload();
                    setMessage(
                      `预设重新加载完成：新增 ${result.added.length}，相同 ${result.unchanged.length}；活动绑定保持不变。`,
                    );
                  });
                }}
              >
                重新加载预设
              </ActionButton>
              <ActionButton
                type="button"
                disabled={busy}
                onClick={() => {
                  captureFeedback();
                  return action(async () => {
                    const result = await command("permission.profile.reload", {
                      builtin: true,
                    });
                    await reload();
                    setMessage(
                      `内置预设重新加载完成：新增 ${result.added.length}，相同 ${result.unchanged.length}；已恢复内置目录，活动绑定保持不变。`,
                    );
                  });
                }}
              >
                重新加载内置预设
              </ActionButton>
            </div>
            {choosingSource ? (
              <DirectoryPicker
                request={request}
                initialPath={catalog.source.path}
                onClose={() => setChoosingSource(false)}
                onSelect={async (path) => {
                  await command("permission.source.select", { path });
                  await reload();
                  setMessage("预设目录已替换，可重新加载预设。");
                }}
              />
            ) : null}
            <pre>
              {JSON.stringify(
                {
                  origins: catalog.origins.filter(
                    (origin) => !selected || origin.definition_id === selected,
                  ),
                },
                null,
                2,
              )}
            </pre>
          </details>
          <p role="status">{message}</p>
        </div>
      </section>
    </>
  );
}

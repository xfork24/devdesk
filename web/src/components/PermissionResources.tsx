import {
  ActionButton,
  ActionForm,
  ActionSelect,
  ActionInput,
  useActionFeedback,
} from "./ActionFeedback";
import { useEffect, useState } from "react";
import { ResourceEditor } from "./AgentCatalog";
import { DirectoryPicker } from "./DirectoryPicker";

type Request = (
  method: string,
  params?: Record<string, unknown>,
) => Promise<any>;
type Profile = {
  schema_version: number;
  scope: string;
  id: string;
  identity: string;
  name: string;
  version: number;
  target_kind: "harness" | "sandbox";
  policy: Record<string, unknown>;
};
const profileView = (source: any): Profile => ({
  ...source.data,
  scope: source.scope,
  identity: source.identity,
  version: source.revision,
  name: source.name,
  id: `${source.identity}:${source.revision}`,
});
const profileSource = (p: Profile) => ({
  type: "permission_profile",
  identity: p.identity,
  revision: p.version,
  name: p.name,
  scope: p.scope,
  data: { schema_version: 2, target_kind: p.target_kind, policy: p.policy },
});
const catalogView = (source: Catalog): Catalog => ({
  ...source,
  profiles: source.profiles.map(profileView),
});
type Target = {
  target: {
    kind: "harness" | "sandbox";
    revision_id: string;
    content_hash: string;
  };
  name: string;
  version: number;
  provider: string;
  runtime_version: string;
  supported: boolean;
  allowed_permission_ids?: string[];
  unsupported_reason?: string;
  platforms: string[];
  recipe_digest?: string;
  enforcement_contract?: string;
  registered_installations: unknown[];
  materializations: unknown[];
  active?: { revision: number; binding_revision_id: string };
};
type Catalog = {
  profiles: Profile[];
  targets: Target[];
  bindings: unknown[];
  source: { path: string; builtin_path: string; is_builtin: boolean };
  origins: unknown[];
  default_sandbox_ref: string;
};

export function PermissionResources({
  request,
  refresh,
  projectId,
}: {
  request: Request;
  refresh: () => Promise<void>;
  projectId?: string;
}) {
  const {
    message,
    setMessage,
    capture: captureFeedback,
    beginFeedback,
    endFeedback,
  } = useActionFeedback();

  const [catalog, setCatalog] = useState<Catalog>();
  const [selected, setSelected] = useState("");
  const [targetId, setTargetId] = useState("");
  const [bindingProfileId, setBindingProfileId] = useState("");
  const [draft, setDraft] = useState("");

  const [busy, setBusy] = useState(false);
  const [preview, setPreview] = useState<unknown>();
  const [choosingSource, setChoosingSource] = useState(false);
  const [scope, setScope] = useState(
    '{"work_type":"discussion","writable_paths":[],"read_only_paths":[]}',
  );
  const [harnessId, setHarnessId] = useState("");
  const [sandboxId, setSandboxId] = useState("");
  useEffect(() => {
    let active = true;
    request("query.permission_resources", { project_id: projectId })
      .then((value: Catalog) => {
        if (active) setCatalog(catalogView(value));
      })
      .catch((e: Error) => {
        if (active) setMessage(e.message);
      });
    return () => {
      active = false;
    };
  }, [request, projectId]);
  const profile = catalog?.profiles.find((p) => p.id === selected);
  const target = catalog?.targets.find(
    (t) => t.target.revision_id === targetId,
  );
  const bindingProfiles = (catalog?.profiles || []).filter(
    (p) =>
      target &&
      p.target_kind === target.target.kind &&
      (!target.allowed_permission_ids ||
        target.allowed_permission_ids.includes(p.id)),
  );
  const bindingProfile = bindingProfiles.find((p) => p.id === bindingProfileId);
  useEffect(() => {
    if (!target || !catalog) {
      setBindingProfileId("");
      return;
    }
    const active = (catalog.bindings as any[]).find(
      (row) => row.id === target.active?.binding_revision_id,
    );
    const activeProfile = active?.data?.permission_revision_id;
    setBindingProfileId((previous) =>
      bindingProfiles.some((p) => p.id === previous)
        ? previous
        : bindingProfiles.some((p) => p.id === activeProfile)
          ? activeProfile
          : "",
    );
  }, [targetId, catalog]);
  async function reload() {
    captureFeedback();

    setCatalog(
      catalogView(
        await request("query.permission_resources", { project_id: projectId }),
      ),
    );
    await refresh();
  }
  async function action(work: () => Promise<void>) {
    captureFeedback();

    (beginFeedback(), setBusy(true));
    setMessage("");
    try {
      await work();
    } catch (e) {
      setMessage(e instanceof Error ? e.message : String(e));
    } finally {
      (endFeedback(), setBusy(false));
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
    setDraft(JSON.stringify(profileSource(p), null, 2));
    setPreview(undefined);
  }
  function fresh() {
    const kind = target?.target.kind || "harness";
    setSelected("");
    setPreview(undefined);
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
        <p role="status">{message || "正在读取版本化权限…"}</p>
      </section>
    );
  return (
    <>
      <ResourceEditor
        kind="permission_context"
        request={request}
        project={projectId ? { id: projectId } : undefined}
        installations={[]}
        revision={0}
      />
      <section className="setting-row">
        <div>
          <h3>Permission 定义与版本绑定</h3>
          <p className="muted">
            保存产生独立修订。应用后，追随目标的新讨论采用新权限，已有讨论在下一轮切换；工程冻结与固定选择保留原版本。
          </p>
          <fieldset disabled={busy}>
            <legend>权限定义</legend>
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
                {catalog.profiles.map((p) => (
                  <option key={p.id} value={p.id}>
                    {p.name} · {p.target_kind} · v{p.version}
                  </option>
                ))}
              </ActionSelect>
            </label>
            <ActionButton type="button" onClick={fresh}>
              新建权限草稿
            </ActionButton>
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
          <fieldset disabled={busy}>
            <legend>目标版本绑定</legend>
            <label>
              目标资源修订
              <ActionSelect
                aria-label="权限目标版本"
                value={targetId}
                onChange={(e) => {
                  captureFeedback();

                  setTargetId(e.target.value);
                  setPreview(undefined);
                }}
              >
                <option value="">选择 Harness 或 Sandbox 版本</option>
                {catalog.targets.map((t) => (
                  <option
                    key={t.target.revision_id}
                    value={t.target.revision_id}
                  >
                    {t.name} · {t.target.kind} · v{t.version} · {t.provider}{" "}
                    {t.runtime_version}
                  </option>
                ))}
              </ActionSelect>
            </label>
            {target ? (
              <>
                <label>
                  应用到目标的权限
                  <ActionSelect
                    aria-label="目标资源权限"
                    value={bindingProfileId}
                    onChange={(event) => {
                      setBindingProfileId(event.target.value);
                      setPreview(undefined);
                    }}
                    required
                  >
                    <option value="" disabled>
                      {bindingProfiles.length
                        ? "请选择可应用的权限修订"
                        : "此目标没有兼容的权限修订"}
                    </option>
                    {bindingProfiles.map((p) => (
                      <option key={p.id} value={p.id}>
                        {p.name} · v{p.version} · {p.target_kind}
                      </option>
                    ))}
                  </ActionSelect>
                </label>
                <p className="muted">
                  仅列出与目标类型及执行能力兼容的权限。此选择独立于上方的权限编辑器。
                </p>
                <p>
                  当前绑定：{target.active?.binding_revision_id || "尚未分配"} ·
                  revision {target.active?.revision || 0}
                  {!target.supported ? ` · ${target.unsupported_reason}` : ""}
                </p>
                <details>
                  <summary>构建、平台与安装登记</summary>
                  <p className="muted">
                    安装登记仍需在执行前核验。目标兼容性预检不会下载或安装运行环境。
                  </p>
                  <pre>
                    {JSON.stringify(
                      {
                        target: target.target,
                        platforms: target.platforms,
                        recipe_digest: target.recipe_digest,
                        enforcement_contract: target.enforcement_contract,
                        registered_installations:
                          target.registered_installations,
                        materializations: target.materializations,
                      },
                      null,
                      2,
                    )}
                  </pre>
                </details>
              </>
            ) : null}
            <div className="actions">
              <ActionButton
                type="button"
                disabled={!bindingProfile || !target}
                onClick={() => {
                  captureFeedback();
                  return action(async () => {
                    setPreview(
                      await request("permission.binding.preflight", {
                        project_id: projectId,
                        target: target!.target,
                        permission_revision_id: bindingProfile!.id,
                      }),
                    );
                    setMessage("目标兼容性预检通过，仍需实际执行范围校验。");
                  });
                }}
              >
                预检目标绑定
              </ActionButton>
              <ActionButton
                type="button"
                disabled={!bindingProfile || !target || !target.supported}
                onClick={() => {
                  captureFeedback();
                  return action(async () => {
                    await command("permission.bindings.apply", {
                      bindings: [
                        {
                          target: target!.target,
                          permission_revision_id: bindingProfile!.id,
                          expected_revision: target!.active?.revision || 0,
                        },
                      ],
                    });
                    await reload();
                    setMessage(
                      "活动绑定已更新；已有工作保留冻结权限，追随目标的讨论将在下一轮切换。",
                    );
                  });
                }}
              >
                应用到该目标版本
              </ActionButton>
            </div>
          </fieldset>
          <details>
            <summary>实际执行范围预览</summary>
            <label>
              Harness 组合目标
              <ActionSelect
                value={harnessId}
                onChange={(e) => {
                  captureFeedback();
                  return setHarnessId(e.target.value);
                }}
              >
                <option value="">选择 Harness 修订</option>
                {catalog.targets
                  .filter((t) => t.target.kind === "harness")
                  .map((t) => (
                    <option
                      key={t.target.revision_id}
                      value={t.target.revision_id}
                    >
                      {t.name} · v{t.version}
                    </option>
                  ))}
              </ActionSelect>
            </label>
            <label>
              Sandbox 组合目标
              <ActionSelect
                value={sandboxId}
                onChange={(e) => {
                  captureFeedback();
                  return setSandboxId(e.target.value);
                }}
              >
                <option value="">选择 Sandbox 修订</option>
                {catalog.targets
                  .filter((t) => t.target.kind === "sandbox")
                  .map((t) => (
                    <option
                      key={t.target.revision_id}
                      value={t.target.revision_id}
                    >
                      {t.name} · v{t.version}
                    </option>
                  ))}
              </ActionSelect>
            </label>
            <label>
              ExecutionScope JSON
              <textarea
                value={scope}
                onChange={(e) => {
                  captureFeedback();
                  return setScope(e.target.value);
                }}
                rows={5}
              />
            </label>
            <ActionButton
              type="button"
              disabled={busy || !harnessId || !sandboxId}
              onClick={() => {
                captureFeedback();
                return action(async () => {
                  setPreview(
                    await request("permission.binding.preflight", {
                      project_id: projectId,
                      harness_ref: harnessId,
                      selection: {
                        sandbox_ref: sandboxId,
                        harness: { mode: "follow_target" },
                        sandbox: { mode: "follow_target" },
                      },
                      scope: JSON.parse(scope),
                    }),
                  );
                  setMessage(
                    "完整范围预览通过；正式执行仍由协调器核验并冻结。",
                  );
                });
              }}
            >
              预览两层实际权限
            </ActionButton>
          </details>
          {preview !== undefined ? (
            <details open>
              <summary>权限解析结果</summary>
              <pre>{JSON.stringify(preview, null, 2)}</pre>
            </details>
          ) : null}
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
                { bindings: catalog.bindings, origins: catalog.origins },
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

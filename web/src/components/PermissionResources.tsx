import { useEffect, useState } from "react";

type Request = (
  method: string,
  params?: Record<string, unknown>,
) => Promise<any>;
type Profile = {
  schema_version: number;
  id: string;
  identity: string;
  name: string;
  version: number;
  target_kind: "harness" | "sandbox";
  policy: Record<string, unknown>;
};
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
  sources: string[];
  origins: unknown[];
  agents?: {
    id: string;
    name: string;
    harness_ref: string;
    sandbox_ref?: string;
    harness_mode: string;
    sandbox_mode: string;
  }[];
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
  const [catalog, setCatalog] = useState<Catalog>();
  const [selected, setSelected] = useState("");
  const [targetId, setTargetId] = useState("");
  const [draft, setDraft] = useState("");
  const [message, setMessage] = useState("");
  const [busy, setBusy] = useState(false);
  const [preview, setPreview] = useState<unknown>();
  const [source, setSource] = useState("");
  const [scope, setScope] = useState(
    '{"work_type":"discussion","writable_paths":[],"read_only_paths":[]}',
  );
  const [harnessId, setHarnessId] = useState("");
  const [sandboxId, setSandboxId] = useState("");
  useEffect(() => {
    let active = true;
    request("query.permission_resources", { project_id: projectId })
      .then((value: Catalog) => {
        if (active) setCatalog(value);
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
  async function reload() {
    setCatalog(
      await request("query.permission_resources", { project_id: projectId }),
    );
    await refresh();
  }
  async function action(work: () => Promise<void>) {
    setBusy(true);
    setMessage("");
    try {
      await work();
    } catch (e) {
      setMessage(e instanceof Error ? e.message : String(e));
    } finally {
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
    const fields = [
      "schema_version",
      "id",
      "identity",
      "name",
      "version",
      "supersedes",
      "target_kind",
      "policy",
    ];
    for (const key of Object.keys(value)) {
      if (!fields.includes(key)) throw new Error(`未知权限字段：${key}`);
    }
    if (value.schema_version !== undefined && value.schema_version !== 2)
      throw new Error("此编辑器使用 schema_version 2");
    return value;
  }
  function edit(p: Profile) {
    setSelected(p.id);
    setDraft(JSON.stringify(p, null, 2));
    setPreview(undefined);
  }
  function fresh() {
    const kind = target?.target.kind || "harness";
    setSelected("");
    setPreview(undefined);
    setDraft(
      JSON.stringify(
        {
          identity: `custom-${crypto.randomUUID()}`,
          name: "新权限",
          target_kind: kind,
          policy: {
            filesystem: { mode: "read_only" },
            network: { mode: "enabled" },
            ...(kind === "harness"
              ? { tools: { mode: "all_available" }, approval: "never" }
              : {}),
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
            <select
              aria-label="版本化权限修订"
              value={selected}
              onChange={(e) => {
                const p = catalog.profiles.find((p) => p.id === e.target.value);
                if (p) edit(p);
              }}
            >
              <option value="">选择权限</option>
              {catalog.profiles.map((p) => (
                <option key={p.id} value={p.id}>
                  {p.name} · {p.target_kind} · v{p.version}
                </option>
              ))}
            </select>
          </label>
          <button type="button" onClick={fresh}>
            新建权限草稿
          </button>
          <label>
            统一权限 JSON
            <textarea
              aria-label="统一权限 JSON"
              rows={14}
              value={draft}
              onChange={(e) => setDraft(e.target.value)}
            />
          </label>
          <div className="actions">
            <button
              type="button"
              disabled={!draft}
              onClick={() =>
                action(async () => {
                  const p = parseDraft();
                  const saved = await command("permission.profile.create", {
                    identity: p.identity,
                    name: p.name,
                    target_kind: p.target_kind,
                    policy: p.policy,
                  });
                  await reload();
                  edit(saved);
                  setMessage("已创建权限修订，尚未应用绑定。");
                })
              }
            >
              创建独立权限
            </button>
            <button
              type="button"
              disabled={!profile}
              onClick={() =>
                action(async () => {
                  const p = parseDraft();
                  const saved = await command(
                    "permission.profile.revise",
                    {
                      name: p.name,
                      target_kind: p.target_kind,
                      policy: p.policy,
                    },
                    {
                      target: profile!.id,
                      expected_revision: profile!.version,
                    },
                  );
                  await reload();
                  edit(saved);
                  setMessage("新修订已保存，原绑定保持不变。");
                })
              }
            >
              保存权限新修订
            </button>
            <button
              type="button"
              disabled={!profile}
              onClick={() =>
                action(async () => {
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
                })
              }
            >
              导出权限
            </button>
          </div>
          <label>
            导入权限文件
            <input
              aria-label="导入权限文件"
              type="file"
              accept="application/json,.json"
              onChange={(e) => {
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
            <select
              aria-label="权限目标版本"
              value={targetId}
              onChange={(e) => {
                setTargetId(e.target.value);
                setPreview(undefined);
              }}
            >
              <option value="">选择 Harness 或 Sandbox 版本</option>
              {catalog.targets.map((t) => (
                <option key={t.target.revision_id} value={t.target.revision_id}>
                  {t.name} · {t.target.kind} · v{t.version} · {t.provider}{" "}
                  {t.runtime_version}
                </option>
              ))}
            </select>
          </label>
          {target ? (
            <>
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
                      registered_installations: target.registered_installations,
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
            <button
              type="button"
              disabled={!profile || !target}
              onClick={() =>
                action(async () => {
                  setPreview(
                    await request("permission.binding.preflight", {
                      project_id: projectId,
                      target: target!.target,
                      permission_revision_id: profile!.id,
                    }),
                  );
                  setMessage("目标兼容性预检通过，仍需实际执行范围校验。");
                })
              }
            >
              预检目标绑定
            </button>
            <button
              type="button"
              disabled={!profile || !target || !target.supported}
              onClick={() =>
                action(async () => {
                  await command("permission.bindings.apply", {
                    bindings: [
                      {
                        target: target!.target,
                        permission_revision_id: profile!.id,
                        expected_revision: target!.active?.revision || 0,
                      },
                    ],
                  });
                  await reload();
                  setMessage(
                    "活动绑定已更新；已有工作保留冻结权限，追随目标的讨论将在下一轮切换。",
                  );
                })
              }
            >
              应用到该目标版本
            </button>
          </div>
        </fieldset>
        <details>
          <summary>实际执行范围预览</summary>
          <label>
            Harness 组合目标
            <select
              value={harnessId}
              onChange={(e) => setHarnessId(e.target.value)}
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
            </select>
          </label>
          <label>
            Sandbox 组合目标
            <select
              value={sandboxId}
              onChange={(e) => setSandboxId(e.target.value)}
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
            </select>
          </label>
          <label>
            ExecutionScope JSON
            <textarea
              value={scope}
              onChange={(e) => setScope(e.target.value)}
              rows={5}
            />
          </label>
          <button
            type="button"
            disabled={busy || !harnessId || !sandboxId}
            onClick={() =>
              action(async () => {
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
                setMessage("完整范围预览通过；正式执行仍由协调器核验并冻结。");
              })
            }
          >
            预览两层实际权限
          </button>
        </details>
        {preview !== undefined ? (
          <details open>
            <summary>权限解析结果</summary>
            <pre>{JSON.stringify(preview, null, 2)}</pre>
          </details>
        ) : null}
        {target ? (
          <p className="muted">
            关联 Agent：
            {(catalog.agents || [])
              .filter((a) =>
                target.target.kind === "harness"
                  ? a.harness_ref === targetId
                  : a.sandbox_ref === targetId,
              )
              .map(
                (a) =>
                  `${a.name}（${(target.target.kind === "harness" ? a.harness_mode : a.sandbox_mode) === "follow_target" ? "追随目标" : "固定原权限"}）`,
              )
              .join("、") || "当前没有 Agent 引用此目标"}
          </p>
        ) : null}
        <details>
          <summary>预定义来源与历史</summary>
          <ul>
            {catalog.sources.map((p) => (
              <li key={p}>{p}</li>
            ))}
          </ul>
          <button
            type="button"
            disabled={busy}
            onClick={() =>
              action(async () => {
                const result = await command("permission.profile.reload", {});
                await reload();
                setMessage(
                  `预设重新加载完成：新增 ${result.added.length}，相同 ${result.unchanged.length}；活动绑定保持不变。`,
                );
              })
            }
          >
            重新加载预设
          </button>
          <label>
            新增预设来源目录
            <input
              value={source}
              onChange={(e) => setSource(e.target.value)}
              placeholder="绝对目录"
            />
          </label>
          <button
            type="button"
            disabled={busy || !source}
            onClick={() =>
              action(async () => {
                await command("permission.source.register", { path: source });
                await reload();
                setSource("");
                setMessage("来源已登记，可重新加载预设。");
              })
            }
          >
            登记预设目录
          </button>
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
  );
}

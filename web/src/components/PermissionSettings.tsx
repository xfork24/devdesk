import {
  RolePermissionSettings,
  type PermissionRole,
} from "./RolePermissionSettings";
import { FormEvent, useEffect, useState } from "react";
import { PermissionResources } from "./PermissionResources";

type Binding = {
  permission?: { revision_id: string };
  overrides?: Record<string, unknown>;
};
type Definition = {
  id: string;
  identity: string;
  name: string;
  version: number;
  policy: Record<string, unknown>;
};
type Configuration = {
  revision: number;
  harnesses: Record<string, Binding>;
  sandbox: Binding;
  approval: "never" | "on_request";
};
type Result = {
  roles?: PermissionRole[];
  permissions: Definition[];
  configuration: Configuration;
  capabilities: unknown[];
};
type Project = { id: string; revision: number; permission_overrides?: unknown };
type Props = {
  request: (method: string, params?: Record<string, unknown>) => Promise<any>;
  project?: Project;
  refresh: () => Promise<void>;
};

export function PermissionSettings({ request, project, refresh }: Props) {
  const [data, setData] = useState<Result>();
  const [target, setTarget] = useState("codex");
  const [revision, setRevision] = useState("");
  const [patch, setPatch] = useState("{}");
  const [approval, setApproval] = useState<Configuration["approval"]>("never");
  const [projectPatch, setProjectPatch] = useState("{}");
  const [name, setName] = useState("");
  const [policy, setPolicy] = useState("{}");
  const [preview, setPreview] = useState<unknown>();
  const [busy, setBusy] = useState(false);
  const [message, setMessage] = useState("");
  useEffect(() => {
    let active = true;
    request("query.permissions")
      .then((value: Result) => {
        if (active) setData(value);
      })
      .catch((error: unknown) => {
        if (active) setMessage(String(error));
      });
    return () => {
      active = false;
    };
  }, [request]);
  const binding = data
    ? target === "docker"
      ? data.configuration.sandbox
      : data.configuration.harnesses[target]
    : undefined;
  const bindingJson = JSON.stringify(binding);
  useEffect(() => {
    const value: Binding = JSON.parse(bindingJson ?? "{}");
    setRevision(value.permission?.revision_id ?? "");
    setPatch(JSON.stringify(value.overrides ?? {}, null, 2));
    setPreview(undefined);
  }, [bindingJson, target]);
  useEffect(() => {
    if (data) setApproval(data.configuration.approval);
  }, [data?.configuration.approval]);
  const projectJson = JSON.stringify(project?.permission_overrides ?? {});
  useEffect(() => {
    setProjectPatch(JSON.stringify(JSON.parse(projectJson), null, 2));
  }, [project?.id, projectJson]);
  async function reload() {
    setData(await request("query.permissions"));
    await refresh();
  }
  function selectedBinding(): Binding {
    return {
      permission: { revision_id: revision },
      overrides: JSON.parse(patch),
    };
  }
  async function preflight() {
    const overrides =
      target === "docker"
        ? { sandbox: selectedBinding() }
        : { harness: selectedBinding() };
    return request("permission.preflight", {
      provider: target === "docker" ? "codex" : target,
      overrides,
      configuration: data ? { ...data.configuration, approval } : undefined,
    });
  }
  async function save(event: FormEvent) {
    event.preventDefault();
    if (!data) return;
    setBusy(true);
    setMessage("");
    try {
      const result = await preflight();
      setPreview(result);
      const next: Configuration =
        target === "docker"
          ? { ...data.configuration, sandbox: selectedBinding(), approval }
          : {
              ...data.configuration,
              harnesses: {
                ...data.configuration.harnesses,
                [target]: selectedBinding(),
              },
              approval,
            };
      await request("permission.bind", {
        command_id: crypto.randomUUID(),
        target: "permission-configuration",
        expected_revision: data.configuration.revision,
        payload: next,
      });
      await reload();
      setMessage("权限已保存，后续会话使用新配置；已有执行保留冻结权限。");
    } catch (error) {
      setMessage(String(error));
    } finally {
      setBusy(false);
    }
  }
  return (
    <>
      <PermissionResources
        request={request}
        refresh={refresh}
        projectId={project?.id}
      />
      <details>
        <summary>旧权限配置兼容入口</summary>
        <section className="setting-row">
          <div>
            <h3>运行权限 Permission</h3>
            <p className="muted">
              Harness 控制工具访问，Docker
              控制容器边界。容器内全权限允许工具读取该容器内的运行状态和凭据；最终访问还受工作范围和挂载限制。
            </p>
            {data ? (
              <>
                <form onSubmit={save}>
                  <label>
                    权限目标
                    <select
                      aria-label="权限目标"
                      value={target}
                      disabled={busy}
                      onChange={(e) => setTarget(e.target.value)}
                    >
                      <option value="codex">Codex Harness</option>
                      <option value="dsh">DSH Harness</option>
                      <option value="docker">Docker Sandbox</option>
                    </select>
                  </label>
                  <label>
                    权限修订
                    <select
                      aria-label="权限修订"
                      value={revision}
                      disabled={busy}
                      onChange={(e) => {
                        setRevision(e.target.value);
                        const selected = data.permissions.find(
                          (p) => p.id === e.target.value,
                        );
                        if (selected)
                          setPolicy(JSON.stringify(selected.policy, null, 2));
                      }}
                    >
                      {data.permissions.map((p) => (
                        <option key={p.id} value={p.id}>
                          {p.name} · v{p.version}
                        </option>
                      ))}
                    </select>
                  </label>
                  <label>
                    运行时审批
                    <select
                      aria-label="权限审批"
                      value={approval}
                      disabled={busy}
                      onChange={(e) =>
                        setApproval(e.target.value as Configuration["approval"])
                      }
                    >
                      <option value="never">无需询问</option>
                      <option value="on_request">按需询问</option>
                    </select>
                  </label>
                  <label>
                    字段覆盖 JSON
                    <textarea
                      aria-label="权限字段覆盖"
                      rows={5}
                      value={patch}
                      disabled={busy}
                      onChange={(e) => setPatch(e.target.value)}
                    />
                  </label>
                  <p className="muted">
                    省略字段表示继承，列表整体替换。例如{" "}
                    {`{"network":{"mode":"disabled"}}`}
                    。预检会说明当前后端能否执行。
                  </p>
                  <div className="actions">
                    <button
                      type="button"
                      disabled={busy}
                      onClick={async () => {
                        setBusy(true);
                        try {
                          setPreview(await preflight());
                          setMessage("预检通过，未启动模型或容器。");
                        } catch (error) {
                          setMessage(String(error));
                        } finally {
                          setBusy(false);
                        }
                      }}
                    >
                      预览最终权限
                    </button>
                    <button disabled={busy}>保存权限绑定</button>
                  </div>
                </form>
                {preview !== undefined && (
                  <details open>
                    <summary>解析结果与执行后端</summary>
                    <pre>{JSON.stringify(preview, null, 2)}</pre>
                  </details>
                )}
                {project && (
                  <details>
                    <summary>当前项目权限覆盖</summary>
                    <label>
                      项目覆盖 JSON
                      <textarea
                        aria-label="项目权限覆盖"
                        rows={6}
                        value={projectPatch}
                        onChange={(e) => setProjectPatch(e.target.value)}
                        disabled={busy}
                      />
                    </label>
                    <button
                      disabled={busy}
                      onClick={async () => {
                        setBusy(true);
                        try {
                          await request("project.update", {
                            command_id: crypto.randomUUID(),
                            target: project.id,
                            expected_revision: project.revision,
                            payload: {
                              permission_overrides: JSON.parse(projectPatch),
                            },
                          });
                          await reload();
                          setMessage("项目覆盖已保存，后续会话生效。");
                        } catch (error) {
                          setMessage(String(error));
                        } finally {
                          setBusy(false);
                        }
                      }}
                    >
                      保存项目权限
                    </button>
                  </details>
                )}
                <RolePermissionSettings
                  roles={data.roles ?? []}
                  request={request}
                  onSaved={reload}
                />
                <details>
                  <summary>创建或修订可复用权限</summary>
                  <label>
                    权限名称
                    <input
                      aria-label="新权限名称"
                      value={name}
                      onChange={(e) => setName(e.target.value)}
                      disabled={busy}
                    />
                  </label>
                  <label>
                    权限策略 JSON
                    <textarea
                      aria-label="新权限策略"
                      rows={8}
                      value={policy}
                      onChange={(e) => setPolicy(e.target.value)}
                      disabled={busy}
                    />
                  </label>
                  <div className="actions">
                    {[false, true].map((revise) => (
                      <button
                        key={String(revise)}
                        disabled={busy || !name.trim()}
                        onClick={async () => {
                          const old = data.permissions.find(
                            (p) => p.id === revision,
                          );
                          setBusy(true);
                          try {
                            await request(
                              revise
                                ? "permission.revise"
                                : "permission.create",
                              {
                                command_id: crypto.randomUUID(),
                                ...(revise && old
                                  ? {
                                      target: old.id,
                                      expected_revision: old.version,
                                    }
                                  : {}),
                                payload: {
                                  identity: `custom-${crypto.randomUUID()}`,
                                  name,
                                  policy: JSON.parse(policy),
                                },
                              },
                            );
                            await reload();
                            setMessage("已保存不可变权限修订。");
                          } catch (error) {
                            setMessage(String(error));
                          } finally {
                            setBusy(false);
                          }
                        }}
                      >
                        {revise ? "创建所选权限的新修订" : "创建权限"}
                      </button>
                    ))}
                  </div>
                </details>
                <details>
                  <summary>后端能力与限制</summary>
                  <pre>{JSON.stringify(data.capabilities, null, 2)}</pre>
                </details>
              </>
            ) : (
              <p>正在读取权限配置…</p>
            )}
            <p role="status">{message}</p>
          </div>
        </section>
      </details>
    </>
  );
}

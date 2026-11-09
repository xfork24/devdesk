import { useCallback, useEffect, useState } from "react";
type Row = Record<string, any>;
const duties: Record<string, string> = {
  analyst: "需求分析",
  architect: "架构设计",
  developer: "开发实现",
  tester: "测试验证",
  reviewer: "独立评审",
};
export function RoleManager({
  request,
  project,
  onChanged,
}: {
  request: (method: string, params?: Row) => Promise<any>;
  project?: Row;
  onChanged?: () => void;
}) {
  const [rows, setRows] = useState<Row[]>([]),
    [old, setOld] = useState<Row>(),
    [name, setName] = useState(""),
    [instructions, setInstructions] = useState(""),
    [duty, setDuty] = useState("analyst"),
    [scope, setScope] = useState("global"),
    [error, setError] = useState(""),
    [busy, setBusy] = useState(false),
    [message, setMessage] = useState("");
  const load = useCallback(async () => {
    const result = await request("query.roles", {
      local: !!project,
      project_id: project?.id,
      limit: 100,
    });
    setRows(result.records || []);
  }, [request, project?.id]);
  useEffect(() => {
    void load().catch((error) => setError(String(error)));
  }, [load]);
  const reset = () => {
    setOld(undefined);
    setName("");
    setInstructions("");
    setDuty("analyst");
    setScope("global");
    setMessage("");
  };
  const mutate = async (method: string, row: Row | undefined, payload: Row) => {
    setBusy(true);
    setError("");
    try {
      await request(method, {
        project_id: project?.id,
        command_id: crypto.randomUUID(),
        target: row?.id,
        expected_revision: row?.revision,
        payload,
      });
      reset();
      setMessage(
        method === "role.delete"
          ? "角色已删除；历史版本、AgentID 和 Feature 配置保留。"
          : "Role 已保存，并生成默认 Sol High / Codex Agent 与实例。已有 Feature 保持原版本。",
      );
      await load();
      onChanged?.();
    } catch (error) {
      setError(String(error));
    } finally {
      setBusy(false);
    }
  };
  return (
    <section className="panel role-manager" aria-label="角色管理">
      <div className="panel-heading">
        <div>
          <h2>角色 Role</h2>
          <p>
            Role 定义职责；Agent 绑定 Role 修订、Model、Harness
            和能力。相同执行职责可以有多个自定义角色。
          </p>
        </div>
        <button onClick={reset}>新增角色</button>
      </div>
      <div className="workflow-list">
        {rows.map((row) => (
          <article key={row.id}>
            <button
              className={old?.id === row.id ? "selected" : undefined}
              onClick={() => {
                setOld(row);
                setName(row.name);
                setInstructions(row.data.instructions);
                setDuty(row.data.execution_role);
                setScope(row.scope.kind);
                setMessage("");
              }}
            >
              <strong>{row.name}</strong>
              <span>
                {duties[row.data.execution_role]} · v{row.version} ·{" "}
                {row.scope.kind}
              </span>
            </button>
            <button
              disabled={busy}
              onClick={() => void mutate("role.delete", row, {})}
            >
              删除 {row.name}
            </button>
          </article>
        ))}
      </div>
      <form
        onSubmit={(event) => {
          event.preventDefault();
          void mutate(old ? "role.revise" : "role.create", old, {
            name,
            scope:
              scope === "project"
                ? { kind: "project", project_id: project!.id }
                : { kind: "global" },
            data: { execution_role: duty, instructions },
          });
        }}
      >
        <h3>{old ? `编辑 ${old.name} · 新修订` : "新建角色"}</h3>
        <div className="form-grid">
          <label>
            Role 名称
            <input
              required
              value={name}
              onChange={(e) => setName(e.target.value)}
            />
          </label>
          <label>
            执行职责
            <select
              value={duty}
              disabled={old?.definition_id.startsWith("builtin-role-")}
              onChange={(e) => setDuty(e.target.value)}
            >
              {Object.entries(duties).map(([key, label]) => (
                <option key={key} value={key}>
                  {label}
                </option>
              ))}
            </select>
          </label>
          <label>
            Role Scope
            <select
              value={scope}
              disabled={!!old}
              onChange={(e) => setScope(e.target.value)}
            >
              <option value="global">Global</option>
              {project && <option value="project">当前 Project</option>}
            </select>
          </label>
          <label className="role-instructions">
            Role 职责说明
            <textarea
              required
              value={instructions}
              onChange={(e) => setInstructions(e.target.value)}
            />
          </label>
        </div>
        <p>
          每个新角色自动获得 GPT-6.1 Sol High、Codex
          固定配方、Skills、Tools、Permission 和默认
          AgentID。编辑内建默认角色会生成新套件版本；默认流程仍需保留五种工程执行职责。
        </p>
        <button disabled={busy} type="submit">
          {old ? "保存 Role 新版本" : "创建 Role"}
        </button>
      </form>
      {error && (
        <p className="error" role="alert">
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

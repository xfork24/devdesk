import { useCallback, useEffect, useState } from "react";
import {
  latest,
  RevisionSelect,
  type Request,
  type Row,
} from "./configuration-ui";
export function ProjectIntegrationBindings({
  request,
  project,
  catalog,
  agents,
}: {
  request: Request;
  project: Row;
  catalog: Row[];
  agents: string[];
}) {
  const [rows, setRows] = useState<Row[]>([]),
    [mapping, setMapping] = useState<Record<string, string>>({}),
    [source, setSource] = useState(""),
    [error, setError] = useState(""),
    [message, setMessage] = useState(""),
    [busy, setBusy] = useState(false);
  const load = useCallback(async () => {
    const result: Row[] = [];
    for (let offset = 0; ; offset += 100) {
      const page = await request("query.integration_bindings", {
        local: true,
        project_id: project.id,
        include_history: true,
        offset,
        limit: 100,
      });
      result.push(...page.records);
      if (offset + page.records.length >= page.total || !page.records.length)
        break;
    }
    const current = latest(
      result.filter(
        (r) =>
          r.kind === "integration_project_configuration" &&
          r.definition_id === project.id,
      ),
    )[0];
    setRows(result);
    setMapping(current?.data || {});
    setSource(current?.id || "");
  }, [request, project.id]);
  useEffect(() => {
    void load().catch((e) => setError(String(e)));
  }, [load]);
  const required = new Set<string>();
  for (const id of agents) {
    const agent = catalog.find((r) => r.id === id);
    for (const tool of agent?.data.tools || []) {
      const row = catalog.find((r) => r.id === tool);
      if (row?.data.schema_version === 2)
        required.add(row.data.integration_ref);
    }
  }
  return (
    <section className="panel" aria-label="项目工具接入绑定">
      <div className="panel-heading">
        <h2>项目接入账户</h2>
        <p>按所选 Agent 的具体工具选择账户、执行环境和授权修订。</p>
      </div>
      <div className="integration-content">
        {!required.size && <p>当前 Agent 使用原生工具，无需外部接入绑定。</p>}
        {[...required].map((id) => (
          <RevisionSelect
            key={id}
            label={`接入 ${catalog.find((r) => r.id === id)?.name || id} 的绑定`}
            records={rows.filter(
              (r) =>
                r.kind === "integration_binding" &&
                r.data.integration_ref === id,
            )}
            value={mapping[id] || ""}
            onChange={(value) => setMapping({ ...mapping, [id]: value })}
            required
          />
        ))}
        <div className="actions">
          <button
            disabled={
              busy ||
              ![...required].every((id) => mapping[id]) ||
              !required.size
            }
            onClick={async () => {
              setBusy(true);
              setError("");
              try {
                await request("integration.project.bind", {
                  project_id: project.id,
                  command_id: crypto.randomUUID(),
                  payload: {
                    project_id: project.id,
                    source_configuration_id: source || null,
                    bindings: mapping,
                  },
                });
                await load();
                setMessage(
                  "项目接入绑定已保存；已有 Feature 保留冻结版本，讨论在静止边界采用新绑定。",
                );
              } catch (e) {
                setError(String(e));
              } finally {
                setBusy(false);
              }
            }}
          >
            保存项目接入绑定
          </button>
          <a
            href={`?tab=integrations&project_id=${encodeURIComponent(project.id)}`}
          >
            管理工具、账户与授权
          </a>
        </div>
        {message && <p role="status">{message}</p>}
        {error && <p role="alert">{error}</p>}
      </div>
    </section>
  );
}

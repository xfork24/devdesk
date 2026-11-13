import { useEffect, useState } from "react";
import {
  ActionButton,
  ActionForm,
  ActionSelect,
  useActionFeedback,
} from "./ActionFeedback";
import {
  latest,
  loadCatalog,
  RevisionSelect,
  type Request,
  type Row,
} from "./configuration-ui";

export function AgentInstances({
  request,
  project,
}: {
  request: Request;
  project?: Row;
}) {
  const [records, setRecords] = useState<Row[]>([]);
  const [agent, setAgent] = useState("");
  const [name, setName] = useState("");
  const [scope, setScope] = useState(project ? "project" : "global");
  const [busy, setBusy] = useState(false);
  const [revision, setRevision] = useState(0);
  const { error, setError, message, setMessage, beginFeedback, endFeedback } =
    useActionFeedback();
  useEffect(() => {
    let current = true;
    loadCatalog(request, project)
      .then((rows) => {
        if (current) setRecords(rows);
      })
      .catch((error) => {
        if (current) setError(String(error));
      });
    return () => {
      current = false;
    };
  }, [request, project?.id, revision]);
  const definitions = latest(
    records.filter(
      (r) =>
        r.type === "agent" && (scope !== "global" || r.scope === "global"),
    ),
  );
  return (
    <section className="panel">
      <h3>创建 Agent</h3>
      <p>
        选择已有 Agent 定义的确切修订创建 Agent。每个 Agent
        独立保留会话和工作经验。
      </p>
      <ActionForm
        onSubmit={async (event) => {
          event.preventDefault();
          setBusy(true);
          beginFeedback();
          setError("");
          setMessage("");
          try {
            await request("agent.instance.create", {
              command_id: crypto.randomUUID(),
              project_id: project?.id,
              payload: {
                name: name.trim(),
                agent_revision_id: agent,
                scope:
                  scope === "global"
                    ? "global"
                    : `project:${project!.id}`,
              },
            });
            setName("");
            setRevision((value) => value + 1);
            setMessage("已创建 Agent，可在角色讨论中选择。");
          } catch (error) {
            setError(String(error));
          } finally {
            setBusy(false);
            endFeedback();
          }
        }}
      >
        <label>
          Agent 名称
          <input
            required
            value={name}
            onChange={(event) => setName(event.target.value)}
          />
        </label>
        <label>
          Agent 范围
          <ActionSelect
            value={scope}
            onChange={(event) => {
              setScope(event.target.value);
              setAgent("");
            }}
          >
            <option value="global">全局</option>
            {project && <option value="project">当前项目</option>}
          </ActionSelect>
        </label>
        <RevisionSelect
          label="Agent 定义修订"
          records={definitions}
          value={agent}
          onChange={(value) => setAgent(String(value))}
        />
        <ActionButton
          disabled={
            busy || !name.trim() || !definitions.some((r) => r.id === agent)
          }
        >
          创建 Agent
        </ActionButton>
      </ActionForm>
      {message && <p role="status">{message}</p>}
      {error && <p role="alert">{error}</p>}
      <h3>已有 Agent</h3>
      {records
        .filter((r) => r.type === "agent_instance")
        .map((instance) => (
          <article key={instance.id} className="document-row">
            <div>
              <strong>{instance.name}</strong>
              <p>
                {instance.scope === "global" ? "全局" : "当前项目"} ·{" "}
                {records.find((r) => r.id === instance.data.agent_revision_id)
                  ?.name || instance.data.agent_revision_id}
              </p>
              <code>{instance.id}</code>
            </div>
          </article>
        ))}
    </section>
  );
}

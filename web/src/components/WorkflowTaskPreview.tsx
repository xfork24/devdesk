import { useState } from "react";
import {
  ActionButton,
  ActionSelect,
  useActionFeedback,
} from "./ActionFeedback";
import { MarkdownField, type Request, type Row } from "./configuration-ui";

export function AgentTaskView({ value }: { value: Row }) {
  // The backend only returns validated task contracts; no renderer-version filtering here.
  const content =
    typeof value.task === "object" ? value.task : value;
  const instructions =
    content.task_instructions ||
    (typeof value.task === "string" ? value.task : "");
  if (!instructions)
    return <p className="muted">此讨论没有 Workflow 步骤任务。</p>;
  return (
    <section aria-label="Agent 任务与上下文">
      {content.role?.role?.data?.instructions ? (
        <MarkdownField
          label={`Role 职责 · ${content.role.name}`}
          value={content.role.role.data.instructions}
          onChange={() => {}}
          readOnly
        />
      ) : null}
      <MarkdownField
        label="任务指令"
        value={instructions}
        onChange={() => {}}
        readOnly
        rows={8}
      />
      {content.stage?.description ? (
        <MarkdownField
          label="阶段背景"
          value={content.stage.description}
          onChange={() => {}}
          readOnly
        />
      ) : null}
      {(content.required_skills || []).map((skill: Row) => (
        <MarkdownField
          key={skill.id}
          label={`Skill · ${skill.name}`}
          value={skill.data?.body || skill.body || ""}
          onChange={() => {}}
          readOnly
        />
      ))}
      {[
        ...(content.specifications || []),
        ...(content.document_specifications || []),
      ].map((spec: Row) => (
        <MarkdownField
          key={spec.id}
          label={`Spec · ${spec.name}`}
          value={spec.data?.body || spec.body || ""}
          onChange={() => {}}
          readOnly
        />
      ))}
      {(value.diagnostics || []).map((line: string) => (
        <p key={line} className="muted">
          {line}
        </p>
      ))}
      <details>
        <summary>来源、材料与输出要求</summary>
        <pre>
          {JSON.stringify(
            {
              source: content.source || value.source,
              role_ref: value.role_ref,
              inputs: content.inputs || value.inputs,
              input_sources: content.input_sources,
              runtime_context: content.runtime_context,
              outputs: content.outputs || value.outputs,
              completion: content.completion || value.completion,
              authorization: content.authorization || value.access,
            },
            null,
            2,
          )}
        </pre>
      </details>
      {typeof value.task === "object" && value.task?.rendered_input ? (
        <details>
          <summary>实际发送的新输入</summary>
          <pre>{value.task.rendered_input}</pre>
        </details>
      ) : null}
    </section>
  );
}

export function WorkflowTaskPreview({
  request,
  document,
  step,
  records,
  project,
  workflowId,
}: {
  request: Request;
  document: Row;
  step: Row;
  records: Row[];
  project?: Row;
  workflowId?: string;
}) {
  const { error, setError, capture, beginFeedback, endFeedback } =
    useActionFeedback();
  const [agent, setAgent] = useState(""),
    [value, setValue] = useState<Row>(),
    [busy, setBusy] = useState(false),
    [previewSignature, setPreviewSignature] = useState("");
  const role =
    step.executor.role_ref ||
    document.stages.find((stage: Row) => stage.key === step.stage)
      ?.default_role_ref;
  const agents = records.filter(
    (row) => row.type === "agent" && row.role_ref === role,
  );
  const selectedAgent = agents.some((row) => row.id === agent) ? agent : "";
  const signature = JSON.stringify({
    document,
    step: step.key,
    agent: selectedAgent,
  });
  async function preview() {
    capture();
    beginFeedback();
    setBusy(true);
    setError("");
    try {
      const result = await request("query.workflow_task_preview", {
        id: workflowId || undefined,
        document,
        step: step.key,
        project_id: project?.id,
        preview_agent_ref: selectedAgent || undefined,
      });
      setValue(result);
      setPreviewSignature(signature);
    } catch (error) {
      setError(String(error));
    } finally {
      setBusy(false);
      endFeedback();
    }
  }
  return (
    <details>
      <summary>预览任务与执行上下文</summary>
      <label>
        预览 Agent（可选）
        <ActionSelect
          value={selectedAgent}
          onChange={(event) => {
            setAgent(event.target.value);
            setValue(undefined);
          }}
        >
          <option value="">尚未绑定</option>
          {agents.map((agent) => (
            <option key={agent.id} value={agent.id}>
              {agent.name} · v{agent.version}
            </option>
          ))}
        </ActionSelect>
      </label>
      <ActionButton
        type="button"
        disabled={busy}
        onClick={() => void preview()}
      >
        预览当前任务
      </ActionButton>
      {value && previewSignature === signature ? (
        <AgentTaskView value={value} />
      ) : value ? (
        <p className="muted">配置已变化，请重新预览。</p>
      ) : null}
      {error ? (
        <p role="alert" className="error">
          {error}
        </p>
      ) : null}
    </details>
  );
}

export function ExecutionTaskDetails({
  request,
  projectId,
  turns,
}: {
  request: Request;
  projectId: string;
  turns: Row[];
}) {
  const {
    error,
    setError,
    capture: captureFeedback,
    beginFeedback,
    endFeedback,
  } = useActionFeedback();

  const [selected, setSelected] = useState("");
  const [loadedTurn, setLoadedTurn] = useState("");
  const [busy, setBusy] = useState(false);
  const [value, setValue] = useState<Row>();

  return (
    <details>
      <summary>查看实际任务指令与上下文</summary>
      <label>
        任务轮次
        <ActionSelect
          aria-label="任务轮次"
          value={selected}
          onChange={(e) => {
            captureFeedback();

            setSelected(e.target.value);
            setValue(undefined);
          }}
        >
          <option value="">选择轮次</option>
          {turns.map((t) => (
            <option key={t.id} value={t.id}>
              {t.id} · {t.status}
            </option>
          ))}
        </ActionSelect>
      </label>
      <ActionButton
        type="button"
        disabled={!selected || busy}
        onClick={async () => {
          captureFeedback();

          (beginFeedback(), setBusy(true));
          setLoadedTurn(selected);
          setError("");
          try {
            setValue(
              await request("query.execution_task", {
                project_id: projectId,
                turn_id: selected,
              }),
            );
          } catch (e) {
            setError(String(e));
          } finally {
            (endFeedback(), setBusy(false));
          }
        }}
      >
        读取冻结任务
      </ActionButton>
      {error ? <p role="alert">{error}</p> : null}
      {value && loadedTurn === selected ? (
        <AgentTaskView value={value} />
      ) : null}
    </details>
  );
}

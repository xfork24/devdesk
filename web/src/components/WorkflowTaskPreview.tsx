import { useState } from "react";
import {
  ActionButton,
  ActionSelect,
  useActionFeedback,
} from "./ActionFeedback";
import { MarkdownPreview, type Request, type Row } from "./configuration-ui";
import { RawJsonLink, ReadableValue, ReadingLink } from "./reading";

export function AgentTaskView({ value, compact = false, projectId, ownerId }: { value: Row; compact?: boolean; projectId?: string; ownerId?: string }) {
  let content = value;
  for (let level = 0; level < 4 && content.task && typeof content.task === "object" && !content.task_instructions; level++) content = content.task;
  const frozen = value.task?.rendered_input ? value.task : value;
  const instructions = content.task_instructions || (typeof value.task === "string" ? value.task : "");
  if (!instructions) return <p className="muted">此轮没有 Workflow 步骤冻结任务。</p>;
  const inputSources = content.input_sources || {};
  return <section aria-label="Agent 任务与上下文" className="reading-body">
    <h4>任务请求</h4><MarkdownPreview text={compact ? instructions.slice(0, 600) : instructions}/>
    {compact && instructions.length > 600 && <p className="muted">此处为任务摘要；完整指令在上下文页阅读。</p>}
    {compact && projectId && ownerId && value.turn_id && <ReadingLink detail="context" projectId={projectId} ownerId={ownerId} target={{turn_id:value.turn_id}}>查看此轮完整上下文</ReadingLink>}
    {!compact && <>
      {content.stage?.description && <><h4>工作背景</h4><MarkdownPreview text={content.stage.description}/></>}
      <h4>输入材料</h4>
      {Object.keys(inputSources).length ? Object.entries(inputSources).map(([key, source]) => <section key={key}><strong>{key}</strong><ReadableValue value={source}/></section>) : <p className="muted">未记录独立的输入来源索引。</p>}
      <details><summary>冻结输入内容</summary><ReadableValue value={content.inputs || value.inputs}/></details>
      <h4>输出要求</h4><ReadableValue value={content.outputs || value.outputs}/>
      <details><summary>职责、方法与规范</summary>
        {content.role?.role?.data?.instructions && <><h4>Role · {content.role.name}</h4><MarkdownPreview text={content.role.role.data.instructions}/></>}
        {(content.required_skills || []).map((skill: Row) => <section key={skill.id}><h4>Skill · {skill.name}</h4><MarkdownPreview text={skill.data?.body || skill.body || ""}/></section>)}
        {[...(content.specifications || []), ...(content.document_specifications || [])].map((spec: Row) => <section key={spec.id}><h4>Spec · {spec.name}</h4><MarkdownPreview text={spec.data?.body || spec.body || ""}/></section>)}
      </details>
      <details><summary>完成条件与授权依据</summary><ReadableValue value={{completion:content.completion || value.completion, authorization:content.authorization || value.access}}/></details>
      {frozen.rendered_input && <details><summary>实际发送的完整输入</summary><pre>{frozen.rendered_input}</pre></details>}
      <details><summary>冻结来源与运行环境</summary><ReadableValue value={{source:content.source || value.source, runtime_context:content.runtime_context}}/></details>
    </>}
    {(value.diagnostics || []).map((line: string) => <p key={line} className="muted">{line}</p>)}
    {projectId && value.turn_id && <RawJsonLink source="task" params={{project_id:projectId,turn_id:value.turn_id}}/>}
  </section>;
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
        <AgentTaskView value={value} compact projectId={projectId} ownerId={value.task?.source?.owner_id || ""} />
      ) : null}
    </details>
  );
}

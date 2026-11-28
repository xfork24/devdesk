import { displayText, truncateText } from "../sdk/display-text";
import { useState } from "react";
import { ActionButton, ActionSelect } from "../sdk/ActionFeedback";
import { useActionFeedback } from "../sdk/action-feedback";
import { MarkdownPreview } from "./configuration-ui";
import { materialTitle } from "./workflow-material";
import { type Request, type Row } from "../sdk/rpc-types";
import {
  LazyDetails,
  RawJsonLink,
  ReadableValue,
  ReadingLink,
} from "./reading";

export function AgentTaskView({
  value,
  compact = false,
  projectId,
  ownerId,
}: {
  value: Row;
  compact?: boolean;
  projectId?: string;
  ownerId?: string;
}) {
  let content = value;
  for (
    let level = 0;
    level < 4 &&
    content.task &&
    typeof content.task === "object" &&
    !content.task_instructions;
    level++
  )
    content = content.task;
  const frozen = value.task?.rendered_input ? value.task : value;
  const instructions =
    content.task_instructions ||
    (typeof value.task === "string" ? value.task : "");
  if (!instructions)
    return <p className="muted">此轮没有 Workflow 步骤冻结任务。</p>;
  const inputSources = content.input_sources || {};
  const requestTitle =
    value.current_feature?.id &&
    value.current_feature.id === content.user_request?.feature_id
      ? value.current_feature.title
      : content.user_request?.title;
  return (
    <section aria-label="Agent 任务与上下文" className="reading-body">
      <h4>任务请求</h4>
      <MarkdownPreview
        text={compact ? truncateText(instructions, 600) : instructions}
      />
      {compact && instructions.length > 600 && (
        <p className="muted">此处为任务摘要；完整指令在上下文页阅读。</p>
      )}
      {compact && projectId && ownerId && value.turn_id && (
        <ReadingLink
          detail="context"
          projectId={projectId}
          ownerId={ownerId}
          target={{ turn_id: value.turn_id }}
        >
          查看此轮完整上下文
        </ReadingLink>
      )}
      {!compact && (
        <>
          {content.user_request && (
            <>
              <h4>用户初始需求</h4>
              <p>{displayText(requestTitle)}</p>
              <MarkdownPreview
                text={
                  content.user_request.initial_requirement ||
                  "尚未提供初始需求，需要向用户澄清。"
                }
              />
            </>
          )}
          {content.supplemental_input && (
            <>
              <h4>补充请求</h4>
              <MarkdownPreview text={content.supplemental_input} />
            </>
          )}
          {content.stage?.description && (
            <>
              <h4>工作背景</h4>
              <MarkdownPreview text={content.stage.description} />
            </>
          )}
          <h4>输入材料</h4>
          {Object.keys(inputSources).length ? (
            Object.entries(inputSources).map(([key, source]) => {
              const origin = source as Row;
              const name = materialTitle(
                value.input_material_names?.[origin.source_output_id],
                origin.binding?.source?.step,
                key,
              );
              return (
                <section key={key}>
                  {origin.source_output_id && projectId && ownerId ? (
                    <p>
                      <ReadingLink
                        detail="documents"
                        projectId={projectId}
                        ownerId={ownerId}
                        target={{ output_id: origin.source_output_id }}
                      >
                        {displayText(name)}
                      </ReadingLink>
                    </p>
                  ) : (
                    <p>
                      <strong>{displayText(name)}</strong>
                      {" · "}
                      {displayText(
                        origin.binding?.type === "constant"
                          ? "冻结常量"
                          : "工作范围输入",
                      )}
                    </p>
                  )}
                  {!origin.source_output_id && (
                    <ReadableValue value={content.inputs?.[key]} />
                  )}
                  <LazyDetails summary="该输入的来源依据">
                    <ReadableValue value={origin} />
                  </LazyDetails>
                </section>
              );
            })
          ) : (
            <p className="muted">未记录独立的输入来源索引。</p>
          )}
          <LazyDetails summary="冻结输入内容">
            <ReadableValue value={content.inputs || value.inputs} />
          </LazyDetails>
          <h4>输出要求</h4>
          {Object.entries(content.outputs || value.outputs || {}).map(
            ([port, requirement]) => {
              const r = requirement as Row;
              return (
                <section key={port}>
                  <strong>
                    {displayText(materialTitle(r.definition?.name, null, port))}
                  </strong>
                  <p>
                    {displayText(r.required ? "必需" : "可选")}
                    {displayText(r.content_checks ? " · 需要内容检查" : "")}
                  </p>
                  <LazyDetails summary="交付规范与检查要求">
                    <ReadableValue value={r} />
                  </LazyDetails>
                </section>
              );
            },
          )}
          <LazyDetails summary="职责、方法与规范">
            {content.role?.role?.data?.instructions && (
              <section
                role="group"
                aria-label={displayText(`Role 职责 · ${content.role.name}`)}
              >
                <h4>Role · {displayText(content.role.name)}</h4>
                <MarkdownPreview text={content.role.role.data.instructions} />
              </section>
            )}
            {(content.required_skills || []).map((skill: Row) => (
              <section
                key={skill.id}
                role="group"
                aria-label={displayText(`Skill · ${skill.name}`)}
              >
                <h4>Skill · {displayText(skill.name)}</h4>
                <MarkdownPreview text={skill.data?.body || skill.body || ""} />
              </section>
            ))}
            {[
              ...(content.specifications || []),
              ...(content.document_specifications || []),
            ].map((spec: Row) => (
              <section
                key={spec.id}
                role="group"
                aria-label={displayText(`Spec · ${spec.name}`)}
              >
                <h4>Spec · {displayText(spec.name)}</h4>
                <MarkdownPreview text={spec.data?.body || spec.body || ""} />
              </section>
            ))}
          </LazyDetails>
          <LazyDetails summary="完成条件与授权依据">
            <ReadableValue
              value={{
                completion: content.completion || value.completion,
                authorization: content.authorization || value.access,
              }}
            />
          </LazyDetails>
          {frozen.rendered_input && (
            <LazyDetails summary="实际发送的完整输入">
              <pre>{displayText(frozen.rendered_input)}</pre>
            </LazyDetails>
          )}
          <LazyDetails summary="冻结来源与运行环境">
            <ReadableValue
              value={{
                source: content.source || value.source,
                runtime_context: content.runtime_context,
              }}
            />
          </LazyDetails>
        </>
      )}
      {(value.diagnostics || []).map((line: string) => (
        <p key={line} className="muted">
          {displayText(line)}
        </p>
      ))}
      {projectId && value.turn_id && (
        <RawJsonLink
          source="task"
          title="查看冻结任务 JSON"
          params={{ project_id: projectId, turn_id: value.turn_id }}
        />
      )}
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
              {displayText(agent.name)} · v{displayText(agent.version)}
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
          {displayText(error)}
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
              {displayText(t.id)} · {displayText(t.status)}
            </option>
          ))}
        </ActionSelect>
      </label>
      <ActionButton
        type="button"
        disabled={!selected || busy}
        onClick={async () => {
          captureFeedback();

          beginFeedback();
          setBusy(true);
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
            endFeedback();
            setBusy(false);
          }
        }}
      >
        读取冻结任务
      </ActionButton>
      {error ? <p role="alert">{displayText(error)}</p> : null}
      {value && loadedTurn === selected ? (
        <AgentTaskView
          value={value}
          compact
          projectId={projectId}
          ownerId={value.task?.task?.owner_id || value.task?.owner_id || ""}
        />
      ) : null}
    </details>
  );
}

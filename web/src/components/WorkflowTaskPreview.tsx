import {
  ActionButton,
  ActionForm,
  ActionSelect,
  useActionFeedback,
} from "./ActionFeedback";
import { useState } from "react";
import { MarkdownField, type Request, type Row } from "./configuration-ui";

export function AgentTaskView({ value }: { value: Row }) {
  const frozen = value.task?.content ? value.task : undefined;
  const content = frozen?.content || value;
  const base = frozen?.base || value.base_context;
  const task = content.task;
  if (!task?.instructions)
    return (
      <p className="muted">
        此记录使用旧版或独立讨论合同，没有 v3 Agent 任务快照。
      </p>
    );
  const text = (label: string, body: string, id: string) => (
    <MarkdownField
      key={id}
      label={label}
      value={body}
      onChange={() => {}}
      readOnly
      rows={6}
    />
  );
  return (
    <section aria-label="Agent 任务与上下文">
      {text("公共任务指令", task.instructions, "common")}
      {task.node_instructions
        ? text("节点任务指令", task.node_instructions, "node")
        : null}
      {content.stage_context?.description
        ? text("阶段背景", content.stage_context.description, "stage")
        : null}
      {base?.agent
        ? text(
            `Role 与 Agent 有效指令 · ${base.agent.name}`,
            base.agent.body,
            base.agent.id,
          )
        : null}
      {(base?.skills || []).map((skill: Row) =>
        text(`Skill · ${skill.name}`, skill.body, skill.id),
      )}
      {(content.specifications || []).map((spec: Row) =>
        text(`Spec · ${spec.name}`, spec.body, spec.id),
      )}
      {(content.delivery_types || []).map((delivery: Row) =>
        text(`Delivery · ${delivery.name}`, delivery.data.body, delivery.id),
      )}
      {(value.diagnostics || []).map((line: string) => (
        <p className="muted" key={line}>
          {line}
        </p>
      ))}
      <details>
        <summary>任务来源与输出合同</summary>
        <pre>
          {JSON.stringify(
            {
              source: content.source,
              definition: frozen?.definition || value.definition,
              digest: frozen?.digest || value.definition_digest,
              output_contract: content.output_contract,
              inputs: content.inputs,
              materials: content.materials,
            },
            null,
            2,
          )}
        </pre>
      </details>
      {frozen ? (
        <details>
          <summary>实际发送的新输入</summary>
          <pre>{frozen.rendered_input}</pre>
          <p className="muted">
            这里记录本轮新增输入；原生会话已有的历史上下文另由会话履历保存。
          </p>
        </details>
      ) : null}
    </section>
  );
}

export function WorkflowTaskPreview({
  request,
  projectId,
  document,
  step,
  agents,
  roles,
  nodeKeys,
}: {
  request: Request;
  projectId?: string;
  document: Row;
  step: Row;
  agents: Row[];
  roles: Row[];
  nodeKeys: string[];
}) {
  const {
    error,
    setError,
    capture: captureFeedback,
    beginFeedback,
    endFeedback,
  } = useActionFeedback();

  const [node, setNode] = useState(nodeKeys[0] || "");
  const [agent, setAgent] = useState("");
  const [value, setValue] = useState<Row>();

  const [busy, setBusy] = useState(false);
  const [previewSignature, setPreviewSignature] = useState("");
  const selectedNode = nodeKeys.includes(node) ? node : nodeKeys[0] || "";
  const role =
    step.execution_recipe?.find((n: Row) => n.key === selectedNode)?.role_ref ||
    step.role_ref;
  const roleIdentity = (ref: string) =>
    roles.find((r) => r.id === ref || r.definition_id === ref)?.definition_id ||
    ref;
  const selectedAgent = agents.some(
    (a) =>
      a.id === agent && roleIdentity(a.data.role_ref) === roleIdentity(role),
  )
    ? agent
    : "";
  const signature = JSON.stringify({
    document,
    node: selectedNode,
    agent: selectedAgent,
  });
  return (
    <details>
      <summary>预览任务与执行上下文</summary>
      {nodeKeys.length ? (
        <label>
          预览配方节点
          <ActionSelect
            aria-label="预览配方节点"
            value={selectedNode}
            onChange={(e) => {
              captureFeedback();

              setNode(e.target.value);
              setValue(undefined);
            }}
          >
            {nodeKeys.map((key) => (
              <option key={key}>{key}</option>
            ))}
          </ActionSelect>
        </label>
      ) : null}
      <label>
        预览 Agent（可选）
        <ActionSelect
          aria-label="预览 Agent"
          value={selectedAgent}
          onChange={(e) => {
            captureFeedback();

            setAgent(e.target.value);
            setValue(undefined);
          }}
        >
          <option value="">使用项目绑定；无绑定时显示缺口</option>
          {agents
            .filter((a) => roleIdentity(a.data.role_ref) === roleIdentity(role))
            .map((a) => (
              <option key={a.id} value={a.id}>
                {a.name} · v{a.version}
              </option>
            ))}
        </ActionSelect>
      </label>
      <ActionButton
        type="button"
        disabled={busy}
        onClick={async () => {
          captureFeedback();

          (beginFeedback(), setBusy(true));
          setError("");
          setPreviewSignature(signature);
          try {
            setValue(
              await request("query.workflow_task_preview", {
                project_id: projectId,
                document,
                path: step.key,
                ...(selectedNode ? { recipe_node: selectedNode } : {}),
                ...(selectedAgent ? { agent_revision_id: selectedAgent } : {}),
              }),
            );
          } catch (e) {
            setError(e instanceof Error ? e.message : String(e));
          } finally {
            (endFeedback(), setBusy(false));
          }
        }}
      >
        生成任务预览
      </ActionButton>
      {error ? <p role="alert">{error}</p> : null}
      {value && previewSignature === signature ? (
        <AgentTaskView value={value} />
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

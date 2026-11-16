import { useState } from "react";
import { ActionButton } from "./ActionFeedback";
import { MarkdownPreview } from "./configuration-ui";
import { ReadingLink } from "./reading";
import type { Row } from "./rpc-types";

type Props = {
  interaction: Row;
  projectId: string;
  ownerId: string;
  status?: string;
  busy: boolean;
  onDecide?: (approved: boolean, reason: string) => Promise<void>;
};

export function ManagerInteraction({
  interaction,
  projectId,
  ownerId,
  status,
  busy,
  onDecide,
}: Props) {
  const [reason, setReason] = useState("");
  const ready = Boolean(
    interaction.title &&
    interaction.requested_action &&
    Array.isArray(interaction.materials) &&
    Array.isArray(interaction.evidence),
  );
  return (
    <article
      className="manager-interaction"
      aria-label={interaction.title || "待决定的操作"}
    >
      <h4>{interaction.title || "此历史请求尚未记录完整材料说明"}</h4>
      <MarkdownPreview text={interaction.background || ""} />
      <h5>需要你做什么</h5>
      <MarkdownPreview
        text={
          interaction.requested_action || "请等待项目经理补齐本次说明和材料。"
        }
      />
      <h5>相关材料</h5>
      {(interaction.materials || []).length ? (
        <ul>
          {(interaction.materials as Row[]).map((reference, index) => (
            <li key={reference.output_id || index}>
              <ReadingLink
                detail="documents"
                projectId={projectId}
                ownerId={reference.owner_id || ownerId}
                target={{
                  instance_id: reference.instance_id,
                  execution_id: reference.execution_id,
                  output_id: reference.output_id,
                }}
              >
                {reference.label}
              </ReadingLink>
              <p>{reference.purpose}</p>
              <p>
                {reference.session_id && (
                  <>
                    <ReadingLink
                      detail="context"
                      projectId={projectId}
                      ownerId={reference.owner_id || ownerId}
                      target={{
                        instance_id: reference.instance_id,
                        execution_id: reference.execution_id,
                        session_id: reference.session_id,
                        turn_id: reference.turn_id,
                      }}
                    >
                      材料上下文
                    </ReadingLink>{" "}
                  </>
                )}
                <ReadingLink
                  detail="records"
                  projectId={projectId}
                  ownerId={reference.owner_id || ownerId}
                  target={{
                    instance_id: reference.instance_id,
                    execution_id: reference.execution_id,
                  }}
                >
                  材料执行记录
                </ReadingLink>
              </p>
            </li>
          ))}
        </ul>
      ) : (
        <p className="muted">本次事项暂无已封存材料，具体原因见经理说明。</p>
      )}
      <h5>项目经理已做的工作</h5>
      <MarkdownPreview text={interaction.completed_work || "尚未记录"} />
      <h5>初审与验证结果</h5>
      <MarkdownPreview text={interaction.review_summary || "尚未记录"} />
      {!!interaction.issues && (
        <>
          <h5>待解决问题</h5>
          <MarkdownPreview text={interaction.issues} />
        </>
      )}
      {!!interaction.recommendation && (
        <>
          <h5>项目经理建议</h5>
          <MarkdownPreview text={interaction.recommendation} />
        </>
      )}
      {!!interaction.evidence?.length && (
        <>
          <h5>上下文与执行依据</h5>
          <ul>
            {(interaction.evidence as Row[]).map((reference, index) => (
              <li key={reference.execution_id + ":" + index}>
                <strong>{reference.label}</strong>
                <p>{reference.purpose}</p>
                {reference.session_id && (
                  <ReadingLink
                    detail="context"
                    projectId={projectId}
                    ownerId={reference.owner_id || ownerId}
                    target={{
                      instance_id: reference.instance_id,
                      execution_id: reference.execution_id,
                      session_id: reference.session_id,
                      turn_id: reference.turn_id,
                    }}
                  >
                    查看上下文
                  </ReadingLink>
                )}{" "}
                <ReadingLink
                  detail="records"
                  projectId={projectId}
                  ownerId={reference.owner_id || ownerId}
                  target={{
                    instance_id: reference.instance_id,
                    execution_id: reference.execution_id,
                  }}
                >
                  查看执行记录
                </ReadingLink>
              </li>
            ))}
          </ul>
        </>
      )}
      {onDecide && status === "pending" && (
        <>
          <label>
            意见或修改要求
            <textarea
              aria-label="意见或修改要求"
              value={reason}
              onChange={(e) => setReason(e.target.value)}
            />
          </label>
          <div className="actions">
            <ActionButton
              disabled={busy || !ready}
              onClick={() => void onDecide(true, reason)}
            >
              确认：{interaction.title || "当前事项"}
            </ActionButton>
            <ActionButton
              disabled={busy || !ready}
              onClick={() => void onDecide(false, reason)}
            >
              退回并反馈意见
            </ActionButton>
          </div>
        </>
      )}
      {status === "stale" && (
        <p role="status">
          本次材料或计划已变化，请等待项目经理准备新的确认事项。
        </p>
      )}
      {status === "approved" && (
        <p role="status">你已确认本轮事项，项目经理将依据决定继续处理。</p>
      )}
      {status === "rejected" && (
        <p role="status">你已退回本轮事项，项目经理将依据意见继续处理。</p>
      )}
    </article>
  );
}

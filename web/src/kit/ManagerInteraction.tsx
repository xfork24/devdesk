import { displayText } from "../sdk/display-text";
import { materialTitle } from "./workflow-material";
import { useState } from "react";
import { ActionButton } from "../sdk/ActionFeedback";
import { ManagerDisclosure } from "./ManagerDisclosure";
import { MarkdownPreview } from "./configuration-ui";
import { ReadingLink } from "./reading";
import type { Row } from "../sdk/rpc-types";

type Props = {
  interaction: Row;
  basis?: string;
  projectId: string;
  ownerId: string;
  status?: string;
  busy: boolean;
  onDecide?: (approved: boolean, reason: string) => Promise<void>;
  requireReason?: boolean;
  awaitingInput?: boolean;
  expanded?: boolean;
  feedback?: { id: string; input: string }[];
};

function ManagerMarkdown({ text }: { text: string }) {
  return <MarkdownPreview text={text} linkGithubRevisions />;
}

export function ManagerInteraction({
  interaction: latestInteraction,
  basis,
  projectId,
  ownerId,
  status: latestStatus,
  busy,
  onDecide: latestDecide,
  requireReason = false,
  awaitingInput = false,
  expanded = false,
  feedback = [],
}: Props) {
  const [reason, setReason] = useState("");
  const [original, setOriginal] = useState<{
    interaction: Row;
    decide: Props["onDecide"];
    basis?: string;
  }>();
  const settled = ["approved", "rejected", "withdrawn"].includes(
    latestStatus || "",
  );
  const interaction = settled
    ? latestInteraction
    : original?.interaction || latestInteraction;
  const onDecide = original?.decide || latestDecide;
  const status =
    !settled && original && original.basis !== basis ? "stale" : latestStatus;
  const capture = () => {
    if (!original)
      setOriginal({
        interaction: structuredClone(latestInteraction),
        decide: latestDecide,
        basis,
      });
  };

  const [error, setError] = useState("");
  async function decide(approved: boolean) {
    setError("");
    try {
      capture();
      await onDecide?.(approved, reason);
    } catch (e) {
      setError(String(e));
    }
  }
  const ready = Boolean(
    interaction.title &&
    interaction.requested_action &&
    Array.isArray(interaction.materials) &&
    Array.isArray(interaction.evidence),
  );
  return (
    <article
      className="manager-interaction"
      aria-label={displayText(interaction.title || "待决定的操作")}
    >
      {!settled && original && original.basis !== basis && (
        <p role="status">
          事项已变化，原意见和依据已保留。
          <button onClick={() => setOriginal(undefined)}>
            使用当前事项重新确认
          </button>
        </p>
      )}
      <ManagerDisclosure
        className="manager-disclosure"
        open={expanded || status === "pending" || awaitingInput}
      >
        <summary>
          <strong>
            {displayText(interaction.title || "此历史请求尚未记录完整材料说明")}
          </strong>
          <span className="manager-interaction-status">
            {displayText(
              awaitingInput
                ? "待补充信息"
                : (
                    {
                      pending: interaction.exception
                        ? "待用户 Approve"
                        : "待确认",
                      approved: interaction.exception
                        ? "评审未通过，用户已例外批准"
                        : "已确认",
                      rejected: "已退回",
                      withdrawn: "已撤回额外确认",
                      stale: "已过期",
                    } as Record<string, string>
                  )[status || ""] || (feedback.length ? "已反馈" : "说明"),
            )}
          </span>
        </summary>
        <div className="manager-disclosure-body">
          {interaction.source_label && (
            <p className="muted">
              所属流程与步骤：{displayText(interaction.source_label)}
            </p>
          )}
          <ManagerMarkdown text={interaction.background || ""} />
          {typeof interaction.supplemental_input === "string" && (
            <details>
              <summary>拟转交的补充输入（原文）</summary>
              <p className="manager-feedback-text">
                {interaction.supplemental_input}
              </p>
            </details>
          )}
          {feedback.length > 0 && (
            <section aria-label="用户反馈">
              <h5>用户反馈</h5>
              {feedback.map((reply) => (
                <p key={reply.id} className="manager-feedback-text">
                  {reply.input}
                </p>
              ))}
            </section>
          )}
          {interaction.signed_reason ? (
            <>
              <h5>用户签署意见</h5>
              <ManagerMarkdown text={interaction.signed_reason} />
            </>
          ) : null}
          {(interaction.rework_budgets || []).map((budget: Row) => (
            <p key={budget.id}>
              业务返工：{budget.used} / {budget.limit}，剩余 {budget.remaining}{" "}
              次{budget.remaining === 0 ? " · 额度耗尽" : ""}
            </p>
          ))}
          <h5>需要你做什么</h5>
          <ManagerMarkdown
            text={
              interaction.requested_action ||
              "请等待项目经理补齐本次说明和材料。"
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
                    {displayText(
                      materialTitle(reference.label, reference.step_name),
                    )}
                  </ReadingLink>
                  <p>{displayText(reference.purpose)}</p>
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
                          {displayText(
                            materialTitle(reference.label, reference.step_name),
                          )}{" "}
                          · 上下文
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
                      {displayText(
                        materialTitle(reference.label, reference.step_name),
                      )}{" "}
                      · 执行记录
                    </ReadingLink>
                  </p>
                </li>
              ))}
            </ul>
          ) : (
            <p className="muted">
              本次事项暂无已封存材料，具体原因见经理说明。
            </p>
          )}
          <h5>项目经理已做的工作</h5>
          <ManagerMarkdown text={interaction.completed_work || "尚未记录"} />
          <h5>初审与验证结果</h5>
          <ManagerMarkdown text={interaction.review_summary || "尚未记录"} />
          {!!interaction.issues && (
            <>
              <h5>待解决问题</h5>
              <ManagerMarkdown text={interaction.issues} />
            </>
          )}
          {!!interaction.recommendation && (
            <>
              <h5>项目经理建议</h5>
              <ManagerMarkdown text={interaction.recommendation} />
            </>
          )}
          {!!interaction.evidence?.length && (
            <>
              <h5>上下文与执行依据</h5>
              <ul>
                {(interaction.evidence as Row[]).map((reference, index) => (
                  <li key={reference.execution_id + ":" + index}>
                    <strong>
                      {displayText(
                        reference.output_id
                          ? materialTitle(reference.label, reference.step_name)
                          : reference.label,
                      )}
                    </strong>
                    <p>{displayText(reference.purpose)}</p>
                    {reference.output_id && (
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
                        查看
                        {displayText(
                          materialTitle(reference.label, reference.step_name),
                        )}
                        原件
                      </ReadingLink>
                    )}{" "}
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
                  onChange={(e) => {
                    capture();
                    setReason(e.target.value);
                  }}
                />
              </label>
              <div className="actions">
                <ActionButton
                  disabled={busy || !ready || (requireReason && !reason.trim())}
                  onClick={() => void decide(true)}
                >
                  确认：{displayText(interaction.title || "当前事项")}
                </ActionButton>
                <ActionButton
                  disabled={busy || !ready || !reason.trim()}
                  onClick={() => void decide(false)}
                >
                  退回并反馈意见
                </ActionButton>
              </div>
              {requireReason && (
                <p className="muted">完成条件未满足，手动批准必须填写理由。</p>
              )}
              {error && (
                <p role="alert" className="error">
                  {displayText(error)}
                </p>
              )}
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
        </div>
      </ManagerDisclosure>
    </article>
  );
}

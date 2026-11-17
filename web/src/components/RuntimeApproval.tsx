import { useState } from "react";
import { displayText } from "../display-text";
import { ActionButton } from "./ActionFeedback";
import { DiscussionLink } from "./reading";
import type { Request, Row } from "./rpc-types";

export function RuntimeApproval({
  request,
  interaction,
  projectId,
  ownerId,
  refresh,
}: {
  request: Request;
  interaction: Row;
  projectId: string;
  ownerId: string;
  refresh: () => Promise<void>;
}) {
  const [busy, setBusy] = useState(false);
  const [error, setError] = useState("");
  async function answer(outcome: string) {
    setBusy(true);
    setError("");
    try {
      await request("interaction.answer", {
        project_id: projectId,
        command_id: crypto.randomUUID(),
        target: interaction.id,
        expected_revision: interaction.revision,
        payload: {
          session_id: interaction.session_id,
          turn_id: interaction.turn_id,
          answer: { kind: "tool_approval", outcome },
        },
      });
      await refresh();
    } catch (e) {
      setError(String(e));
    } finally {
      setBusy(false);
    }
  }
  return (
    <article className="manager-interaction" aria-label="运行时工具审批">
      <details className="manager-disclosure" open>
        <summary>
          <strong>运行时工具审批</strong>
          <span className="manager-interaction-status">待审批</span>
        </summary>
        <div className="manager-disclosure-body">
          <p>{displayText(interaction.reason)}</p>
          {interaction.tool && <pre>{displayText(interaction.tool)}</pre>}
          <DiscussionLink
            projectId={projectId}
            ownerId={interaction.owner_id || ownerId}
            sessionId={interaction.session_id}
            turnId={interaction.turn_id}
          >
            查看执行会话
          </DiscussionLink>
          <div className="actions">
            <ActionButton
              disabled={busy}
              onClick={() => void answer("allowed_once")}
            >
              仅允许本次
            </ActionButton>
            <ActionButton
              disabled={busy}
              onClick={() => void answer("rejected")}
            >
              拒绝
            </ActionButton>
          </div>
          {error && (
            <p role="alert" className="error">
              {displayText(error)}
            </p>
          )}
        </div>
      </details>
    </article>
  );
}

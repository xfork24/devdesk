import { useRef, useState } from "react";
import { ActionButton } from "../sdk/ActionFeedback";
import type { Request, Row } from "../sdk/rpc-types";

export function WorkflowStepInput({
  request,
  projectId,
  item,
  refresh,
}: {
  request: Request;
  projectId: string;
  item: Row;
  refresh: () => Promise<void>;
}) {
  const [input, setInput] = useState("");
  const [original, setOriginal] = useState<Row>();
  const [busy, setBusy] = useState(false);
  const [error, setError] = useState("");
  const attempt = useRef<{ fingerprint: string; id: string } | undefined>(
    undefined,
  );
  const basis = (row: Row) =>
    JSON.stringify([
      row.instance_id,
      row.execution_id,
      row.plan_revision_id,
      row.control_version,
      row.step_revision,
    ]);
  const stale = Boolean(original && basis(original) !== basis(item));
  const selected = original || item;
  const ready =
    selected.revision > 0 &&
    selected.plan_revision_id &&
    selected.step_revision > 0;
  async function submit() {
    setBusy(true);
    setError("");
    try {
      const params = {
        project_id: projectId,
        target: selected.instance_id,
        expected_revision: selected.revision,
        payload: {
          plan_revision_id: selected.plan_revision_id,
          control_version: selected.control_version,
          expected_step_revision: selected.step_revision,
          execution_id: selected.execution_id,
          step: selected.step,
          input,
        },
      };
      const fingerprint = JSON.stringify(params);
      if (attempt.current?.fingerprint !== fingerprint)
        attempt.current = { fingerprint, id: crypto.randomUUID() };
      await request("workflow.step.input", {
        ...params,
        command_id: attempt.current.id,
      });
      setInput("");
      setOriginal(undefined);
      attempt.current = undefined;
      await refresh();
    } catch (error) {
      setError(String(error));
    } finally {
      setBusy(false);
    }
  }
  return (
    <form
      className="workflow-step-inline-input"
      onSubmit={(event) => {
        event.preventDefault();
        if (!busy && !stale && ready && input.trim()) void submit();
      }}
    >
      <label>
        补充本次任务
        <textarea
          aria-label="补充本次任务"
          value={input}
          onChange={(event) => {
            setOriginal((previous) => previous || structuredClone(item));
            setInput(event.target.value);
          }}
        />
      </label>
      {stale && (
        <p role="status">
          步骤或计划已变化，原输入已保留。
          <button type="button" onClick={() => setOriginal(undefined)}>
            使用当前步骤重新核对
          </button>
        </p>
      )}
      {error && <p role="alert">{error}</p>}
      <ActionButton
        type="submit"
        disabled={busy || stale || !ready || !input.trim()}
      >
        提交补充输入
      </ActionButton>
    </form>
  );
}

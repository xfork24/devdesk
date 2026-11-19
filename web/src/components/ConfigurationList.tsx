import { useEffect, useRef, type ReactNode } from "react";
import { displayText } from "../display-text";
import { ActionButton } from "./ActionFeedback";
import { compareConfigurationRecords } from "./configuration-state";
import type { Row } from "./rpc-types";

type Props = {
  records: Row[];
  selectedId?: string;
  onSelect: (record: Row) => void | Promise<void>;
  ready?: boolean;
  autoSelectFirst?: boolean;
  disabled?: boolean;
  newAction?: { label: string; onClick: () => void; selected: boolean };
  actions?: ReactNode;
  emptyMessage?: string;
};

export function ConfigurationList({
  records,
  selectedId,
  onSelect,
  ready = true,
  autoSelectFirst = true,
  disabled = false,
  newAction,
  actions,
  emptyMessage,
}: Props) {
  const initialized = useRef(false);
  const rows = [...records].sort(compareConfigurationRecords);
  useEffect(() => {
    if (initialized.current || !ready) return;
    initialized.current = true;
    if (autoSelectFirst && !selectedId && rows[0]) void onSelect(rows[0]);
  }, [ready, autoSelectFirst, selectedId, rows, onSelect]);

  return (
    <div
      className="configuration-list"
      onClickCapture={(event) => {
        if (event.target instanceof Element && event.target.closest("button"))
          initialized.current = true;
      }}
    >
      {newAction && (
        <ActionButton
          type="button"
          className={
            newAction.selected ? "new-resource selected" : "new-resource"
          }
          aria-pressed={newAction.selected}
          disabled={disabled}
          onClick={newAction.onClick}
        >
          {displayText(newAction.label)}
        </ActionButton>
      )}
      {actions}
      {rows.map((row) => (
        <ActionButton
          type="button"
          key={row.id}
          disabled={disabled}
          aria-pressed={selectedId === row.id}
          className={selectedId === row.id ? "selected" : ""}
          onClick={() => {
            void onSelect(row);
          }}
        >
          <strong>{displayText(row.name)}</strong>
          <span>
            {displayText(
              row.type === "workflow_draft" ? "草稿" : `v${row.version}`,
            )}
            {row.scope &&
              ` · ${displayText(row.scope === "global" ? "Global" : "项目")}`}
          </span>
        </ActionButton>
      ))}
      {ready && !rows.length && emptyMessage && (
        <p>{displayText(emptyMessage)}</p>
      )}
    </div>
  );
}

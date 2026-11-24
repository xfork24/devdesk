import { displayText } from "../display-text";
import { type Row } from "./rpc-types";

export function StandardDiff({ diff }: { diff: Row }) {
  return (
    <div role="status" className="standard-diff">
      <p>
        {displayText(
          diff.equal
            ? "规范定义相同"
            : `${diff.changes?.length || 0} 项规范变化`,
        )}
      </p>
      {(diff.changes || []).map((change: Row, index: number) => (
        <details key={index}>
          <summary>
            {displayText(
              (
                {
                  added: "新增",
                  removed: "移除",
                  definition_changed: "定义变化",
                  reference_changed: "引用版本变化",
                  relationship_added: "新增关联",
                  relationship_removed: "移除关联",
                  entries_changed: "入口变化",
                } as Row
              )[change.kind] || change.kind,
            )}{" "}
            ·{" "}
            {displayText(
              change.source_kind || change.edge?.relation || "Workflow",
            )}{" "}
            {displayText(
              change.before_ref || change.after_ref || change.edge?.path || "",
            )}
          </summary>
          {(change.fields || []).map((field: Row) => (
            <div key={field.path}>
              <strong>{displayText(field.path)}</strong>
              <div className="standard-comparison">
                <pre>{displayText(JSON.stringify(field.before, null, 2))}</pre>
                <pre>{displayText(JSON.stringify(field.after, null, 2))}</pre>
              </div>
            </div>
          ))}
          {!change.fields?.length ? (
            <pre>{displayText(JSON.stringify(change, null, 2))}</pre>
          ) : null}
        </details>
      ))}
    </div>
  );
}

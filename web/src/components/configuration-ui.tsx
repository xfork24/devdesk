import type { Row } from "./rpc-types";
import { ActionButton, ActionSelect } from "./ActionFeedback";
import {
  lazy,
  Suspense,
  useEffect,
  useId,
  useRef,
  useState,
  type ReactNode,
} from "react";
const RenderMarkdown = lazy(() => import("./MarkdownPreview"));
type RevisionSelectProps = {
  label: string;
  records: Row[];
  required?: boolean;
} & (
  | { multiple: true; value: string[]; onChange: (value: string[]) => void }
  | { multiple?: false; value: string; onChange: (value: string) => void }
);
export function RevisionSelect(props: RevisionSelectProps) {
  const { label, records, value, multiple = false, required = false } = props;
  const ids = Array.isArray(value) ? value : [value];
  const options = [
    ...new Map(records.map((row) => [row.id, row])).values(),
  ].sort(
    (a, b) =>
      String(a.definition_id).localeCompare(String(b.definition_id)) ||
      b.version - a.version,
  );
  for (const id of ids) {
    const old = records.find((r) => r.id === id);
    if (old && !options.some((r) => r.id === id)) options.push(old);
  }
  return (
    <label>
      {label}
      <ActionSelect
        aria-label={label}
        multiple={multiple}
        required={required}
        value={value}
        onChange={(e) => {
          if (props.multiple) {
            props.onChange(
              Array.from(e.target.selectedOptions, (o) => o.value),
            );
          } else {
            props.onChange(e.target.value);
          }
        }}
      >
        {!multiple && <option value="">请选择</option>}
        {options.map((r) => (
          <option key={r.id} value={r.id}>
            {r.name} · v{r.version}
            {r.scope?.startsWith("project:") ? " · 项目" : ""}
          </option>
        ))}
        {ids
          .filter((id) => id && !options.some((r) => r.id === id))
          .map((id) => (
            <option key={id} value={id}>
              固定历史修订 · {id}
            </option>
          ))}
      </ActionSelect>
    </label>
  );
}
export function MarkdownPreview({
  text,
  trailing,
}: {
  text: string;
  trailing?: ReactNode;
}) {
  return (
    <div className="markdown-preview" aria-label="内容预览">
      <Suspense fallback={<p>正在加载内容…</p>}>
        <RenderMarkdown text={text} trailing={trailing} />
      </Suspense>
    </div>
  );
}

export function MarkdownField({
  label,
  value,
  onChange,
  readOnly = false,
  disabled = false,
  maxLength,
  required = false,
  rows = 10,
}: {
  label: string;
  value: string;
  onChange: (value: string) => void;
  readOnly?: boolean;
  disabled?: boolean;
  maxLength?: number;
  required?: boolean;
  rows?: number;
}) {
  const [editing, setEditing] = useState(false);
  const field = useRef<HTMLDivElement>(null);
  const outsidePointer = useRef(false);
  const id = useId();
  useEffect(() => {
    if (!editing) return;
    const pointerDown = (event: PointerEvent) => {
      outsidePointer.current = !field.current?.contains(event.target as Node);
    };
    const finishClick = () => {
      if (outsidePointer.current) {
        outsidePointer.current = false;
        setEditing(false);
      }
    };
    // Let the outside control handle its click before changing the editor's height.
    document.addEventListener("pointerdown", pointerDown, true);
    document.addEventListener("click", finishClick);
    document.addEventListener("pointercancel", finishClick);
    return () => {
      outsidePointer.current = false;
      document.removeEventListener("pointerdown", pointerDown, true);
      document.removeEventListener("click", finishClick);
      document.removeEventListener("pointercancel", finishClick);
    };
  }, [editing]);
  return (
    <div
      ref={field}
      className="markdown-field full-width"
      role="group"
      aria-label={label}
      aria-disabled={disabled || undefined}
      onBlur={(event) => {
        if (
          !outsidePointer.current &&
          !event.currentTarget.contains(event.relatedTarget as Node | null)
        )
          setEditing(false);
      }}
    >
      <div className="markdown-field-heading">
        {editing && !readOnly && !disabled ? (
          <label htmlFor={id}>{label}</label>
        ) : (
          <span>{label}</span>
        )}
      </div>
      {editing && !readOnly && !disabled ? (
        <textarea
          id={id}
          aria-label={label}
          autoFocus
          required={required}
          maxLength={maxLength}
          rows={rows}
          value={value}
          onChange={(event) => onChange(event.target.value)}
        />
      ) : readOnly || disabled ? (
        value.trim() ? (
          <MarkdownPreview text={value} />
        ) : (
          <p className="markdown-empty">暂无内容</p>
        )
      ) : (
        <div
          className="markdown-editor-preview"
          role="button"
          tabIndex={0}
          aria-label={label}
          onClick={(event) => {
            event.preventDefault();
            setEditing(true);
          }}
          onKeyDown={(event) => {
            if (event.key === "Enter" || event.key === " ") {
              event.preventDefault();
              setEditing(true);
            }
          }}
        >
          {value.trim() ? (
            <MarkdownPreview text={value} />
          ) : (
            <p className="muted">点击填写内容</p>
          )}
        </div>
      )}
    </div>
  );
}

export function MarkdownTopicsField({
  value,
  onChange,
  readOnly = false,
}: {
  value: string[];
  onChange: (value: string[]) => void;
  readOnly?: boolean;
}) {
  return (
    <div
      className="markdown-field full-width"
      role="group"
      aria-label="必须讨论的主题"
    >
      <div className="markdown-field-heading">
        <span>必须讨论的主题</span>
      </div>
      {value.length ? (
        <ol className="markdown-topics">
          {value.map((topic, index) => (
            <li key={index}>
              <MarkdownField
                label={`讨论主题 ${index + 1}`}
                value={topic}
                readOnly={readOnly}
                rows={4}
                onChange={(next) =>
                  onChange(value.map((item, i) => (i === index ? next : item)))
                }
              />
              {!readOnly && (
                <ActionButton
                  type="button"
                  aria-label={`删除讨论主题 ${index + 1}`}
                  onClick={() => onChange(value.filter((_, i) => i !== index))}
                >
                  删除主题
                </ActionButton>
              )}
            </li>
          ))}
        </ol>
      ) : (
        <p className="muted">暂无主题</p>
      )}
      {!readOnly && (
        <ActionButton type="button" onClick={() => onChange([...value, ""])}>
          添加讨论主题
        </ActionButton>
      )}
    </div>
  );
}

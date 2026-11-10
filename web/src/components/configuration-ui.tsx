import { ActionButton, ActionSelect } from "./ActionFeedback";
import {
  lazy,
  Suspense,
  useCallback,
  useEffect,
  useId,
  useRef,
  useState,
} from "react";
import { useCatalogGeneration } from "../template-catalog";
const RenderMarkdown = lazy(() => import("./MarkdownPreview"));
export type Row = Record<string, any>;
export type Request = (method: string, params?: Row) => Promise<any>;
export const templateSourceLabel = (row?: Row) =>
  (
    ({
      default: "默认副本",
      modified: "已修改",
      custom: "自建",
      existing: "已有定义",
      archived: "来自重置前定义",
    }) as Record<string, string>
  )[row?.template_source?.origin as string] || "";
export function latest(records: Row[]) {
  const map = new Map<string, Row>();
  for (const r of records) {
    const key = `${r.kind}:${r.definition_id}`;
    if (!map.has(key) || map.get(key)!.version < r.version) map.set(key, r);
  }
  return [...map.values()];
}
export async function loadCatalog(
  request: Request,
  project?: Row,
  includeArchived = false,
) {
  const result: Row[] = [];
  let offset = 0;
  let generation: number | undefined;
  for (;;) {
    const page = await request("query.configurations", {
      include_archived: includeArchived,
      local: !!project,
      project_id: project?.id,
      offset,
      limit: 100,
      include_history: true,
    });
    if (generation !== undefined && page.catalog_generation !== generation)
      throw new Error("模板目录已变化，请重新载入");
    generation = page.catalog_generation;
    result.push(
      ...page.records.map((row: Row) => ({
        ...row,
        template_source: page.template_sources?.[row.id],
      })),
    );
    offset += page.records.length;
    if (offset >= page.total || page.records.length === 0) break;
  }
  return result;
}
export async function loadIntegrationRecords(request: Request, project?: Row) {
  const result: Row[] = [];
  let offset = 0;
  let generation: number | undefined;
  for (;;) {
    const page = await request("query.integrations", {
      local: !!project,
      project_id: project?.id,
      include_history: true,
      offset,
      limit: 100,
    });
    if (generation !== undefined && page.catalog_generation !== generation)
      throw new Error("模板目录已变化，请重新载入");
    generation = page.catalog_generation;
    result.push(
      ...page.records.map((row: Row) => ({
        ...row,
        template_source: page.template_sources?.[row.id],
      })),
    );
    offset += page.records.length;
    if (offset >= page.total || !page.records.length) return result;
  }
}
export function useDraft<T>(key: string, initial: () => T) {
  const generation = useCatalogGeneration();
  key = generation > 1 ? `${key}:catalog:${generation}` : key;
  const cache = useRef<Record<string, T>>({});
  const [value, rawSet] = useState<T>(() => {
    try {
      const saved = JSON.parse(localStorage.getItem(key) || "null");
      if (saved?.version === 3) {
        cache.current = saved.drafts || {};
        return saved.value;
      }
      if (saved?.version === 2) return saved.value;
    } catch {}
    return initial();
  });
  const setValue = useCallback((next: T | ((previous: T) => T)) => {
    rawSet((previous) => {
      const result =
        typeof next === "function" ? (next as (p: T) => T)(previous) : next;
      const before = (previous as Row).selected;
      const after = (result as Row).selected;
      if (before !== undefined && after !== undefined && before !== after) {
        cache.current[before || "new"] = previous;
        const cached = after ? cache.current[after] : undefined;
        return cached
          ? (result as Row).view
            ? { ...cached, view: (result as Row).view }
            : cached
          : result;
      }
      return result;
    });
  }, []);
  useEffect(() => {
    try {
      const selected = (value as Row).selected;
      if (selected !== undefined) cache.current[selected || "new"] = value;
      localStorage.setItem(
        key,
        JSON.stringify({ version: 3, value, drafts: cache.current }),
      );
    } catch {}
  }, [key, value]);
  return [value, setValue] as const;
}
export function selectConfigurationObject(id: string, key = "config_object") {
  const params = new URLSearchParams(location.search);
  if (id) params.set(key, id);
  else params.delete(key);
  history.replaceState(null, "", `?${params}`);
}
export function useGroupTab(group: string, allowed: string[]) {
  const read = useCallback(() => {
    const normalize = (value: string | null) =>
      group === "actors" && value === "agent_instance" ? "agent" : value;
    const url = normalize(
      new URLSearchParams(location.search).get("config_tab"),
    );
    let stored: string | null = null;
    try {
      stored = normalize(localStorage.getItem(`devdesk:v2:tab:${group}`));
    } catch {}
    return allowed.includes(url || "")
      ? url!
      : allowed.includes(stored || "")
        ? stored!
        : allowed[0];
  }, [group, allowed.join("|")]);
  const [tab, setTab] = useState(read);
  useEffect(() => {
    if (group === "actors") {
      const params = new URLSearchParams(location.search);
      if (params.get("config_tab") === "agent_instance") {
        params.set("config_tab", "agent");
        history.replaceState(null, "", `?${params}`);
      }
    }
    const pop = () => setTab(read());
    window.addEventListener("popstate", pop);
    return () => window.removeEventListener("popstate", pop);
  }, [read]);
  const choose = (value: string) => {
    setTab(value);
    const params = new URLSearchParams(location.search);
    params.set("config_tab", value);
    params.delete("config_object");
    history.pushState(null, "", `?${params}`);
    try {
      localStorage.setItem(`devdesk:v2:tab:${group}`, value);
    } catch {}
  };
  return [tab, choose] as const;
}
export function RevisionSelect({
  label,
  records,
  value,
  onChange,
  multiple = false,
  required = false,
}: {
  label: string;
  records: Row[];
  value: string | string[];
  onChange: (value: any) => void;
  multiple?: boolean;
  required?: boolean;
}) {
  const ids = Array.isArray(value) ? value : [value];
  const options = latest(records);
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
        onChange={(e) =>
          onChange(
            multiple
              ? Array.from(e.target.selectedOptions, (o) => o.value)
              : e.target.value,
          )
        }
      >
        {!multiple && <option value="">请选择</option>}
        {options.map((r) => (
          <option key={r.id} value={r.id}>
            {r.name} · v{r.version}
            {r.scope?.kind === "project" ? " · 项目" : ""}
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
export function MarkdownPreview({ text }: { text: string }) {
  return (
    <div className="markdown-preview" aria-label="内容预览">
      <Suspense fallback={<p>正在加载内容…</p>}>
        <RenderMarkdown text={text} />
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

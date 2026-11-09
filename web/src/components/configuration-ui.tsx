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
const RenderMarkdown = lazy(() => import("./MarkdownPreview"));
export type Row = Record<string, any>;
export type Request = (method: string, params?: Row) => Promise<any>;
export const dutyLabels: Record<string, string> = {
  analyst: "需求分析",
  architect: "架构设计",
  developer: "开发实现",
  tester: "测试验证",
  reviewer: "独立评审",
};
export const actionLabels: Record<string, string> = {
  "agent.interact": "角色讨论",
  "proposal.adopt": "采纳提案",
  "attempt.execute": "正式执行",
  "delivery.review": "交付评审",
  "merge.prepare": "准备候选",
  "merge.review": "候选评审",
  "merge.apply": "应用合入",
  "snapshot.capture": "上下文快照",
  "resource.cleanup": "资源释放",
  "tasks.foreach": "Task 子流程",
  "workflow.call": "调用子流程",
  branch: "条件分支",
  join: "汇合",
};
export function latest(records: Row[]) {
  const map = new Map<string, Row>();
  for (const r of records) {
    const key = `${r.kind}:${r.definition_id}`;
    if (!map.has(key) || map.get(key)!.version < r.version) map.set(key, r);
  }
  return [...map.values()];
}
export async function loadCatalog(request: Request, project?: Row) {
  const result: Row[] = [];
  let offset = 0;
  for (;;) {
    const page = await request("query.configurations", {
      local: !!project,
      project_id: project?.id,
      offset,
      limit: 100,
      include_history: true,
    });
    result.push(...page.records);
    offset += page.records.length;
    if (offset >= page.total || page.records.length === 0) break;
  }
  return result;
}
export async function loadIntegrationRecords(request: Request, project?: Row) {
  const result: Row[] = [];
  let offset = 0;
  for (;;) {
    const page = await request("query.integrations", {
      local: !!project,
      project_id: project?.id,
      include_history: true,
      offset,
      limit: 100,
    });
    result.push(...page.records);
    offset += page.records.length;
    if (offset >= page.total || !page.records.length) return result;
  }
}
export function useDraft<T>(key: string, initial: () => T) {
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
    const url = new URLSearchParams(location.search).get("config_tab");
    let stored: string | null = null;
    try {
      stored = localStorage.getItem(`devdesk:v2:tab:${group}`);
    } catch {}
    return allowed.includes(url || "")
      ? url!
      : allowed.includes(stored || "")
        ? stored!
        : allowed[0];
  }, [group, allowed.join("|")]);
  const [tab, setTab] = useState(read);
  useEffect(() => {
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
    <div className="markdown-preview" aria-label="Markdown 预览">
      <Suspense fallback={<p>正在渲染 Markdown…</p>}>
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
  required = false,
  rows = 10,
}: {
  label: string;
  value: string;
  onChange: (value: string) => void;
  readOnly?: boolean;
  required?: boolean;
  rows?: number;
}) {
  const [editing, setEditing] = useState(false);
  const id = useId();
  return (
    <div className="markdown-field full-width" role="group" aria-label={label}>
      <div className="markdown-field-heading">
        {editing && !readOnly ? (
          <label htmlFor={id}>{label}</label>
        ) : (
          <span>{label}</span>
        )}
        {!readOnly && (
          <ActionButton
            type="button"
            aria-label={editing ? `预览${label}` : `编辑${label}源码`}
            onClick={() => setEditing(!editing)}
          >
            {editing ? "完成编辑" : "编辑源码"}
          </ActionButton>
        )}
      </div>
      {editing && !readOnly ? (
        <textarea
          id={id}
          autoFocus
          required={required}
          rows={rows}
          value={value}
          onChange={(e) => onChange(e.target.value)}
        />
      ) : value.trim() ? (
        <MarkdownPreview text={value} />
      ) : (
        <p className="markdown-empty">暂无内容</p>
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
  const [editing, setEditing] = useState(false);
  const label = "必须讨论的主题";
  return (
    <div className="markdown-field full-width" role="group" aria-label={label}>
      <div className="markdown-field-heading">
        <span>{label}</span>
        {!readOnly && (
          <ActionButton
            type="button"
            aria-label={editing ? `预览${label}` : `编辑${label}源码`}
            onClick={() => setEditing(!editing)}
          >
            {editing ? "完成编辑" : "编辑源码"}
          </ActionButton>
        )}
      </div>
      {editing && !readOnly ? (
        <>
          {value.map((topic, index) => (
            <div key={index} className="markdown-topic-source">
              <label>
                讨论主题 {index + 1}
                <textarea
                  rows={4}
                  value={topic}
                  onChange={(e) =>
                    onChange(
                      value.map((item, i) =>
                        i === index ? e.target.value : item,
                      ),
                    )
                  }
                />
              </label>
              <ActionButton
                type="button"
                aria-label={`删除讨论主题 ${index + 1}`}
                onClick={() => onChange(value.filter((_, i) => i !== index))}
              >
                删除主题
              </ActionButton>
            </div>
          ))}
          <ActionButton type="button" onClick={() => onChange([...value, ""])}>
            添加讨论主题
          </ActionButton>
        </>
      ) : value.length ? (
        <ol className="markdown-topics">
          {value.map((topic, index) => (
            <li key={index}>
              <MarkdownPreview text={topic} />
            </li>
          ))}
        </ol>
      ) : (
        <p className="markdown-empty">暂无主题</p>
      )}
    </div>
  );
}

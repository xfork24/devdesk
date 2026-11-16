import { useCallback, useEffect, useRef, useState } from "react";
import { getConfigurationSession } from "../configuration-client";
import { useCatalogGeneration } from "../template-catalog";
import type { Row, Request } from "./rpc-types";
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
    const key = `${r.type}:${r.definition_id}`;
    if (!map.has(key) || map.get(key)!.version < r.version) map.set(key, r);
  }
  return [...map.values()];
}
export async function loadCatalog(
  request: Request,
  projectId?: string,
  includeArchived = false,
  kind?: string,
  includeHistory = false,
) {
  const result: Row[] = [];
  let offset = 0;
  let generation: string | undefined;
  for (;;) {
    const page = await request("query.configuration_index", {
      include_archived: includeArchived,
      local: !!projectId,
      project_id: projectId,
      offset,
      limit: 1000,
      include_history: includeHistory,
      kind,
      index_version: generation,
    });
    if (generation !== undefined && page.index_version !== generation)
      throw new Error("模板目录已变化，请重新载入");
    generation = page.index_version;
    result.push(...page.records.map(decodeConfigurationIndex));
    offset += page.records.length;
    if (offset >= page.total || page.records.length === 0) break;
  }
  return result;
}
export function decodeConfigurationIndex(row: Row): Row {
  if (!row.hash) return row; // Structural RPC fixtures and existing complete records retain their wire shape.
  return {
    ...row,
    definition_id: row.identity || row.id.replace(/:[1-9]\d*$/, ""),
    content_hash: row.hash,
    data: row.data || {},
    template_source: { origin: row.origin },
    template_archived: !!row.archived,
    _index: true,
  };
}
export async function loadConfigurationRecord(
  request: Request,
  row: Row,
  projectId?: string,
  signal?: AbortSignal,
): Promise<Row> {
  if (!row._index && row.data) return row;
  const result = await request(
    "query.configuration_records",
    { refs: [row.id], project_id: projectId, local: !!projectId },
    { signal },
  );
  const record = result.records.find((r: Row) => r.id === row.id);
  if (!record) throw new Error("配置修订已不可用");
  return record;
}
export async function loadIntegrationRecords(
  request: Request,
  projectId?: string,
) {
  const result: Row[] = [];
  let offset = 0;
  let generation: number | undefined;
  for (;;) {
    const page = await request("query.integrations", {
      local: !!projectId,
      project_id: projectId,
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
  const session = getConfigurationSession();
  try {
    const ownerKey = `${key}:dataset-owner`;
    const owner = localStorage.getItem(ownerKey);
    if (!owner) localStorage.setItem(ownerKey, session);
    else if (owner !== session) key = `${key}:dataset:${session}`;
  } catch {
    key = `${key}:dataset:${session}`;
  }
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
  const allowedKey = JSON.stringify(allowed);
  const read = useCallback(() => {
    const allowed: string[] = JSON.parse(allowedKey);
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
  }, [group, allowedKey]);
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
  }, [read, group]);
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
// References are edited with payload fields, and written directly on the source envelope.
const sourceReferenceFields: Record<string, string[]> = {
  capability: ["harness_ref", "model_ref", "skills", "tools", "spec_refs"],
  agent: ["role_ref", "capability_ref"],
  delivery_type: ["spec_refs"],
  harness: ["model_providers", "model_catalog_ref"],
  standard: ["root_workflow_refs"],
};
export function sourceContent(record: Row): Row {
  return structuredClone({
    ...record.data,
    ...Object.fromEntries(
      (sourceReferenceFields[record.type] || [])
        .filter((key) => key in record)
        .map((key) => [key, record[key]]),
    ),
  });
}
export function sourcePayload(type: string, content: Row): Row {
  const data = { ...content };
  const refs: Row = {};
  for (const key of sourceReferenceFields[type] || []) {
    if (key in data) {
      refs[key] = data[key];
      delete data[key];
    }
  }
  return { ...refs, data };
}

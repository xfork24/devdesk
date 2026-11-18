import { QueryCache } from "./query-cache";
import type { Request, RequestOptions, Row } from "./components/rpc-types";

const queries = new QueryCache();
// Discovery belongs to this bounded background service, including while a page is detached.
const models = new QueryCache(16, 1024 * 1024, true);
const modelListeners = new Set<() => void>();
const modelFailures = new Map<string, { until: number; error: unknown }>();
export const getModelEpoch = () => modelEpoch;
export function subscribeModels(listener: () => void) {
  modelListeners.add(listener);
  return () => {
    modelListeners.delete(listener);
  };
}
const listeners = new Set<() => void>();
const queryListeners = new Set<() => void>();
export const getConfigurationQueryEpoch = () => serial;
export function subscribeConfigurationQueries(listener: () => void) {
  queryListeners.add(listener);
  return () => {
    queryListeners.delete(listener);
  };
}
const projects = new Map<string, number>();
const sourceEpochs = new Map<string, number>();
let identity = "",
  host = "",
  globalVersion = "",
  sources = "",
  serial = 0,
  modelEpoch = 0;
let resetGeneration = false;
const origin = () =>
  typeof location === "undefined" ? "test" : location.origin;
export const getConfigurationSession = () => identity || origin();
export function subscribeConfigurationSession(listener: () => void) {
  listeners.add(listener);
  return () => {
    listeners.delete(listener);
  };
}
export function invalidateConfigurationQueries() {
  serial++;
  queries.clear();
  queryListeners.forEach((listener) => listener());
}
export function invalidateModelCatalogs() {
  modelEpoch++;
  models.clear();
  modelFailures.clear();
  modelListeners.forEach((listener) => listener());
}
function outdatedVersion(version: Row, project?: string) {
  return (
    host === version?.host &&
    (Number(version.global) < Number(globalVersion) ||
      (!!project &&
        version.project > 0 &&
        version.project < (projects.get(project) || 0)) ||
      Object.entries(version.source_epochs || {}).some(
        ([id, epoch]) => Number(epoch) < (sourceEpochs.get(id) || 0),
      ))
  );
}
export function configurationResponseOutdated(value: Row, params: Row) {
  return (
    !!value?.catalog_version &&
    outdatedVersion(
      value.catalog_version,
      value.active_project_id || params.project_id,
    )
  );
}
export function observeConfigurationVersion(version: Row, project?: string) {
  if (!version?.host) return;
  if (outdatedVersion(version, project)) return;
  const changed =
    host !== version.host ||
    globalVersion !== String(version.global) ||
    sources !== version.project_sources ||
    (!!project &&
      version.project > 0 &&
      projects.get(project) !== version.project);
  if (host && host !== version.host) {
    resetGeneration = true;
    invalidateModelCatalogs();
    projects.clear();
    sourceEpochs.clear();
  }
  Object.entries(version.source_epochs || {}).forEach(([id, epoch]) =>
    sourceEpochs.set(id, Number(epoch)),
  );
  host = version.host;
  globalVersion = String(version.global);
  sources = version.project_sources || "";
  if (project && version.project > 0) projects.set(project, version.project);
  if (changed) invalidateConfigurationQueries();
}
export function observeConfigurationResponse(value: Row, params: Row): boolean {
  if (!value || typeof value !== "object") return false;
  if (value.data_root) {
    const next = `${origin()}:${value.data_root}`;
    if (identity !== next) {
      identity = next;
      invalidateConfigurationQueries();
      invalidateModelCatalogs();
      resetGeneration = true;
      listeners.forEach((listener) => listener());
    }
  }
  observeConfigurationVersion(
    value.catalog_version,
    value.active_project_id || params.project_id,
  );
  const reset = resetGeneration && typeof value.catalog_generation === "number";
  if (reset) resetGeneration = false;
  return reset;
}
const cachedMethods = new Set([
  "query.configuration_index",
  "query.configuration_records",
  "query.configuration_usage",
  "query.configuration_defaults",
  "query.project_configuration",
  "query.project_standard_preview",
  "query.project_directory",
  "query.ui_bootstrap",
  "query.standard",
  "query.standard_catalog",
  "query.workflows",
  "query.roles",
  "query.model_providers",
  "query.permission_resources",
  "query.integrations",
  "query.integration_bindings",
  "query.discussion_agents",
]);
function stable(value: unknown): string {
  return JSON.stringify(value, (_key, item) =>
    item && typeof item === "object" && !Array.isArray(item)
      ? Object.fromEntries(
          Object.entries(item).sort(([a], [b]) => a.localeCompare(b)),
        )
      : item,
  );
}
function scope(params: Row) {
  return stable([
    origin(),
    identity,
    host,
    globalVersion,
    sources,
    params.project_id || "global",
    projects.get(params.project_id) || 0,
    serial,
  ]);
}
export async function readConfiguration<T>(
  method: string,
  params: Row,
  load: (signal: AbortSignal) => Promise<T>,
  options: RequestOptions = {},
): Promise<T> {
  if (!host || !cachedMethods.has(method) || options.fresh)
    return load(options.signal || new AbortController().signal);
  for (let attempt = 0; attempt < 4; attempt++) {
    const epoch = serial;
    try {
      const result = await queries.read(
        `${scope(params)}:${method}:${stable(params)}`,
        load,
        options.signal,
      );
      if (epoch !== serial)
        throw new DOMException("Configuration changed", "AbortError");
      return result;
    } catch (error) {
      if (
        options.signal?.aborted ||
        options.prefetch ||
        attempt === 3 ||
        !(error instanceof DOMException) ||
        error.name !== "AbortError"
      )
        throw error;
      await new Promise((resolve) => setTimeout(resolve, 25 * (attempt + 1)));
    }
  }
  throw new Error("配置已变化，请重试");
}
export function configurationMutation(method: string) {
  if (
    [
      "preview",
      "preflight",
      "browse",
      "export",
      "read",
      "list",
      "status",
      "check",
      "validate",
      "diff",
      "resolve",
    ].includes(method.split(".").at(-1) || "")
  )
    return false;
  return (
    /^(configuration|project|permission|integration|role|skill|spec|standard|capability|agent|model|model_provider|model_catalog|harness|sandbox|tool|delivery_type)\./.test(
      method,
    ) ||
    /^workflow\.(draft\.|create|revise|publish|delete|retire|change\.apply)/.test(
      method,
    )
  );
}
export function modelMutation(method: string) {
  if (method.endsWith("status")) return false;
  return /^(credentials\.(configure|authorize)|codex\.auth\.(start|cancel|logout|check)|runtime\.|harness\.|model_provider\.|config\.templates\.)/.test(
    method,
  );
}
if (typeof window !== "undefined")
  window.addEventListener(
    "devdesk:runtime-options-changed",
    invalidateModelCatalogs,
  );
export async function readModelCatalog(
  request: Request,
  provider: string,
  endpoint: string | undefined,
  account: string,
  installationId: string | undefined,
  options: RequestOptions = {},
  recent?: (value: Row) => void,
): Promise<Row> {
  const method = provider === "openai" ? "codex.auth.models" : "models.list";
  const params =
    provider === "openai"
      ? { ...(installationId ? { installation_id: installationId } : {}) }
      : { model_provider: provider, ...(endpoint ? { endpoint } : {}) };
  const key = stable([
    origin(),
    identity,
    host,
    modelEpoch,
    provider,
    endpoint || "",
    account,
    installationId || "",
  ]);
  if (options.fresh) {
    models.clear();
    modelFailures.delete(key);
  }
  const previous = models.peek<Row>(key);
  if (previous) recent?.(previous);
  const failure = modelFailures.get(key);
  if (failure && failure.until > Date.now()) throw failure.error;
  return models.read(
    key,
    async (signal) => {
      try {
        const value = await request(method, params, {
          signal,
          fresh: true,
          shared: true,
        });
        // Cache directory metadata only, never credential or arbitrary provider response fields.
        return {
          source: value.source,
          read_at: Date.now(),
          models: (value.models || []).map((m: Row) => ({
            id: m.model || m.id,
            displayName: m.displayName,
            context_window: m.context_window,
            max_output_tokens: m.max_output_tokens,
          })),
        };
      } catch (error) {
        if (!signal.aborted) {
          if (modelFailures.size >= 16)
            modelFailures.delete(modelFailures.keys().next().value!);
          modelFailures.set(key, { until: Date.now() + 15000, error });
        }
        throw error;
      }
    },
    options.signal,
    5 * 60 * 1000,
  );
}
// Never enqueue model discovery, installation, history, or Standard closure work.
const prefetchQueue = new Map<string, () => Promise<unknown>>();
let prefetchRunning = 0,
  foreground = 0;
export function foregroundRequest(active: boolean) {
  foreground += active ? 1 : -1;
}
function drainPrefetch() {
  if (foreground || prefetchRunning >= 2) {
    if (prefetchQueue.size) setTimeout(drainPrefetch, 100);
    return;
  }
  const next = prefetchQueue.entries().next().value;
  if (!next) return;
  prefetchQueue.delete(next[0]);
  prefetchRunning++;
  void next[1]()
    .catch(() => {})
    .finally(() => {
      prefetchRunning--;
      drainPrefetch();
    });
  if (prefetchQueue.size) setTimeout(drainPrefetch, 0);
}
export function prefetchConfiguration(
  request: Request,
  projectId?: string,
  kind?: string,
) {
  if (!host) return;
  const params = {
    project_id: projectId,
    local: !!projectId,
    limit: 1000,
    offset: 0,
    kind,
    include_history: false,
    include_archived: false,
  };
  const key = `${scope(params)}:index:${kind || "all"}`;
  if (prefetchQueue.size >= 2 || prefetchQueue.has(key)) return;
  prefetchQueue.set(key, () =>
    key === `${scope(params)}:index:${kind || "all"}`
      ? request("query.configuration_index", params, { prefetch: true })
      : Promise.resolve(),
  );
  setTimeout(drainPrefetch, 150);
}

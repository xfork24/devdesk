import { displayText, shortId as short } from "./display-text";
import { WorkflowExecutionRecords } from "./components/WorkflowExecutionRecords";
import { WorkflowContexts } from "./components/WorkflowContexts";
import { RawJsonReader } from "./components/reading";
import { navigateReading, readingKeys } from "./components/reading-state";
import {
  getCatalogGeneration,
  observeCatalogGeneration,
  useCatalogGeneration,
  isTemplateMutation,
} from "./template-catalog";
import { TemplateSettings } from "./components/TemplateSettings";
import {
  ActionButton,
  ActionForm,
  ActionSelect,
} from "./components/ActionFeedback";
import {
  currentActionTarget,
  publishActionFeedback,
  type ActionTarget,
  useActionFeedback,
} from "./components/action-feedback";
import "./browser-compat";
import { ProjectManager } from "./components/ProjectManager";
import {
  FeatureBranchQueue,
  type FeatureQueueItem,
} from "./components/FeatureBranchQueue";
import { ConfigurationScope } from "./components/configuration-scope";
import {
  configurationMutation,
  configurationResponseOutdated,
  foregroundRequest,
  getConfigurationSession,
  invalidateConfigurationQueries,
  invalidateModelCatalogs,
  modelMutation,
  observeConfigurationResponse,
  observeConfigurationVersion,
  prefetchConfiguration,
  readConfiguration,
  subscribeConfigurationSession,
} from "./configuration-client";
import {
  observeRuntimeCommand,
  observeRuntimeEvent,
  readRuntime,
} from "./runtime-state";
import React, {
  useState,
  useEffect,
  useCallback,
  useRef,
  useMemo,
  useSyncExternalStore,
  FormEvent,
} from "react";
import { MarkdownField, MarkdownPreview } from "./components/configuration-ui";
import {
  RpcResponseTimeout,
  type HostVersion,
  type ResourceVersion,
  type Row,
  type Request,
  type RequestOptions,
} from "./components/rpc-types";
import {
  Layers3,
  LayoutDashboard,
  Workflow,
  FileText,
  Library,
  History,
  Settings,
  Plus,
  ArrowRight,
  X,
  Pause,
  ChevronRight,
  FolderOpen,
  CircleDot,
  Activity,
  Box,
  AlertTriangle,
} from "lucide-react";
import { ConfigurationPage } from "./components/ConfigurationPages";
import { CallHistory } from "./components/Integrations";
import { AgentCatalog } from "./components/AgentCatalog";
import { WorkStandard } from "./components/WorkStandard";
import { WorkflowMaterials } from "./components/WorkflowMaterials";
import { WorkflowScope } from "./components/WorkflowScope";
import { RepositorySettings } from "./components/RepositorySettings";
import { GitHubConnection } from "./components/GitHubConnection";
import { RoleConversation } from "./components/RoleConversation";
import { RuntimeSettings } from "./components/RuntimeSettings";
import { RuntimePreparationStatus } from "./components/RuntimePreparationStatus";
import {
  KernelAuthentication,
  KernelConnection,
} from "./components/KernelConnection";

function ConfigurationMain({
  scope,
  children,
  className,
}: {
  scope: React.ContextType<typeof ConfigurationScope>;
  children: React.ReactNode;
  className?: string;
}) {
  return (
    <ConfigurationScope.Provider value={scope}>
      <main className={className}>{displayText(children)}</main>
    </ConfigurationScope.Provider>
  );
}

type Field = {
  key: string;
  label: string;
  value?: string;
  type?: "textarea" | "select" | "number" | "markdown" | "json";
  options?: { value: string; label: string }[];
  optional?: boolean;
  description?: string;
  advanced?: boolean;
  min?: number;
  max?: number;
};
type DialogSpec = {
  title: string;
  method: string;
  target?: string;
  payload?: Row;
  fields: Field[];
  revision?: number;
  preconditions?: ResourceVersion[];
  hostVersion?: HostVersion;
  commandId?: string;
  feedbackTarget?: ActionTarget | null;
};
const labels: Record<string, string> = {
  draft: "草稿",
  completed: "已完成",
  running: "运行中",
  failed: "失败",
  cancelled: "已取消",
  interrupted: "已中断",
  queued: "排队中",
  succeeded: "成功",
  rejected: "已退回",
  pass: "通过",
  changes_requested: "要求修改",
  inconclusive: "不确定",
  active: "使用中",
  released: "已释放",
  cleanup_failed: "清理失败",
};
const pretty = (v: unknown) => JSON.stringify(v, null, 2);
let sequence = 0;
async function rpc(
  method: string,
  params: Row = {},
  options: RequestOptions = {},
): ReturnType<Request> {
  if (isTemplateMutation(method)) {
    if (getCatalogGeneration() === undefined)
      await rpc("query.template_catalog");
    params = { ...params, expected_catalog_generation: getCatalogGeneration() };
  }

  const preparingRuntime = [
    "runtime.prepare",
    "runtime.prepare_harness",
    "runtime.prepare_sandbox",
    "session.open",
    "experience.restore",
    "codex.auth.start",
    "codex.auth.status",
    "codex.auth.cancel",
    "codex.auth.preparation_cancel",
  ].includes(method);
  if (preparingRuntime)
    window.dispatchEvent(
      new CustomEvent("devdesk.runtime-preparation", {
        detail: { pending: true },
      }),
    );
  if (!options.prefetch) foregroundRequest(true);
  try {
    const result = await readRuntime(
      method,
      params,
      (runtimeSignal) =>
        readConfiguration(
          method,
          params,
          async (signal) => {
            const id = ++sequence;
            const traceId = globalThis.crypto
              ?.randomUUID?.()
              .replaceAll("-", "");
            const timeout = AbortSignal.timeout(
              [
                "session.open",
                "experience.restore",
                "runtime.prepare",
              ].includes(method)
                ? 25 * 60 * 1000
                : method === "codex.auth.start" ||
                    method === "configuration.templates.reset"
                  ? 5 * 60 * 1000
                  : 30000,
            );
            let data;
            try {
              const res = await fetch("/rpc", {
                method: "POST",
                headers: {
                  "Content-Type": "application/json",
                  ...(traceId ? { "X-DevDesk-Trace-Id": traceId } : {}),
                },
                body: JSON.stringify({ jsonrpc: "2.0", id, method, params }),
                signal: AbortSignal.any([signal, timeout]),
              });
              if (!res.ok) throw new Error(`HTTP ${res.status} · 内核连接失败`);
              data = await res.json();
            } catch (error) {
              if (timeout.aborted && !signal.aborted) {
                const message = method.startsWith("query.")
                  ? "读取超时，可重新读取"
                  : "内核响应超时，操作结果待确认";
                throw new RpcResponseTimeout(
                  `${message} · ${method} · 请求 ${traceId || id}`,
                  {
                    cause: error,
                  },
                );
              }
              throw error;
            }
            if (data.error)
              throw new Error(
                `${data.error.data?.code || ""} · ${data.error.message}`,
              );
            if (
              method.startsWith("query.") &&
              configurationResponseOutdated(data.result, params)
            )
              throw new DOMException("Configuration changed", "AbortError");
            return data.result;
          },
          { ...options, signal: runtimeSignal },
        ),
      options,
      getConfigurationSession(),
    );
    const reset = observeConfigurationResponse(result, params);
    observeCatalogGeneration(
      result?.catalog_generation ??
        (["query.template_catalog", "configuration.templates.reset"].includes(
          method,
        )
          ? result?.generation
          : undefined),
      reset,
    );
    observeRuntimeCommand(method, params);
    if (configurationMutation(method)) invalidateConfigurationQueries();
    if (modelMutation(method)) invalidateModelCatalogs();
    return result;
  } finally {
    if (preparingRuntime)
      window.dispatchEvent(
        new CustomEvent("devdesk.runtime-preparation", {
          detail: { pending: false },
        }),
      );
    if (!options.prefetch) foregroundRequest(false);
  }
}
function Badge({ value }: { value: string }) {
  return (
    <span className={`badge status-${value}`}>
      {displayText(labels[value] || value)}
    </span>
  );
}
function Empty({ children }: { children: React.ReactNode }) {
  return (
    <div className="empty">
      <Box size={26} />
      <p>{displayText(children)}</p>
    </div>
  );
}
const route = new URLSearchParams(location.search);
function routeTabValue(p: URLSearchParams) {
  const original = p.toString();
  const tab = p.get("tab");
  if (tab === "workflows" && p.has("workflow_id"))
    p.set("config_tab", "workflow");
  if (original !== p.toString()) history.replaceState(null, "", `?${p}`);
  return [
    "overview",
    "projects",
    "workflows",
    "actors",
    "discussion",
    "components",
    "settings",
    "execution",
    "history",
  ].includes(tab || "")
    ? tab!
    : "overview";
}
const routeTab = routeTabValue(route);
const defaultExecutionTab = "manager";
type ActivityFilter = "running" | "waiting_confirmation" | "blocked";
const executionTabs = [
  ["manager", "项目经理"],
  ["overview", "工作流程"],
  ["documents", "材料"],
  ["context", "上下文"],
  ["records", "执行记录"],
];

function App() {
  const catalogGeneration = useCatalogGeneration();
  const requestGeneration = useRef(0);
  const activePage = useRef({
    tab: routeTab,
    featureId: "",
    taskFilter: "",
    executionTab: "",
  });
  const configurationVersion = useRef("");
  const runtimeView = useRef<
    | {
        roots: Set<string>;
        contexts: Set<string>;
      }
    | undefined
  >(undefined);
  const workspaceLoads = useRef(0);
  const workspaceRefreshQueued = useRef(false);
  const refreshWorkspace = useRef<() => void>(() => {});
  const projectGeneration = useRef(0);
  const activityRequest = useRef<AbortController | null>(null);
  const kernelStopped = useRef(false);
  const [detailPages, setDetailPages] = useState<Record<string, number>>({});
  const [loadedData, setData] = useState<Row>();
  const configurationReadVersion = JSON.stringify(loadedData?.catalog_version);
  // Refresh editor reads on a committed configuration change while retaining their draft/feedback state.
  const [caps, setCaps] = useState<Row>();
  const [projectId, setProjectId] = useState(route.get("project_id") || "");
  const [featureId, setFeatureId] = useState(route.get("feature_id") || "");
  const [tab, setTab] = useState(routeTab);
  const [branchQueueRevision, setBranchQueueRevision] = useState(0);
  const configurationSession = useSyncExternalStore(
    subscribeConfigurationSession,
    getConfigurationSession,
  );
  const configurationLifetime = useMemo(
    () => ({
      scope: `${tab}:${projectId}:${configurationSession}`,
      controller: new AbortController(),
    }),
    [tab, projectId, configurationSession],
  );
  const configurationReader = useMemo(() => {
    const controller = configurationLifetime.controller;
    return {
      scope: `${configurationLifetime.scope}:${configurationReadVersion}`,
      controller,
      request: (
        method: string,
        params: Row = {},
        options: RequestOptions = {},
      ) =>
        rpc(
          method,
          params,
          options.shared ||
            !/^(query\.|models\.list|codex\.auth\.models)/.test(method)
            ? options
            : {
                ...options,
                signal: options.signal
                  ? AbortSignal.any([options.signal, controller.signal])
                  : controller.signal,
              },
        ),
    };
  }, [configurationLifetime, configurationReadVersion]);
  useEffect(
    () => () => configurationLifetime.controller.abort(),
    [configurationLifetime],
  );
  useEffect(
    () => () => activityRequest.current?.abort(),
    [tab, projectId, configurationSession],
  );
  const configurationRequest = configurationReader.request;
  const configurationScope = useMemo(
    () => ({
      request: configurationRequest,
      projectId: projectId || undefined,
    }),
    [configurationRequest, projectId],
  );
  const [error, setError] = useState("");
  const [connection, setConnection] = useState("连接中");
  const [dialog, setDialog] = useState<DialogSpec>();
  const [busy, setBusy] = useState(false);
  const mutationInFlight = useRef(false);
  const [historyScope, setHistoryScope] = useState(
    route.get("history_scope") === "global" ? "global" : "project",
  );
  const [featurePage, setFeaturePage] = useState(0);
  const [projectPage, setProjectPage] = useState(0);
  const [view, setView] = useState<{
    title: string;
    body: string;
    format?: "markdown";
    metadata?: Row;
  }>();
  const [setupPath, setSetupPath] = useState("./data");
  const [focusedSession, setFocusedSession] = useState<Row | undefined>(() =>
    route.get("session_id") && route.get("tab") === "discussion"
      ? {
          id: route.get("session_id"),
          owner_id:
            route.get("session_scope") === "global"
              ? "global"
              : route.get("task_id") ||
                route.get("feature_id") ||
                route.get("project_id"),
          project_id: route.get("project_id") || "",
          turn_id: route.get("turn_id"),
        }
      : undefined,
  );
  const discussionFocus =
    tab === "discussion" && focusedSession?.project_id === projectId
      ? focusedSession
      : undefined;
  const [taskFilter, setTaskFilter] = useState(route.get("task_id") || "");
  const [readingTarget, setReadingTarget] = useState<Row>(() => ({
    ...Object.fromEntries(
      readingKeys.map((key) => [key, route.get(key) || ""]),
    ),
    _project: route.get("project_id") || "",
    _feature: route.get("feature_id") || "",
  }));
  const [executionTab, setExecutionTab] = useState(
    route.get("detail") ||
      (route.get("tab") === "documents" ? "documents" : defaultExecutionTab),
  );
  activePage.current = { tab, featureId, taskFilter, executionTab };
  const workspaceFeatureId = tab === "overview" ? "" : featureId;
  const summaryPage =
    tab === "execution" &&
    ["manager", "overview"].includes(executionTab) &&
    !!projectId;
  // Summary DTOs are display-only. Hide them while navigating to a view that
  // needs exact details; keep header metadata and editor lifetimes stable.
  const data =
    loadedData?.kind === "execution_workspace" && !summaryPage
      ? undefined
      : loadedData;
  if (data && (!featureId || data.detail?.feature?.id === featureId)) {
    const owners = new Set([
      featureId,
      ...(data.detail?.tasks || []).map((task: Row) => task.id),
    ]);
    runtimeView.current = Array.isArray(data.configurations)
      ? {
          roots: new Set(
            data.configurations
              .filter(
                (record: Row) =>
                  record.type === "workflow_instance" &&
                  (!featureId || owners.has(record.data?.owner_id)),
              )
              .map((record: Row) => record.data?.root_instance_id || record.id),
          ),
          contexts: new Set(
            (data.detail?.contexts || []).map((context: Row) => context.id),
          ),
        }
      : undefined;
  } else runtimeView.current = undefined;
  const activeReadingTarget =
    readingTarget._project === projectId && readingTarget._feature === featureId
      ? readingTarget
      : {};
  const [workFilter, setWorkFilter] = useState(route.get("filter") || "");
  useEffect(() => {
    const params = new URLSearchParams();
    for (const key of [
      "config_tab",
      "config_object",
      "workflow_id",
      "workflow_node",
      "workflow_port",
      "tool_ref",
      "standard_id",
      "permission_ref",
    ]) {
      const value = new URLSearchParams(location.search).get(key);
      if (value) params.set(key, value);
    }
    if (
      !projectId ||
      (readingTarget._project === projectId &&
        readingTarget._feature === featureId)
    ) {
      for (const key of readingKeys)
        if (readingTarget[key]) params.set(key, readingTarget[key]);
    }
    params.set("tab", tab);
    if (projectId) params.set("project_id", projectId);
    if (featureId) params.set("feature_id", featureId);
    if (taskFilter) params.set("task_id", taskFilter);
    if (discussionFocus) {
      params.set("session_id", discussionFocus.id);
      if (discussionFocus.turn_id)
        params.set("turn_id", discussionFocus.turn_id);
      else params.delete("turn_id");
      if (discussionFocus.owner_id === "global")
        params.set("session_scope", "global");
    }
    if (tab === "execution") params.set("detail", executionTab);
    if (tab === "history") params.set("history_scope", historyScope);
    if (workFilter) params.set("filter", workFilter);
    if (featurePage) params.set("page", String(featurePage));
    const url = `?${params}`;
    if (location.search !== url) {
      const previous = new URLSearchParams(location.search),
        canonical = new URLSearchParams(params);
      previous.sort();
      canonical.sort();
      if (previous.toString() === canonical.toString())
        window.history.replaceState(null, "", url);
      else window.history.pushState(null, "", url);
    }
  }, [
    tab,
    projectId,
    featureId,
    taskFilter,
    executionTab,
    readingTarget,
    workFilter,
    featurePage,
    historyScope,
    discussionFocus,
  ]);
  useEffect(() => {
    const pop = () => {
      const p = new URLSearchParams(location.search);
      const pid = p.get("project_id") || "";
      if (pid !== projectId) {
        ++requestGeneration.current;
        ++projectGeneration.current;
        setProjectId(pid);
        setData(undefined);
        setView(undefined);
      }
      setTab(routeTabValue(p));
      setHistoryScope(
        p.get("history_scope") === "global" ? "global" : "project",
      );
      setFeatureId(p.get("feature_id") || "");
      setTaskFilter(p.get("task_id") || "");
      setExecutionTab(p.get("detail") || defaultExecutionTab);
      setReadingTarget({
        ...Object.fromEntries(
          readingKeys.map((key) => [key, p.get(key) || ""]),
        ),
        _project: p.get("project_id") || "",
        _feature: p.get("feature_id") || "",
      });
      setFocusedSession(
        p.get("tab") === "discussion" && p.get("session_id")
          ? {
              id: p.get("session_id"),
              owner_id:
                p.get("session_scope") === "global"
                  ? "global"
                  : p.get("task_id") || p.get("feature_id") || pid,
              project_id: pid,
              turn_id: p.get("turn_id"),
            }
          : undefined,
      );
      setWorkFilter(p.get("filter") || "");
      setFeaturePage(Number(p.get("page") || 0));
    };
    window.addEventListener("popstate", pop);
    return () => window.removeEventListener("popstate", pop);
  }, [projectId]);
  const load = useCallback(
    async (afterCommand = false) => {
      // A command refresh must settle before another form captures its revision.
      // SSE refreshes during the mutation are covered by that final read.
      if (kernelStopped.current || (mutationInFlight.current && !afterCommand))
        return;
      workspaceLoads.current++;
      const generation = ++requestGeneration.current;
      try {
        let response = await fetch("/capabilities", {
          signal: AbortSignal.timeout(10000),
        });
        if (response.status === 401) {
          // A restarted local kernel can establish a fresh session without a
          // key; a protected remote kernel instead presents its login form.
          const session = await fetch("/session", {
            method: "POST",
            headers: { "Content-Type": "application/json" },
            body: "{}",
            signal: AbortSignal.timeout(10000),
          });
          if (session.ok)
            response = await fetch("/capabilities", {
              signal: AbortSignal.timeout(10000),
            });
        }
        if (generation !== requestGeneration.current) return;
        if (response.status === 401) {
          setCaps({ authentication_required: true });
          setData(undefined);
          setConnection("等待认证");
          return;
        }
        if (!response.ok)
          throw new Error(`无法连接内核 · HTTP ${response.status}`);
        const c = await response.json();
        if (c.protocol_version !== 1 && c.protocol_version !== undefined)
          throw new Error("内核协议版本不兼容，请更新后重试。");
        if (generation !== requestGeneration.current) return;
        setCaps(c);
        setConnection("已连接");
        if (!c.setup_required) {
          const configurationPage =
            ["projects", "workflows", "actors", "components"].includes(tab) ||
            (tab === "discussion" && !workspaceFeatureId && !taskFilter);
          const d = await rpc(
            summaryPage
              ? "query.execution_workspace"
              : configurationPage
                ? "query.ui_bootstrap"
                : "query.workspace",
            summaryPage
              ? {
                  project_id: projectId,
                  feature_id: workspaceFeatureId || undefined,
                  offset: featurePage * 50,
                  task_offset: (detailPages.tasks || 0) * 50,
                  project_offset: projectPage * 50,
                  github_only: true,
                  limit: 50,
                  work_filter: workFilter,
                }
              : {
                  project_id: projectId || undefined,
                  feature_id: workspaceFeatureId || undefined,
                  offset: featurePage * 50,
                  project_offset: projectPage * 50,
                  github_only: true,
                  limit: 50,
                  detail_pages: detailPages,
                  work_filter: workFilter,
                  task_id: taskFilter || undefined,
                },
          );
          if (generation !== requestGeneration.current) return;
          if (d.catalog_version)
            configurationVersion.current = JSON.stringify(d.catalog_version);
          setData(d);
        }
      } catch (e) {
        if (
          generation === requestGeneration.current &&
          projectId &&
          e instanceof Error &&
          e.message.startsWith("not_found ·")
        ) {
          ++requestGeneration.current;
          ++projectGeneration.current;
          setProjectId("");
          setFeatureId("");
          setTaskFilter("");
          setData(undefined);
          setView(undefined);
          const url = new URL(location.href);
          for (const key of [
            "project_id",
            "feature_id",
            "task_id",
            "session_id",
          ])
            url.searchParams.delete(key);
          history.replaceState(null, "", `${url.pathname}${url.search}`);
          return;
        }
        if (generation === requestGeneration.current) {
          setError(String(e));
          setConnection("连接失败");
        }
      } finally {
        workspaceLoads.current--;
        if (!workspaceLoads.current && workspaceRefreshQueued.current) {
          workspaceRefreshQueued.current = false;
          queueMicrotask(() => refreshWorkspace.current());
        }
      }
    },
    [
      projectId,
      workspaceFeatureId,
      featurePage,
      projectPage,
      detailPages,
      workFilter,
      taskFilter,
      tab,
      summaryPage,
    ],
  );
  refreshWorkspace.current = () => {
    // Coalesce events while a read is in flight. Starting another generation
    // here would discard every response when workflow events arrive faster.
    if (workspaceLoads.current) {
      workspaceRefreshQueued.current = true;
      return;
    }
    void load();
  };
  useEffect(() => {
    load();
  }, [load]);
  const datasetOpening = caps?.lifecycle?.status === "initializing";
  useEffect(() => {
    if (!datasetOpening) return;
    const timer = setInterval(() => refreshWorkspace.current(), 1000);
    return () => clearInterval(timer);
  }, [datasetOpening]);
  useEffect(() => {
    if (caps?.setup_required !== false) return;
    let timer: ReturnType<typeof setTimeout> | undefined;
    let branchFactsChanged = false;
    const stream = new EventSource(
      `/events?tail=true&configuration=true${projectId ? `&project_id=${encodeURIComponent(projectId)}` : ""}`,
    );
    stream.onopen = () => {
      if (!kernelStopped.current) setConnection("已连接");
    };
    stream.onmessage = (message) => {
      try {
        const value: Row = JSON.parse(message.data);
        if (
          activePage.current.tab === "overview" &&
          ["stream.ready", "stream.reset"].includes(value.kind)
        )
          branchFactsChanged = true;
        observeConfigurationVersion(value.configuration_version, projectId);
        observeRuntimeEvent(projectId, value);
        let resourceShellChanged = false;
        let executionContentChanged = false;
        if (value.kind === "workflow.runtime") {
          const source = JSON.parse(value.detail);
          executionContentChanged =
            !runtimeView.current ||
            runtimeView.current.roots.has(
              source.root_instance_id || source.instance_id,
            );
        }
        let executionShellChanged =
          ["stream.ready", "stream.reset", "workflow.start"].includes(
            value.kind,
          ) ||
          /^(project|repository|workspace|settings)\./.test(value.kind || "");
        if (value.kind === "resource.changed") {
          const versions: Row[] = JSON.parse(value.detail);
          if (activePage.current.tab === "overview")
            branchFactsChanged ||= versions.some(
              ({ resource }) =>
                ["feature", "task", "repository_worktree"].includes(
                  resource?.kind,
                ) ||
                (resource?.kind === "governance" &&
                  /^(workflow-delivery|platform-publish-stage):/.test(
                    resource.id,
                  )),
            );
          executionContentChanged ||= versions.some(
            ({ resource }) =>
              resource?.kind === "interaction" ||
              (resource?.kind === "context" &&
                (!runtimeView.current ||
                  runtimeView.current.contexts.has(resource.id))),
          );
          executionShellChanged ||= versions.some(({ resource }) =>
            [
              "feature",
              "task",
              "project",
              "repository_operation",
              "repository_worktree",
              "head",
              "source_definition",
              "source_head",
            ].includes(resource?.kind),
          );
          resourceShellChanged = versions.some(
            ({ resource }) =>
              resource?.kind === "runtime_installation" ||
              (resource?.kind === "setting" &&
                [
                  "global_concurrency",
                  "default_project_concurrency",
                  "default_harness",
                ].includes(resource.id)),
          );
          if (
            versions.some(
              ({ resource }) =>
                resource?.kind === "runtime_installation" ||
                (resource?.kind === "setting" &&
                  (resource.id.startsWith("credentials.dsh.") ||
                    resource.id === "default_harness")),
            )
          )
            invalidateModelCatalogs();
        }
        if (
          value.kind === "stream.reset" ||
          /^(project|repository|workspace|runtime|settings)\./.test(
            value.kind || "",
          )
        )
          invalidateConfigurationQueries();
        if (
          value.kind === "stream.reset" ||
          /^(runtime|credentials|codex\.auth|harness|model_provider|model_catalog)\./.test(
            value.kind || "",
          )
        )
          invalidateModelCatalogs();
        const page = activePage.current;
        if (document.visibilityState === "hidden") return;
        if (
          page.tab === "execution" &&
          !executionShellChanged &&
          (["manager", "overview"].includes(page.executionTab) ||
            !executionContentChanged)
        )
          return;
        if (
          ["projects", "workflows", "actors", "components"].includes(
            page.tab,
          ) ||
          (page.tab === "discussion" && !page.featureId && !page.taskFilter)
        ) {
          const version = value.configuration_version
            ? JSON.stringify(value.configuration_version)
            : "";
          const changed = version && version !== configurationVersion.current;
          if (
            !changed &&
            !resourceShellChanged &&
            !["stream.ready", "stream.reset"].includes(value.kind) &&
            !/^(project|repository|workspace|runtime|settings)\./.test(
              value.kind || "",
            )
          )
            return;
        }
      } catch {
        /* Older or interrupted event payloads still trigger a refresh. */
      }
      if (!timer)
        timer = setTimeout(() => {
          timer = undefined;
          if (branchFactsChanged) {
            branchFactsChanged = false;
            setBranchQueueRevision((revision) => revision + 1);
          }
          refreshWorkspace.current();
        }, 180);
    };
    stream.onerror = () => {
      if (!kernelStopped.current) setConnection("连接中断 · 自动重连");
    };
    const visible = () => {
      if (document.visibilityState === "hidden") return;
      if (activePage.current.tab === "overview")
        setBranchQueueRevision((revision) => revision + 1);
      refreshWorkspace.current();
    };
    document.addEventListener("visibilitychange", visible);
    return () => {
      stream.close();
      document.removeEventListener("visibilitychange", visible);
      if (timer) clearTimeout(timer);
    };
  }, [projectId, caps?.setup_required]);
  const detail = data?.detail;
  const f = detail?.feature;
  const projects: Row[] = (loadedData?.projects || []).filter(
    (p: Row) => p.workspace_definition?.repository,
  );
  const features: FeatureQueueItem[] = data?.features || [];
  const definitions: Row[] = (data?.definitions || []).filter(
    (d: Row) => d.family !== "agent" || d.agent_profile,
  );
  const project = loadedData?.active_project?.workspace_definition?.repository
    ? loadedData.active_project
    : undefined;
  const repositoryReady = loadedData?.repository_ready === true;
  const projectOptions =
    project && !projects.some((p) => p.id === project.id)
      ? [project, ...projects]
      : projects;
  const overviewProject = projectId || project?.id || "";
  const openOverviewRoute = (feature?: string) => {
    activityRequest.current?.abort();
    setDetailPages({});
    const params = new URLSearchParams({
      tab: feature ? "execution" : "overview",
    });
    if (overviewProject) params.set("project_id", overviewProject);
    if (feature) {
      params.set("feature_id", feature);
      params.set("detail", defaultExecutionTab);
    }
    const url = `?${params}`;
    if (location.search !== url) navigateReading(url);
  };
  const openFeatureByActivity = async (filter: ActivityFilter) => {
    if (!overviewProject) return;
    activityRequest.current?.abort();
    const request = new AbortController();
    activityRequest.current = request;
    const { signal } = request;
    const source = location.search;
    const target = currentActionTarget();
    publishActionFeedback(target, "");
    try {
      const results = await rpc(
        "query.execution_workspace",
        {
          project_id: overviewProject,
          work_filter: filter,
          limit: 1,
        },
        { signal, fresh: true },
      );
      if (signal.aborted || source !== location.search) return;
      const match = results.features[0];
      if (match) openOverviewRoute(match.id);
      else publishActionFeedback(target, "当前没有符合此状态的 Feature。");
    } catch (error) {
      if (!signal.aborted) publishActionFeedback(target, String(error), true);
    } finally {
      if (activityRequest.current === request) activityRequest.current = null;
    }
  };
  const resetProjectView = (id: string) => {
    activityRequest.current?.abort();
    ++requestGeneration.current;
    ++projectGeneration.current;
    setProjectId(id);
    setFocusedSession(undefined);
    setFeatureId("");
    setFeaturePage(0);
    setDetailPages({});
    setTaskFilter("");
    setView(undefined);
    setData(undefined);
  };
  const openProject = (id: string) => {
    resetProjectView(id);
    setDialog(undefined);
  };
  const pager = (key: string) => {
    const page = detailPages[key] || 0;
    const size = key === "runs" ? 250 : 50;
    return (
      detail &&
      (page > 0 || detail.totals?.[key] > size) && (
        <Pagination
          page={page}
          setPage={(next) =>
            setDetailPages((current) => ({
              ...current,
              [key]: next,
            }))
          }
          hasMore={(page + 1) * size < detail.totals[key]}
        />
      )
    );
  };
  const getRevision = (target?: string) => {
    if (!target) return undefined;
    return [
      f,
      project,
      ...projects,
      ...(detail?.tasks || []),
      ...(detail?.contexts || []),
    ].find((x) => x?.id === target)?.revision;
  };
  const show = (spec: DialogSpec) =>
    setDialog({
      ...spec,
      revision: spec.revision ?? getRevision(spec.target),
      hostVersion: caps?.lifecycle,
      commandId: crypto.randomUUID(),
      preconditions:
        spec.method === "settings.update"
          ? (data?.settings_versions || []).filter((version: ResourceVersion) =>
              spec.fields.some((field) => field.key === version.resource.id),
            )
          : spec.preconditions,
      feedbackTarget: spec.feedbackTarget ?? currentActionTarget(),
    });
  const command = async (
    method: string,
    target?: string,
    payload: Row = {},
    revision?: number,
    feedback = true,
  ) => {
    const feedbackTarget = currentActionTarget();
    if (feedback) publishActionFeedback(feedbackTarget, "");
    mutationInFlight.current = true;
    setBusy(true);
    setError("");
    try {
      const result = await rpc(method, {
        project_id: projectId || undefined,
        command_id: crypto.randomUUID(),
        target: target || null,
        expected_revision: revision ?? getRevision(target) ?? null,
        payload,
      });
      if (feedback) publishActionFeedback(feedbackTarget, "操作已保存");
      if (method === "project.open_github") {
        resetProjectView(result.id);
        setTab("projects");
      } else await load(true);
      return result;
    } catch (e) {
      if (feedback && !publishActionFeedback(feedbackTarget, String(e), true))
        setError(String(e));
      throw e;
    } finally {
      mutationInFlight.current = false;
      setBusy(false);
    }
  };
  const createProject = () =>
    show({
      title: "打开 GitHub 项目",
      method: "project.open_github",
      fields: [
        { key: "repository", label: "GitHub 仓库（owner/repo 或 URL）" },
        { key: "mainline", label: "主分支", value: "main" },
        { key: "title", label: "项目名称", optional: true },
      ],
    });
  const createFeature = async () => {
    const target = currentActionTarget();
    const generation = projectGeneration.current;
    try {
      const configuration = data?.configurations
        ?.filter((r: Row) => r.type === "project_configuration")
        .sort((a: Row, b: Row) => b.version - a.version)[0];
      const workflow = configuration?.data?.workflow_ref
        ? await rpc("query.workflow", {
            id: configuration.data.workflow_ref,
            project_id: project?.id,
          })
        : undefined;
      const declaredInputs = Object.entries(workflow?.data?.inputs || {});
      const choices: Row[] = [];
      for (let offset = 0; ;) {
        const page = await rpc("query.workspace", {
          project_id: project?.id,
          offset,
          limit: 100,
        });
        choices.push(...page.features);
        offset += page.features.length;
        if (offset >= page.total_features || !page.features.length) break;
      }
      if (generation !== projectGeneration.current) return;
      show({
        title: "创建 Feature",
        method: "feature.create",
        feedbackTarget: target,
        payload: {
          project_id: project?.id,
          require_configuration: true,
          start_workflow: false,
          configuration_revision_id: configuration?.id,
        },
        fields: [
          { key: "title", label: "Feature 名称" },
          {
            key: "initial_requirement",
            label: "初始需求",
            type: "textarea",
            description:
              "用自然语言描述你想解决的问题和期望结果。Agent 会按 Spec 判断是否需要进一步澄清。",
          },
          ...(declaredInputs.length
            ? [
                {
                  key: "workflow_inputs",
                  label: "流程输入（JSON）",
                  type: "json" as const,
                  value: "{}",
                  optional: true,
                  advanced: true,
                  description: `此流程声明的输入：${declaredInputs.map(([name, port]) => `${name}（${(port as Row).required === false ? "可选" : "必填"}，${(port as Row).delivery_ref}）`).join("；")}。各字段值须符合对应交付类型。`,
                },
              ]
            : []),
          {
            key: "predecessor",
            label: "关联前序 Feature（可选）",
            optional: true,
            type: "select",
            value: "",
            description:
              "用于记录同项目 Feature 的前后关联；新 Feature 的代码基线仍为项目主干。",
            options: [
              { value: "", label: "不关联前序 Feature" },
              ...choices.map((f) => ({
                value: f.id,
                label: `${f.title} · ${labels[f.status] || f.status} · ${short(f.id)}`,
              })),
            ],
          },
        ],
      });
    } catch (e) {
      publishActionFeedback(target, String(e), true);
    }
  };
  const nav = [
    ["overview", "工作总览", LayoutDashboard],
    ["execution", "执行与交付", Workflow],
    ["discussion", "角色讨论", FileText],
    ["projects", "项目管理", Layers3],
    ["workflows", "流程与规范", Workflow],
    ["actors", "角色与代理", Library],
    ["components", "基础组件", Settings],
    ["history", "历史记录", History],
    ["settings", "设置", Settings],
  ] as const;
  if (caps?.authentication_required)
    return (
      <div className="setup">
        <div className="setup-card">
          <div className="brand">
            <Layers3 />
            <strong>DevDesk</strong>
          </div>
          <KernelAuthentication
            authenticated={async () => {
              setError("");
              await load();
            }}
          />
          {error && (
            <div className="error" role="alert">
              {displayText(error)}
            </div>
          )}
          <KernelConnection />
        </div>
      </div>
    );
  if (caps?.setup_required)
    return (
      <div className="setup">
        <div className="setup-card">
          <div className="brand">
            <Layers3 />
            <strong>DevDesk</strong>
            <span>WORKBENCH 0.3</span>
          </div>
          <h1>为工程建立一个工作空间</h1>
          <p>
            选择当前内核所在主机的 data
            目录。工程状态、文档和执行历史将保存在这里。
          </p>
          <div className="notice">
            打开 GitHub 仓库开始工作，所需 Harness 运行环境由 DevDesk 按需准备
          </div>
          <ActionForm
            onSubmit={async (e) => {
              e.preventDefault();
              if (datasetOpening || busy) return;
              setError("");
              try {
                setBusy(true);
                const commandId = crypto.randomUUID();
                const r = await fetch("/setup", {
                  method: "POST",
                  headers: { "Content-Type": "application/json" },
                  body: JSON.stringify({
                    path: setupPath,
                    command_id: commandId,
                    instance_id: caps.lifecycle?.instance_id,
                    expected_revision: caps.lifecycle?.revision,
                  }),
                }).then((r) => r.json());
                if (r.lifecycle)
                  setCaps((current) => ({
                    ...current,
                    lifecycle: r.lifecycle,
                  }));
                if (r.lifecycle?.status === "ready") {
                  await load();
                  return;
                }
                if (r.lifecycle?.status === "initializing") return;
                if (r.error) throw new Error(`${r.code || ""} · ${r.error}`);
                await load();
              } catch (e) {
                setError(String(e));
              } finally {
                setBusy(false);
              }
            }}
          >
            <label>
              数据目录
              <input
                value={setupPath}
                onChange={(e) => setSetupPath(e.target.value)}
                disabled={busy || datasetOpening}
                required
              />
            </label>
            <p className="muted">
              支持空目录或已有 DevDesk 数据集。切换目录需重启程序。
            </p>
            <ActionButton className="primary" disabled={busy || datasetOpening}>
              初始化 / 打开数据集 <ArrowRight size={16} />
            </ActionButton>
          </ActionForm>
          {(datasetOpening || busy) && (
            <p className="muted" role="status">
              正在打开数据集，完成后将自动进入工作台。
            </p>
          )}
          {error && (
            <div className="error" role="alert">
              {displayText(error)}
            </div>
          )}
          <KernelConnection />
        </div>
      </div>
    );
  return (
    <div className="app">
      <aside className="sidebar">
        <div className="brand">
          <Layers3 size={26} />
          <strong>DevDesk</strong>
        </div>
        <div className="sidebar-caption">
          软件工程工作台 <span>v0.3</span>
        </div>
        <div className="project-selector">
          <span>当前项目</span>
          <ActionSelect
            aria-label="当前项目"
            value={projectId}
            disabled={busy}
            onChange={(e) => openProject(e.target.value)}
          >
            <option value="">
              {projects.length === 0 ? "尚无项目" : "选择项目"}
            </option>
            {projectOptions.map((p) => (
              <option key={p.id} value={p.id}>
                {displayText(p.title)}
              </option>
            ))}
          </ActionSelect>
          <ActionButton aria-label="打开 GitHub 项目" onClick={createProject}>
            <Plus size={16} />
          </ActionButton>
        </div>
        {(data?.total_projects || 0) > 50 && (
          <Pagination
            page={projectPage}
            setPage={(page) => {
              setProjectPage(page);
            }}
            hasMore={(projectPage + 1) * 50 < (data?.total_projects || 0)}
          />
        )}
        <nav>
          {nav.map(([id, title, Icon]) => (
            <ActionButton
              key={id}
              className={tab === id ? "nav active" : "nav"}
              onClick={() => {
                if (id === "execution") setExecutionTab(defaultExecutionTab);
                setTab(id);
              }}
              onMouseEnter={() => {
                if (
                  [
                    "projects",
                    "workflows",
                    "actors",
                    "components",
                    "discussion",
                  ].includes(id)
                )
                  prefetchConfiguration(
                    rpc,
                    projectId || undefined,
                    id === "actors" ? "role" : undefined,
                  );
              }}
              onFocus={() => {
                if (
                  [
                    "projects",
                    "workflows",
                    "actors",
                    "components",
                    "discussion",
                  ].includes(id)
                )
                  prefetchConfiguration(
                    rpc,
                    projectId || undefined,
                    id === "actors" ? "role" : undefined,
                  );
              }}
            >
              <Icon size={18} />
              {displayText(title)}
            </ActionButton>
          ))}
        </nav>
        <div className="sidebar-bottom">
          <div className="runtime-dot" />
          <div>
            <strong>
              {displayText(project ? "GitHub 项目" : "打开 GitHub 仓库")}
            </strong>
            <p>工程流程与角色讨论</p>
          </div>
        </div>
      </aside>
      <div className="main">
        <header>
          <div className="breadcrumb">
            <FolderOpen size={15} />
            <span>{displayText(project?.title || "工作空间")}</span>
            <ChevronRight size={14} />
            <span>{displayText(f?.title || "总览")}</span>
          </div>
          <div className="header-right">
            <span
              className={`connection ${connection === "已连接" ? "online" : ""}`}
            >
              <CircleDot size={12} />
              {displayText(connection)}
            </span>
            <span className="repository-status">
              {displayText(
                project
                  ? repositoryReady
                    ? "GitHub 已接入"
                    : "仓库接入中"
                  : "尚未打开仓库",
              )}
            </span>
          </div>
        </header>
        <ConfigurationMain
          scope={configurationScope}
          className={
            tab === "execution"
              ? "execution-page"
              : tab === "settings"
                ? "settings-page"
                : undefined
          }
        >
          {error && (
            <div className="error" role="alert">
              <AlertTriangle size={16} />
              <span>{displayText(error)}</span>
              <ActionButton aria-label="关闭错误" onClick={() => setError("")}>
                <X size={16} />
              </ActionButton>
            </div>
          )}
          {data && <RuntimePreparationStatus request={rpc} />}
          {!project && data && (
            <div className="notice">
              打开 GitHub 仓库开始工作。
              <ActionButton onClick={createProject}>
                打开 GitHub 项目
              </ActionButton>
            </div>
          )}
          {project && !repositoryReady && tab !== "settings" && (
            <RepositorySettings
              key={`repository-status:${project.id}`}
              project={project}
              ready={repositoryReady}
              request={rpc}
              refresh={load}
              onOpenConnection={() => setTab("settings")}
            />
          )}
          {project &&
            repositoryReady &&
            data?.configuration_ready === false &&
            tab !== "projects" && (
              <div className="notice">
                项目待配置：已提供默认 Workflow、Role Agent
                和规范，可在项目管理中采用或调整。
                <ActionButton onClick={() => setTab("projects")}>
                  配置项目
                </ActionButton>
              </div>
            )}
          <div className="page-heading">
            <div>
              <span className="eyebrow">ENGINEERING WORKSPACE</span>
              <h1>{displayText(nav.find((n) => n[0] === tab)?.[1])}</h1>
              <p>
                {displayText(
                  tab === "overview"
                    ? "从需求到交付，每一次决定都有依据。"
                    : tab === "execution"
                      ? "运行、验证与评审，由领域规则驱动。"
                      : tab === "documents"
                        ? "可编辑的上下文，不可变的交付版本。"
                        : tab === "workflows"
                          ? "定义角色交互、开发步骤、门禁和推进策略。"
                          : tab === "definitions"
                            ? "锁定规范与身份，为每次执行建立确定起点。"
                            : tab === "history"
                              ? "持久记录命令、执行边界与关键决定。"
                              : "内核数据与执行能力。",
                )}
              </p>
            </div>
            <div className="actions">
              {tab === "overview" && (
                <>
                  <ActionButton onClick={createProject}>
                    <Plus size={16} /> 打开 GitHub 项目
                  </ActionButton>
                  <ActionButton
                    className="primary"
                    disabled={
                      !repositoryReady || !data?.configuration_ready || busy
                    }
                    onClick={createFeature}
                  >
                    <Plus size={16} /> 新 Feature
                  </ActionButton>
                </>
              )}
            </div>
          </div>
          {!data && (
            <section className="panel">
              <Empty>
                {displayText(
                  connection === "已断开"
                    ? "内核已退出，请连接其他内核或重新启动后连接。"
                    : error
                      ? "当前内核无法连接，可重试或连接其他内核。"
                      : "正在连接内核…",
                )}
              </Empty>
              <KernelConnection />
            </section>
          )}
          {data && repositoryReady && tab === "overview" && (
            <div className="overview-grid">
              <section
                className="panel feature-queue-panel"
                aria-labelledby="feature-queue-title"
              >
                <div className="panel-heading">
                  <h2 id="feature-queue-title">
                    Feature 工作队列{" "}
                    <span>{displayText(data.total_features)}</span>
                  </h2>
                  {features.some((feature) => feature.id === featureId) && (
                    <button
                      type="button"
                      className="primary"
                      onClick={() => openOverviewRoute(featureId)}
                    >
                      进入执行工作台 <ArrowRight size={15} aria-hidden="true" />
                    </button>
                  )}
                </div>
                {features.length === 0 ? (
                  <Empty>创建第一个 Feature，开始规划与交付。</Empty>
                ) : (
                  <FeatureBranchQueue
                    key={overviewProject}
                    projectId={overviewProject}
                    repository={project?.workspace_definition.repository || ""}
                    mainline={project?.workspace_definition.mainline || "main"}
                    features={features}
                    selectedId={featureId}
                    onSelect={(id) => {
                      activityRequest.current?.abort();
                      setFeatureId(id);
                      setTaskFilter("");
                      setDetailPages({});
                    }}
                    request={rpc}
                    revision={branchQueueRevision}
                  />
                )}
                <Pagination
                  page={featurePage}
                  setPage={setFeaturePage}
                  hasMore={(featurePage + 1) * 50 < data.total_features}
                />
              </section>
              <aside className="metrics" aria-label="项目统计">
                <Metric
                  title="项目 Features"
                  value={data.summary?.features ?? data.total_features ?? 0}
                  icon={<Layers3 />}
                  onClick={() => openOverviewRoute()}
                />
                <Metric
                  title="运行中"
                  value={data.summary?.running || 0}
                  icon={<Activity />}
                  onClick={() => void openFeatureByActivity("running")}
                />
                <Metric
                  title="等待人确认"
                  value={data.summary?.waiting_confirmation || 0}
                  icon={<Pause />}
                  onClick={() =>
                    void openFeatureByActivity("waiting_confirmation")
                  }
                />
                <Metric
                  title="需要处理"
                  value={data.summary?.blocked || 0}
                  icon={<AlertTriangle />}
                  onClick={() => void openFeatureByActivity("blocked")}
                />
              </aside>
            </div>
          )}
          {data && tab === "execution" && (
            <div className="feature-picker">
              <label>
                当前 Feature
                <ActionSelect
                  aria-label="当前 Feature"
                  value={featureId}
                  onChange={(e) => {
                    setFeatureId(e.target.value);
                    setTaskFilter("");
                    setDetailPages({});
                  }}
                >
                  <option value="">请选择 Feature</option>
                  {f && !features.some((x) => x.id === f.id) && (
                    <option value={f.id}>{displayText(f.title)}</option>
                  )}
                  {features.map((x) => (
                    <option key={x.id} value={x.id}>
                      {displayText(x.title)}
                    </option>
                  ))}
                </ActionSelect>
              </label>
              {f && (
                <>
                  <Badge value={f.status} />
                  <span className="muted">
                    输入 v{displayText(f.input_version)} · revision{" "}
                    {displayText(f.revision)}
                  </span>
                </>
              )}
            </div>
          )}
          {data && tab === "discussion" && (
            <RoleConversation
              key={`${project?.id || "global"}:${discussionFocus?.id || ""}:${discussionFocus?.turn_id || ""}`}
              initialOwner={discussionFocus?.owner_id || featureId || undefined}
              initialSession={discussionFocus?.id}
              initialTurn={discussionFocus?.turn_id || undefined}
              project={project}
              feature={f}
              tasks={detail?.tasks || []}
              agents={definitions}
              configurations={data.configurations || []}
              onLocateExperience={(entry) => {
                const global =
                  entry.project_id === "00000000-0000-4000-8000-000000000000";
                const sourceProject = global ? projectId : entry.project_id;
                if (sourceProject !== projectId) openProject(sourceProject);
                if (!global) {
                  setFeatureId(entry.feature_id || "");
                  setTaskFilter(entry.task_id || "");
                }
                setFocusedSession({
                  id: entry.session_id,
                  owner_id: global
                    ? "global"
                    : entry.task_id || entry.feature_id || sourceProject,
                  project_id: sourceProject,
                  turn_id: entry.turn_id,
                });
                setReadingTarget({});
                setTab("discussion");
              }}
              onDiscussionDeleted={(session, turn) => {
                if (discussionFocus?.id !== session) return;
                if (!turn) {
                  setFocusedSession(undefined);
                  setReadingTarget({});
                } else if (discussionFocus.turn_id === turn) {
                  setFocusedSession({ ...discussionFocus, turn_id: undefined });
                  setReadingTarget({});
                }
              }}
              request={rpc}
              refresh={load}
            />
          )}
          {tab === "execution" && f && (
            <>
              <section
                className={
                  executionTab === "manager" ? "manager-feature-view" : "panel"
                }
              >
                <h2>{displayText(f.title)}</h2>
                <div
                  className="detail-tabs"
                  role="tablist"
                  aria-label="执行详情"
                >
                  {executionTabs.map(([key, label]) => (
                    <ActionButton
                      key={key}
                      role="tab"
                      aria-selected={executionTab === key}
                      onClick={() => {
                        setReadingTarget({});
                        setExecutionTab(key);
                      }}
                    >
                      {displayText(label)}
                    </ActionButton>
                  ))}
                </div>
                {executionTab === "manager" &&
                  !["completed", "cancelled"].includes(f.status) &&
                  !data?.configurations?.some(
                    (r: Row) =>
                      r.type === "workflow_instance" &&
                      r.data.owner_id === f.id &&
                      r.data.parent_execution_id == null,
                  ) && (
                    <section className="panel" aria-label="启动 Feature">
                      <div className="panel-heading">
                        <div>
                          <h3>Feature 尚未启动</h3>
                          <p>
                            已保存需求和执行配置，点击启动后创建工作流并采用已选调度模式。
                          </p>
                        </div>
                        <ActionButton
                          className="primary"
                          disabled={busy || !repositoryReady}
                          onClick={() =>
                            void command("workflow.start", f.id).catch(() => {})
                          }
                        >
                          启动 Feature
                        </ActionButton>
                      </div>
                    </section>
                  )}
                {executionTab === "manager" &&
                  !data?.configurations?.some(
                    (r: Row) =>
                      r.type === "workflow_instance" &&
                      r.data.owner_id === f.id &&
                      r.data.parent_execution_id == null,
                  ) && (
                    <WorkStandard
                      request={rpc}
                      projectId={projectId}
                      owner={f}
                      refresh={load}
                    />
                  )}
                {executionTab === "documents" && (
                  <>
                    <WorkflowMaterials
                      key={`${projectId}:${f.id}`}
                      projectId={projectId}
                      ownerId={f.id}
                      request={rpc}
                      target={activeReadingTarget}
                      documents={detail.documents}
                    />
                    <h3>材料修订</h3>
                    <ActionButton
                      onClick={() =>
                        show({
                          title: "保存材料修订",
                          method: "document.revise",
                          target: f.id,
                          fields: [
                            {
                              key: "kind",
                              label: "交付类型",
                              type: "select",
                              options: (data?.delivery_types || []).map(
                                (r: Row) => ({
                                  value: r.id,
                                  label: r.name,
                                }),
                              ),
                            },
                            { key: "title", label: "标题" },
                            { key: "body", label: "内容", type: "markdown" },
                          ],
                        })
                      }
                    >
                      新建材料
                    </ActionButton>
                    {displayText(pager("documents"))}
                  </>
                )}
                {executionTab === "context" && (
                  <WorkflowContexts
                    key={`${projectId}:${f.id}`}
                    projectId={projectId}
                    ownerId={f.id}
                    request={rpc}
                    target={activeReadingTarget}
                    contexts={detail.contexts}
                    onEditContext={(context) =>
                      show({
                        title: "更新上下文",
                        method: "context.update",
                        target: context.id,
                        revision: context.revision,
                        fields: [
                          {
                            key: "content",
                            label: "内容",
                            type: "markdown",
                            value: context.content,
                          },
                        ],
                      })
                    }
                  />
                )}
                {executionTab === "records" && (
                  <WorkflowExecutionRecords
                    key={`${projectId}:${f.id}`}
                    projectId={projectId}
                    ownerId={f.id}
                    request={rpc}
                    target={activeReadingTarget}
                  />
                )}
              </section>
            </>
          )}
          {tab === "execution" && !f && projectId && (
            <section className="panel">
              <div className="detail-tabs" role="tablist" aria-label="执行详情">
                {executionTabs.map(([key, label]) => (
                  <ActionButton
                    key={key}
                    role="tab"
                    aria-selected={executionTab === key}
                    onClick={() => {
                      setReadingTarget({});
                      setExecutionTab(key);
                    }}
                  >
                    {displayText(label)}
                  </ActionButton>
                ))}
              </div>
              {executionTab === "documents" && (
                <WorkflowMaterials
                  key={`${projectId}:${featureId}`}
                  projectId={projectId}
                  ownerId={featureId || projectId}
                  request={rpc}
                  target={activeReadingTarget}
                />
              )}
              {executionTab === "context" && (
                <WorkflowContexts
                  key={`${projectId}:${featureId}`}
                  projectId={projectId}
                  ownerId={featureId || projectId}
                  request={rpc}
                  target={activeReadingTarget}
                />
              )}
              {executionTab === "records" && (
                <WorkflowExecutionRecords
                  key={`${projectId}:${featureId}`}
                  projectId={projectId}
                  ownerId={featureId || projectId}
                  request={rpc}
                  target={activeReadingTarget}
                />
              )}
            </section>
          )}
          {tab === "execution" &&
            ["manager", "overview"].includes(executionTab) &&
            projectId && (
              <WorkflowScope
                key={`${projectId}:${featureId || projectId}`}
                projectId={projectId}
                ownerId={featureId || projectId}
                request={rpc}
                refresh={load}
                records={data?.configurations || []}
                tasks={detail?.tasks || []}
                view={executionTab}
                target={activeReadingTarget}
                onDiscussion={(id, owner, turnId) => {
                  setFocusedSession({
                    id,
                    owner_id: owner,
                    turn_id: turnId,
                    project_id: projectId,
                  });
                  setTab("discussion");
                }}
              />
            )}
          {tab === "execution" &&
            !featureId &&
            !f &&
            ["manager", "overview"].includes(executionTab) && (
              <section className="panel">
                <h2>
                  Feature 工作队列
                  {displayText(workFilter ? ` · ${workFilter}` : "")}
                </h2>
                {features.map((feature) => (
                  <ActionButton
                    className="feature-row"
                    key={feature.id}
                    onClick={() => setFeatureId(feature.id)}
                  >
                    {displayText(feature.title)} ·{" "}
                    {displayText(labels[feature.status] || feature.status)}
                  </ActionButton>
                ))}
                <Pagination
                  page={featurePage}
                  setPage={setFeaturePage}
                  hasMore={(featurePage + 1) * 50 < (data?.total_features || 0)}
                />
              </section>
            )}
          {data && ["workflows", "components"].includes(tab) && (
            <ConfigurationPage
              key={`${tab}:${projectId}:${catalogGeneration}:${configurationSession}`}
              group={tab === "workflows" ? "processes" : "foundations"}
              request={configurationRequest}
              project={project}
              installations={data.runtime_installations || []}
              refresh={load}
            />
          )}
          {data && ["actors", "definitions"].includes(tab) && (
            <AgentCatalog
              key={`actors:${projectId}:${catalogGeneration}:${configurationSession}`}
              request={configurationRequest}
              project={project}
              installations={data.runtime_installations || []}
              group="actors"
            />
          )}
          {data && tab === "projects" && (
            <ProjectManager
              key={`project-manager:${projectId || "global"}:${catalogGeneration}:${configurationSession}`}
              bootstrap={data}
              request={configurationRequest}
              project={project}
              refresh={load}
              onOpen={async (id) => {
                await command("project.open_github", id, {});
                setTab("projects");
              }}
            />
          )}
          {data && tab === "history" && (
            <label>
              历史范围
              <ActionSelect
                value={historyScope}
                onChange={(e) => {
                  setHistoryScope(e.target.value);
                }}
              >
                <option value="project">当前项目</option>
                <option value="global">Global</option>
              </ActionSelect>
            </label>
          )}
          {data && tab === "history" && (
            <CallHistory
              key={`${projectId}:${historyScope}`}
              request={rpc}
              historyScope={historyScope === "global" ? "global" : "project"}
              project={
                historyScope === "project" && projectId
                  ? { id: projectId }
                  : undefined
              }
            />
          )}
          {data && tab === "settings" && (
            <>
              <section className="panel">
                <div className="panel-heading">
                  <h2>数据与运行</h2>
                  <span className="muted">按需准备运行环境</span>
                </div>
                <KernelConnection />
                <div className="setting-row">
                  <div>
                    <h3>内核 data 目录</h3>
                    <code>{displayText(data.data_root)}</code>
                    <p className="muted">
                      全局配置与各项目分别保存；打开项目即可切换。更改 data
                      路径需退出后以 --data 新路径重启。
                    </p>
                  </div>
                  <ActionButton
                    onClick={() =>
                      setView({
                        title: "打开其他数据集",
                        body: "1. 退出当前内核。\n2. 使用 devdesk-host --data /path/to/dataset 启动，或不传 --data 后在初始设置中选择路径。\n\n不自动搬迁原数据。",
                      })
                    }
                  >
                    更改路径说明
                  </ActionButton>
                </div>
                <div className="setting-row">
                  <div>
                    <h3>全局配置数据库</h3>
                    <code>{displayText(data.global_database)}</code>
                    <p className="muted">并发配置、共享定义与项目索引。</p>
                  </div>
                </div>
                {data.project_database && (
                  <div className="setting-row">
                    <div>
                      <h3>当前项目数据库</h3>
                      <code>{displayText(data.project_database)}</code>
                      <p className="muted">
                        工程状态、执行历史和产物归属仅限当前项目。
                      </p>
                    </div>
                  </div>
                )}
                {project && (
                  <div className="setting-row">
                    <div>
                      <h3>repository 位置</h3>
                      <code>
                        {displayText(
                          project.workspace_definition.path || "未设置",
                        )}
                      </code>
                    </div>
                  </div>
                )}
              </section>
              <GitHubConnection request={rpc} />
              <RuntimeSettings
                provider={data.default_harness || "codex"}
                installations={data.runtime_installations || []}
                settingsVersions={data.settings_versions || []}
                request={rpc}
                refresh={load}
              />
              <section className="panel" aria-label="模板与额度设置">
                <TemplateSettings request={rpc} refresh={load} />
                <div className="setting-row">
                  <div>
                    <h3>并发额度设置</h3>
                    <p>
                      全局 {displayText(data.global_concurrency)} 个 Run ·
                      单项目默认 {displayText(data.default_project_concurrency)}{" "}
                      个 Run
                    </p>
                  </div>
                  <ActionButton
                    onClick={() =>
                      show({
                        title: "并发额度设置",
                        method: "settings.update",
                        fields: [
                          {
                            key: "global_concurrency",
                            label: "全局并发额度",
                            type: "number",
                            value: String(data.global_concurrency),
                            min: 1,
                            max: 32,
                          },
                          {
                            key: "default_project_concurrency",
                            label: "单项目默认并发额度",
                            type: "number",
                            value: String(data.default_project_concurrency),
                            description: "用于新建项目。",
                            min: 1,
                            max: 32,
                          },
                        ],
                      })
                    }
                  >
                    调整
                  </ActionButton>
                </div>
              </section>
              <div className="setting-row">
                <div>
                  <h3>退出内核</h3>
                  <p className="muted">
                    未确认的运行在下次启动后标记中断，可显式重试。
                  </p>
                </div>
                <ActionButton
                  className="danger"
                  onClick={() =>
                    show({
                      title: "退出当前内核",
                      method: "shutdown",
                      fields: [],
                    })
                  }
                >
                  退出
                </ActionButton>
              </div>
              <div className="notice">
                Web UI 连接当前 Rust Host。真实工程需配置 Harness、Docker 和
                GitHub；平台验收记录见文档。
              </div>
            </>
          )}
          <footer>
            DevDesk 0.2 <span>工程内核 · 持久化历史 · GitHub 工程接入</span>
          </footer>
        </ConfigurationMain>
      </div>
      {dialog && (
        <CommandDialog
          key={dialog.method + (dialog.target || "")}
          spec={dialog}
          busy={busy}
          close={() => setDialog(undefined)}
          submit={async (payload) => {
            let result;
            if (dialog.method === "shutdown") {
              const response = await fetch("/shutdown", {
                method: "POST",
                headers: { "Content-Type": "application/json" },
                body: JSON.stringify({
                  command_id: dialog.commandId,
                  instance_id: dialog.hostVersion?.instance_id,
                  expected_revision: dialog.hostVersion?.revision,
                }),
              });
              const outcome = await response.json();
              if (!response.ok || outcome.error)
                throw new Error(
                  outcome.error
                    ? `${outcome.code} · ${outcome.error}`
                    : `退出失败 HTTP ${response.status}`,
                );
              kernelStopped.current = true;
              ++requestGeneration.current;
              ++projectGeneration.current;
              setView(undefined);
              setConnection("已断开");
              setCaps(undefined);
              setData(undefined);
            } else if (dialog.method === "settings.update") {
              const changes = Object.fromEntries(
                dialog.fields
                  .filter(({ key, value }) => String(payload[key]) !== value)
                  .map(({ key }) => [key, payload[key]]),
              );
              if (!Object.keys(changes).length) {
                setDialog(undefined);
                return;
              }
              result = await rpc(dialog.method, {
                command_id: dialog.commandId,
                preconditions: dialog.preconditions?.filter(({ resource }) =>
                  Object.hasOwn(changes, resource.id),
                ),
                payload: changes,
              });
              await load();
            } else {
              result = await command(
                dialog.method,
                dialog.target,
                payload,
                dialog.revision,
                false,
              );
            }
            if (dialog.method === "feature.create" && result?.id) {
              setFeatureId(result.id);
              setTab("overview");
            }
            if (dialog.method !== "shutdown")
              publishActionFeedback(
                dialog.feedbackTarget || null,
                "操作已保存",
              );
            setDialog(undefined);
          }}
        />
      )}
      {view && (
        <Reader
          title={displayText(view.title)}
          body={view.body}
          format={view.format}
          metadata={view.metadata}
          close={() => setView(undefined)}
        />
      )}
    </div>
  );
}
function Metric({
  title,
  value,
  icon,
  onClick,
}: {
  title: string;
  value: number;
  icon: React.ReactNode;
  onClick: () => void;
}) {
  return (
    <div className="metric-card">
      <ActionButton className="metric" type="button" onClick={onClick}>
        <span className="metric-icon" aria-hidden="true">
          {icon}
        </span>
        <span className="metric-label">{title}</span>
        <strong>{value.toString().padStart(2, "0")}</strong>
      </ActionButton>
    </div>
  );
}
function Pagination({
  page,
  setPage,
  hasMore,
}: {
  page: number;
  setPage: (p: number) => void;
  hasMore: boolean;
}) {
  return (
    <div className="pagination">
      <ActionButton disabled={page === 0} onClick={() => setPage(page - 1)}>
        上一页
      </ActionButton>
      <span>第 {page + 1} 页</span>
      <ActionButton disabled={!hasMore} onClick={() => setPage(page + 1)}>
        下一页
      </ActionButton>
    </div>
  );
}
function CommandDialog({
  spec,
  busy,
  close,
  submit,
}: {
  spec: DialogSpec;
  busy: boolean;
  close: () => void;
  submit: (p: Row) => Promise<void>;
}) {
  const {
    error: localError,
    setError: setLocalError,
    beginFeedback,
    endFeedback,
  } = useActionFeedback();
  const formRef = useRef<HTMLFormElement>(null);
  const [values, setValues] = useState<Record<string, string>>(() =>
    Object.fromEntries(
      spec.fields.map((f) => [f.key, f.value ?? f.options?.[0]?.value ?? ""]),
    ),
  );
  useModalFocus(formRef, () => {
    if (!busy) close();
  });
  const send = async (e: FormEvent) => {
    e.preventDefault();
    beginFeedback();
    setLocalError("");
    try {
      const payload: Row = { ...spec.payload };
      for (const field of spec.fields) {
        const value = values[field.key];
        if (field.optional && !value.trim()) continue;
        if (field.type === "markdown" && !field.optional && !value.trim())
          throw new Error(`请填写${field.label}`);
        if (field.type === "json") payload[field.key] = JSON.parse(value);
        else if (field.type === "number") payload[field.key] = Number(value);
        else if (value || !field.optional) payload[field.key] = value;
      }
      await submit(payload);
    } catch (error) {
      setLocalError(String(error));
    } finally {
      endFeedback();
    }
  };
  useEffect(() => {
    const escape = (e: KeyboardEvent) => {
      if (e.key === "Escape" && !busy) close();
    };
    window.addEventListener("keydown", escape);
    return () => window.removeEventListener("keydown", escape);
  }, [close, busy]);
  const renderField = (f: Field, i: number) =>
    f.type === "markdown" ? (
      <MarkdownField
        key={f.key}
        label={f.label}
        value={values[f.key]}
        required={!f.optional}
        onChange={(value) => setValues({ ...values, [f.key]: value })}
      />
    ) : (
      <label key={f.key}>
        {displayText(f.label)}
        {f.type === "textarea" || f.type === "json" ? (
          <textarea
            aria-label={displayText(f.label)}
            autoFocus={i === 0 && !f.advanced}
            required={!f.optional}
            value={values[f.key]}
            onChange={(e) => setValues({ ...values, [f.key]: e.target.value })}
          />
        ) : f.type === "select" ? (
          <ActionSelect
            aria-label={displayText(f.label)}
            value={values[f.key]}
            onChange={(e) => setValues({ ...values, [f.key]: e.target.value })}
          >
            {f.options?.map((o) => (
              <option value={o.value} key={o.value}>
                {displayText(o.label)}
              </option>
            ))}
          </ActionSelect>
        ) : (
          <input
            aria-label={displayText(f.label)}
            autoFocus={i === 0 && !f.advanced}
            required={!f.optional}
            type={f.type === "number" ? "number" : "text"}
            min={f.min}
            max={f.max}
            value={values[f.key]}
            onChange={(e) => setValues({ ...values, [f.key]: e.target.value })}
          />
        )}
        {f.description && (
          <span className="muted">{displayText(f.description)}</span>
        )}
      </label>
    );
  return (
    <div className="modal-backdrop">
      <ActionForm
        className="modal"
        ref={formRef}
        role="dialog"
        aria-modal="true"
        aria-label={displayText(spec.title)}
        onSubmit={send}
      >
        <div className="modal-heading">
          <h2>{displayText(spec.title)}</h2>
          <ActionButton
            type="button"
            aria-label="关闭"
            disabled={busy}
            onClick={close}
          >
            <X size={18} />
          </ActionButton>
        </div>
        {localError && (
          <div className="error" role="alert">
            {displayText(localError)}
          </div>
        )}
        {spec.fields.length === 0 && (
          <p className="muted">确认执行此领域操作。结果将保存到工程历史。</p>
        )}
        {spec.fields.filter((f) => !f.advanced).map(renderField)}
        {spec.fields.some((f) => f.advanced) && (
          <details>
            <summary>高级选项</summary>
            {spec.fields.filter((f) => f.advanced).map(renderField)}
          </details>
        )}
        <div className="modal-actions">
          <ActionButton type="button" disabled={busy} onClick={close}>
            取消
          </ActionButton>
          <ActionButton className="primary" disabled={busy}>
            {displayText(busy ? "保存中…" : "确认")}
          </ActionButton>
        </div>
      </ActionForm>
    </div>
  );
}
function useModalFocus(
  ref: React.RefObject<HTMLElement | null>,
  close: () => void,
) {
  const closeRef = useRef(close);
  closeRef.current = close;
  useEffect(() => {
    const previous = document.activeElement as HTMLElement | null;
    const container = ref.current;
    if (!container) return;
    const elements = () =>
      Array.from(
        container.querySelectorAll<HTMLElement>(
          'button:not(:disabled),input,textarea,select,[tabindex="0"]',
        ),
      );
    elements()[0]?.focus();
    const key = (event: KeyboardEvent) => {
      if (event.key === "Escape") {
        event.preventDefault();
        closeRef.current();
      }
      if (event.key === "Tab") {
        const list = elements();
        const first = list[0],
          last = list[list.length - 1];
        if (event.shiftKey && document.activeElement === first) {
          event.preventDefault();
          last?.focus();
        } else if (!event.shiftKey && document.activeElement === last) {
          event.preventDefault();
          first?.focus();
        }
      }
    };
    container.addEventListener("keydown", key);
    return () => {
      container.removeEventListener("keydown", key);
      previous?.focus();
    };
  }, [ref]);
}
function Reader({
  title,
  body,
  format,
  metadata,
  close,
}: {
  title: string;
  body: string;
  format?: "markdown";
  metadata?: Row;
  close: () => void;
}) {
  const ref = useRef<HTMLElement>(null);
  useModalFocus(ref, close);
  return (
    <div className="modal-backdrop" onClick={close}>
      <section
        className="modal reader"
        ref={ref}
        role="dialog"
        aria-modal="true"
        aria-label={displayText(title)}
        onClick={(e) => e.stopPropagation()}
      >
        <div className="modal-heading">
          <h2>{displayText(title)}</h2>
          <ActionButton aria-label="关闭" onClick={close}>
            <X size={18} />
          </ActionButton>
        </div>
        {format === "markdown" ? (
          <MarkdownPreview text={body} />
        ) : (
          <pre>{displayText(body)}</pre>
        )}
        {metadata && (
          <details>
            <summary>文档信息</summary>
            <pre>{displayText(pretty(metadata))}</pre>
          </details>
        )}
      </section>
    </div>
  );
}
export function AppRoot() {
  return route.has("read") ? <RawJsonReader request={rpc} /> : <App />;
}

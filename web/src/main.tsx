import {
  ActionButton,
  ActionForm,
  ActionSelect,
  currentActionTarget,
  publishActionFeedback,
  type ActionTarget,
  useActionFeedback,
} from "./components/ActionFeedback";
import "./browser-compat";
import { ProjectManager } from "./components/ProjectManager";
import React, {
  useState,
  useEffect,
  useCallback,
  useRef,
  FormEvent,
} from "react";
import { createRoot } from "react-dom/client";
import { MarkdownField, MarkdownPreview } from "./components/configuration-ui";
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
  Check,
  X,
  Play,
  Pause,
  RotateCcw,
  GitMerge,
  ChevronRight,
  FolderOpen,
  CircleDot,
  Activity,
  ShieldCheck,
  Terminal,
  Box,
  Copy,
  Menu,
  AlertTriangle,
} from "lucide-react";
import "./style.css";
import { ConfigurationPage } from "./components/ConfigurationPages";
import { CallHistory } from "./components/Integrations";
import { AgentCatalog } from "./components/AgentCatalog";
import { WorkflowInstanceView } from "./components/WorkflowInstanceView";
import { RepositorySettings } from "./components/RepositorySettings";
import { RoleConversation } from "./components/RoleConversation";
import { RuntimePreferences } from "./components/RuntimePreferences";
import { PermissionSettings } from "./components/PermissionSettings";
import { RuntimeSettings } from "./components/RuntimeSettings";
import { RuntimePreparationStatus } from "./components/RuntimePreparationStatus";
import {
  KernelAuthentication,
  KernelConnection,
} from "./components/KernelConnection";

type RecordData = { [key: string]: any };
type Field = {
  key: string;
  label: string;
  value?: string;
  type?: "textarea" | "select" | "number" | "markdown";
  options?: { value: string; label: string }[];
  optional?: boolean;
  description?: string;
};
type DialogSpec = {
  title: string;
  method: string;
  target?: string;
  payload?: RecordData;
  fields: Field[];
  revision?: number;
  feedbackTarget?: ActionTarget | null;
};
const labels: Record<string, string> = {
  draft: "草稿",
  planning: "规划中",
  ready: "就绪",
  developing: "开发中",
  validating: "验收中",
  accepted: "已接受",
  merging: "合入中",
  completed: "已完成",
  running: "运行中",
  waiting_review: "等待评审",
  failed: "失败",
  cancelled: "已取消",
  interrupted: "已中断",
  queued: "排队中",
  succeeded: "成功",
  rejected: "已退回",
  pass: "通过",
  changes_requested: "要求修改",
  inconclusive: "不确定",
  not_reviewable: "不可评审",
  active: "使用中",
  released: "已释放",
  cleanup_failed: "清理失败",
  planning_phase: "规划",
  implementation: "Task 实现",
  closure: "Feature 收尾",
  requirement: "需求",
  plan: "设计 / Task / Test 计划",
  merge_candidate: "Merge 候选",
  red: "Red · 预期断言失败",
  implement: "实现",
  green: "Green · 测试验证",
  docs: "文档",
  verify: "交付验证",
  feature_tests: "Feature 完整测试",
  merge_verify: "候选验证",
  research: "需求调研",
  design: "设计",
  task_plan: "任务计划",
  test_plan: "测试计划",
  test: "测试定义",
  doc: "说明文档",
};
const pretty = (v: any) => JSON.stringify(v, null, 2);
const short = (id: string) => id?.slice(0, 8) || "—";
const fmt = (time: number) =>
  new Date(time).toLocaleString("zh-CN", { hour12: false });
let sequence = 0;
async function rpc(method: string, params: RecordData = {}) {
  const res = await fetch("/rpc", {
    method: "POST",
    headers: { "Content-Type": "application/json" },
    body: JSON.stringify({ jsonrpc: "2.0", id: ++sequence, method, params }),
    signal: AbortSignal.timeout(
      [
        "session.open",
        "attempt.start",
        "review.request",
        "merge.request",
        "runtime.prepare",
        "codex.auth.start",
        "codex.auth.status",
        "codex.auth.models",
      ].includes(method)
        ? 25 * 60 * 1000
        : 30000,
    ),
  });
  if (!res.ok) throw new Error(`HTTP ${res.status} · 内核连接失败`);
  const data = await res.json();
  if (data.error)
    throw new Error(`${data.error.data?.code || ""} · ${data.error.message}`);
  return data.result;
}
function Badge({ value }: { value: string }) {
  return (
    <span className={`badge status-${value}`}>{labels[value] || value}</span>
  );
}
function Empty({ children }: { children: React.ReactNode }) {
  return (
    <div className="empty">
      <Box size={26} />
      <p>{children}</p>
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

function App() {
  const requestGeneration = useRef(0);
  const projectGeneration = useRef(0);
  const kernelStopped = useRef(false);
  const [detailPages, setDetailPages] = useState<Record<string, number>>({});
  const [data, setData] = useState<RecordData>();
  const [caps, setCaps] = useState<RecordData>();
  const [projectId, setProjectId] = useState("");
  const [featureId, setFeatureId] = useState(route.get("feature_id") || "");
  const [tab, setTab] = useState(routeTab);
  const [error, setError] = useState("");
  const [connection, setConnection] = useState("连接中");
  const [dialog, setDialog] = useState<DialogSpec>();
  const [busy, setBusy] = useState(false);
  const mutationInFlight = useRef(false);
  const historyTarget = useRef<ActionTarget | null>(null);
  const [history, setHistory] = useState<RecordData[]>([]);
  const [historyScope, setHistoryScope] = useState(
    ["project", "global", "tools"].includes(route.get("history_scope") || "")
      ? route.get("history_scope")!
      : "project",
  );
  const [toolsHistoryScope, setToolsHistoryScope] = useState("project");
  const [historyPage, setHistoryPage] = useState(0);
  const [featurePage, setFeaturePage] = useState(0);
  const [projectPage, setProjectPage] = useState(0);
  const [view, setView] = useState<{
    title: string;
    body: string;
    format?: "markdown";
    metadata?: RecordData;
  }>();
  const [setupPath, setSetupPath] = useState("./data");
  const [roleRevision, setRoleRevision] = useState(0);
  const [focusedSession, setFocusedSession] = useState<RecordData>();
  const [taskFilter, setTaskFilter] = useState(route.get("task_id") || "");
  const [executionTab, setExecutionTab] = useState(
    route.get("detail") ||
      (route.get("tab") === "documents" ? "documents" : "overview"),
  );
  const [workFilter, setWorkFilter] = useState(route.get("filter") || "");
  const routeApplied = useRef(false);
  useEffect(() => {
    const params = new URLSearchParams();
    for (const key of ["config_tab", "config_object", "workflow_id"]) {
      const value = new URLSearchParams(location.search).get(key);
      if (value) params.set(key, value);
    }
    params.set("tab", tab);
    if (projectId) params.set("project_id", projectId);
    if (featureId) params.set("feature_id", featureId);
    if (taskFilter) params.set("task_id", taskFilter);
    if (tab === "execution") params.set("detail", executionTab);
    if (tab === "history") params.set("history_scope", historyScope);
    if (workFilter) params.set("filter", workFilter);
    if (featurePage) params.set("page", String(featurePage));
    const url = `?${params}`;
    if (location.search !== url) window.history.pushState(null, "", url);
  }, [
    tab,
    projectId,
    featureId,
    taskFilter,
    executionTab,
    workFilter,
    featurePage,
    historyScope,
  ]);
  useEffect(() => {
    const pop = () => {
      const p = new URLSearchParams(location.search);
      const pid = p.get("project_id");
      if (pid && pid !== projectId) {
        void rpc("project.open_github", {
          command_id: crypto.randomUUID(),
          target: pid,
          payload: {},
        })
          .then(() => {
            ++requestGeneration.current;
            setProjectId(pid);
            setFeatureId(p.get("feature_id") || "");
            setTaskFilter(p.get("task_id") || "");
          })
          .catch((e) => setError(String(e)));
      }
      setTab(routeTabValue(p));
      setHistoryScope(p.get("history_scope") || "project");
      setFeatureId(p.get("feature_id") || "");
      setTaskFilter(p.get("task_id") || "");
      setExecutionTab(p.get("detail") || "overview");
      setWorkFilter(p.get("filter") || "");
      setFeaturePage(Number(p.get("page") || 0));
    };
    window.addEventListener("popstate", pop);
    return () => window.removeEventListener("popstate", pop);
  }, [projectId]);
  const [snapshotA, setSnapshotA] = useState("");
  const [snapshotB, setSnapshotB] = useState("");
  const load = useCallback(
    async (afterCommand = false) => {
      // A command refresh must settle before another form captures its revision.
      // SSE refreshes during the mutation are covered by that final read.
      if (kernelStopped.current || (mutationInFlight.current && !afterCommand))
        return;
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
          let active = c.active_project_id || "";
          let obsoleteRoute = false;
          if (!routeApplied.current) {
            routeApplied.current = true;
            const requested = route.get("project_id");
            if (requested && requested !== active) {
              try {
                await rpc("project.open_github", {
                  command_id: crypto.randomUUID(),
                  target: requested,
                  payload: {},
                });
                active = requested;
              } catch (error) {
                if (
                  !(error instanceof Error) ||
                  !error.message.startsWith("not_found ·")
                )
                  throw error;
                obsoleteRoute = true;
                for (const key of [
                  "project_id",
                  "feature_id",
                  "task_id",
                  "session_id",
                ])
                  route.delete(key);
                window.history.replaceState(null, "", `?${route}`);
                setFeatureId("");
                setTaskFilter("");
              }
            }
          }

          if (projectId !== active) {
            ++projectGeneration.current;
            setProjectId(active);
            setFeatureId(
              route.get("project_id") === active || !route.get("project_id")
                ? route.get("feature_id") || ""
                : "",
            );
            setFeaturePage(Number(route.get("page") || 0));
            setDetailPages({});
            setHistory([]);
            setHistoryPage(0);
            setSnapshotA("");
            setSnapshotB("");
            setData(undefined);
            return;
          }
          const d = await rpc("query.workspace", {
            project_id: projectId || undefined,
            feature_id: obsoleteRoute ? undefined : featureId || undefined,
            offset: featurePage * 50,
            project_offset: projectPage * 50,
            github_only: true,
            limit: 50,
            detail_pages: detailPages,
            work_filter: workFilter,
            task_id: obsoleteRoute ? undefined : taskFilter || undefined,
          });
          if (generation !== requestGeneration.current) return;
          setData(d);
        }
      } catch (e) {
        if (generation === requestGeneration.current) {
          setError(String(e));
          setConnection("连接失败");
        }
      }
    },
    [
      projectId,
      featureId,
      featurePage,
      projectPage,
      detailPages,
      workFilter,
      taskFilter,
    ],
  );
  useEffect(() => {
    load();
  }, [load]);
  useEffect(() => {
    if (caps?.setup_required !== false) return;
    let timer: ReturnType<typeof setTimeout> | undefined;
    const stream = new EventSource(
      `/events${projectId ? `?project_id=${encodeURIComponent(projectId)}` : ""}`,
    );
    stream.onopen = () => {
      if (!kernelStopped.current) setConnection("已连接");
    };
    stream.onmessage = () => {
      if (!timer)
        timer = setTimeout(() => {
          timer = undefined;
          load();
        }, 180);
    };
    stream.onerror = () => {
      if (!kernelStopped.current) setConnection("连接中断 · 自动重连");
    };
    return () => {
      stream.close();
      if (timer) clearTimeout(timer);
    };
  }, [load, projectId, caps?.setup_required]);
  useEffect(() => {
    let current = true;
    if (tab === "history" && data && historyScope !== "tools")
      rpc("query.history", {
        project_id: projectId || undefined,
        scope: historyScope,
        offset: historyPage * 50,
        limit: 50,
      })
        .then((d) => {
          if (current) setHistory(d.events);
        })
        .catch((e) => {
          if (
            current &&
            !publishActionFeedback(historyTarget.current, String(e), true)
          )
            setError(String(e));
        });
    return () => {
      current = false;
    };
  }, [tab, historyPage, historyScope, data, projectId]);
  const detail = data?.detail;
  const f = detail?.feature;
  const projects: RecordData[] = (data?.projects || []).filter(
    (p: RecordData) => p.workspace_definition?.repository,
  );
  const features: RecordData[] = data?.features || [];
  const definitions: RecordData[] = (data?.definitions || []).filter(
    (d: RecordData) => d.family !== "agent" || d.agent_profile,
  );
  const project = data?.active_project?.workspace_definition?.repository
    ? data.active_project
    : undefined;
  const repositoryReady = data?.repository_ready === true;
  const projectOptions =
    project && !projects.some((p) => p.id === project.id)
      ? [project, ...projects]
      : projects;
  const resetProjectView = (id: string) => {
    ++requestGeneration.current;
    ++projectGeneration.current;
    setProjectId(id);
    setFeatureId("");
    setFeaturePage(0);
    setDetailPages({});
    setHistory([]);
    setHistoryPage(0);
    setSnapshotA("");
    setSnapshotB("");
    setTaskFilter("");
    setView(undefined);
    setData(undefined);
  };
  const openProject = async (id: string) => {
    const feedbackTarget = currentActionTarget();
    setBusy(true);
    setError("");
    try {
      await rpc("project.open_github", {
        command_id: crypto.randomUUID(),
        target: id,
        payload: {},
      });
      resetProjectView(id);
      setDialog(undefined);
    } catch (e) {
      if (!publishActionFeedback(feedbackTarget, String(e), true))
        setError(String(e));
    } finally {
      setBusy(false);
    }
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
              ...(["attempts", "merges"].includes(key) ? { runs: 0 } : {}),
            }))
          }
          hasMore={(page + 1) * size < detail.totals[key]}
        />
      )
    );
  };
  const allowed = (id: string, method: string) =>
    detail?.allowed_actions?.[id]?.allowed?.includes(method);
  const getRevision = (target?: string) => {
    if (!target) return undefined;
    return [
      f,
      project,
      ...projects,
      ...(detail?.tasks || []),
      ...(detail?.attempts || []),
      ...(detail?.merges || []),
      ...(detail?.contexts || []),
      ...(detail?.effects || []),
    ].find((x) => x?.id === target)?.revision;
  };
  const show = (spec: DialogSpec) =>
    setDialog({
      ...spec,
      revision: getRevision(spec.target),
      feedbackTarget: spec.feedbackTarget ?? currentActionTarget(),
    });
  const command = async (
    method: string,
    target?: string,
    payload: RecordData = {},
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
  const configurationLock = data?.configurations?.find(
    (r: RecordData) => r.kind === "feature_lock" && r.definition_id === f?.id,
  );
  const begin = (target: string, phase?: string) => {
    show({
      title: phase === "closure" ? "启动 Feature 收尾" : "启动 Task Attempt",
      method: "attempt.start",
      target,
      payload: { phase },
      fields: [],
    });
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
      const choices: RecordData[] = [];
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
          configuration_revision_id: data?.configurations
            ?.filter((r: RecordData) => r.kind === "project_configuration")
            .sort((a: RecordData, b: RecordData) => b.version - a.version)[0]
            ?.id,
        },
        fields: [
          { key: "title", label: "Feature 名称" },
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
  const review = (d: RecordData, decision: string) =>
    show({
      title: `${labels[decision]} · ${labels[d.kind] || d.kind}`,
      method: "review.submit",
      target: d.id,
      payload: { decision },
      fields: [
        {
          key: "reason",
          label: "评审理由",
          type: "textarea",
          value:
            decision === "pass"
              ? "已检查冻结版本、证据与验收条件，同意接受。"
              : "",
        },
      ],
    });
  const act = (
    id: string,
    method: string,
    title: string,
    fn: () => void,
    primary = false,
  ) =>
    allowed(id, method) && (
      <ActionButton
        className={primary ? "primary" : ""}
        disabled={busy}
        onClick={fn}
      >
        {title}
      </ActionButton>
    );
  const deliveryCard = (d: RecordData) => {
    const reviews = (detail.reviews as RecordData[]).filter(
      (r) => r.delivery_id === d.id,
    );
    const accepted = detail.accepted_deliveries.includes(d.id);
    return (
      <article className="delivery" key={d.id}>
        <div className="row">
          <div>
            <span className="eyebrow">DELIVERY · {short(d.id)}</span>
            <h3>{labels[d.kind] || d.kind}</h3>
          </div>
          <Badge
            value={d.stale ? "stale" : accepted ? "accepted" : "waiting_review"}
          />
        </div>
        <p className="muted">
          输入 v{d.input_version} · {d.documents.length} 文档 ·{" "}
          {d.evidence.length} 证据 · {d.source_deliveries?.length || 0}{" "}
          个来源交付 · origin={d.origin}
        </p>
        {d.candidate && <code>{d.candidate}</code>}
        <div className="actions">
          <ActionButton onClick={() => openArtifact(d.artifact_id)}>
            交付报告 <ArrowRight size={13} />
          </ActionButton>
          <ActionButton
            onClick={() => setView({ title: "冻结交付版本", body: pretty(d) })}
          >
            冻结引用
          </ActionButton>
          {act(d.id, "review.request", "Agent 评审", () =>
            command("review.request", d.id, {}).catch(() => {}),
          )}
          {act(d.id, "review.submit", "通过", () => review(d, "pass"))}
          {act(d.id, "review.submit", "退回", () =>
            review(d, "changes_requested"),
          )}
          {act(d.id, "review.submit", "不确定", () =>
            review(d, "inconclusive"),
          )}
          {act(d.id, "review.submit", "不可评审", () =>
            review(d, "not_reviewable"),
          )}
          {act(d.id, "override.accept", "例外接受", () =>
            show({
              title: "人工 Override · 只豁免指定质量条件",
              method: "override.accept",
              target: d.id,
              fields: [
                { key: "reason", label: "例外原因及影响", type: "textarea" },
                {
                  key: "items",
                  label: "豁免条目（逗号分隔）：review 或 test:RunID",
                  value: "review",
                },
              ],
            }),
          )}
        </div>
        {reviews.map((r) => (
          <div className="review-line" key={r.id}>
            <ShieldCheck size={15} />
            <Badge value={r.decision} />
            <span>
              {r.actor} · {r.reason}
            </span>
            <ActionButton
              className="text-button"
              onClick={() => setView({ title: "逐项 Review", body: pretty(r) })}
            >
              逐项标准
            </ActionButton>
          </div>
        ))}
        {detail.overrides
          .filter((o: RecordData) => o.delivery_id === d.id)
          .map((o: RecordData) => (
            <div className="notice" key={o.id}>
              人工例外 · {o.reason} · {o.items.join(", ")}（原事实保留）
            </div>
          ))}
        {detail.allowed_actions[d.id]?.blocked.map((reason: string) => (
          <div className="notice" key={reason}>
            {reason}
          </div>
        ))}
      </article>
    );
  };
  const openArtifact = async (id: string) => {
    const feedbackTarget = currentActionTarget();
    const generation = projectGeneration.current;
    try {
      const [r, artifact] = await Promise.all([
        fetch(`/artifacts/${id}?project_id=${encodeURIComponent(projectId)}`),
        rpc("query.artifact", { id, project_id: projectId }),
      ]);
      if (!r.ok) throw new Error("产物读取失败或哈希校验失败");
      const body = await r.text();
      if (generation === projectGeneration.current)
        setView({
          title: `Artifact · ${short(id)}`,
          body,
          ...(/\.md$/i.test(artifact.relative_path || "") ||
          artifact.media_type?.includes("markdown")
            ? { format: "markdown" as const }
            : {}),
        });
    } catch (e) {
      if (
        generation === projectGeneration.current &&
        !publishActionFeedback(feedbackTarget, String(e), true)
      )
        setError(String(e));
    }
  };
  const nav = [
    ["overview", "工作总览", LayoutDashboard],
    ["execution", "执行与交付", Workflow],
    ["discussion", "角色讨论", FileText],
    ["projects", "项目管理", Layers3],
    ["workflows", "流程与规范", Workflow],
    ["actors", "角色与 Agent", Library],
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
              {error}
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
              try {
                setBusy(true);
                const r = await fetch("/setup", {
                  method: "POST",
                  headers: { "Content-Type": "application/json" },
                  body: JSON.stringify({ path: setupPath }),
                }).then((r) => r.json());
                if (r.error) throw new Error(r.error);
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
                required
              />
            </label>
            <p className="muted">
              支持空目录或已有 DevDesk 数据集。切换目录需重启程序。
            </p>
            <ActionButton className="primary" disabled={busy}>
              初始化 / 打开数据集 <ArrowRight size={16} />
            </ActionButton>
          </ActionForm>
          {error && (
            <div className="error" role="alert">
              {error}
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
            value={projectId || project?.id || ""}
            disabled={busy}
            onChange={(e) => openProject(e.target.value)}
          >
            {projects.length === 0 && <option value="">尚无项目</option>}
            {projectOptions.map((p) => (
              <option key={p.id} value={p.id}>
                {p.title}
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
              onClick={() => setTab(id)}
            >
              <Icon size={18} />
              {title}
              {id === "execution" &&
                detail?.runs.filter((r: RecordData) => r.status === "running")
                  .length > 0 && (
                  <span className="nav-count">
                    {
                      detail.runs.filter(
                        (r: RecordData) => r.status === "running",
                      ).length
                    }
                  </span>
                )}
            </ActionButton>
          ))}
        </nav>
        <div className="sidebar-bottom">
          <div className="runtime-dot" />
          <div>
            <strong>{project ? "GitHub 项目" : "打开 GitHub 仓库"}</strong>
            <p>工程流程与角色讨论</p>
          </div>
        </div>
      </aside>
      <div className="main">
        <header>
          <div className="breadcrumb">
            <FolderOpen size={15} />
            <span>{project?.title || "工作空间"}</span>
            <ChevronRight size={14} />
            <span>{f?.title || "总览"}</span>
          </div>
          <div className="header-right">
            <span
              className={`connection ${connection === "已连接" ? "online" : ""}`}
            >
              <CircleDot size={12} />
              {connection}
            </span>
            <span className="repository-status">
              {project
                ? repositoryReady
                  ? "GitHub 已接入"
                  : "仓库接入中"
                : "尚未打开仓库"}
            </span>
          </div>
        </header>
        <main>
          {error && (
            <div className="error" role="alert">
              <AlertTriangle size={16} />
              <span>{error}</span>
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
            />
          )}
          {project &&
            repositoryReady &&
            !data?.configuration_ready &&
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
              <h1>{nav.find((n) => n[0] === tab)?.[1]}</h1>
              <p>
                {tab === "overview"
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
                            : "内核数据与执行能力。"}
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
                {connection === "已断开"
                  ? "内核已退出，请连接其他内核或重新启动后连接。"
                  : error
                    ? "当前内核无法连接，可重试或连接其他内核。"
                    : "正在连接内核…"}
              </Empty>
              <KernelConnection />
            </section>
          )}
          {data && repositoryReady && tab === "overview" && (
            <>
              <div className="metrics">
                <Metric
                  title="项目 Features"
                  value={data.summary?.features ?? data.total_features}
                  icon={<Layers3 />}
                  onClick={() => {
                    setTab("execution");
                    setWorkFilter("");
                    setFeatureId("");
                    setTaskFilter("");
                    setFeaturePage(0);
                    setExecutionTab("overview");
                  }}
                />
                <Metric
                  title="项目 Tasks"
                  value={data.summary?.tasks || 0}
                  icon={<Workflow />}
                  onClick={() => {
                    setTab("execution");
                    setWorkFilter("tasks");
                    setFeatureId("");
                    setTaskFilter("");
                    setFeaturePage(0);
                    setExecutionTab("overview");
                  }}
                />
                <Metric
                  title="运行中"
                  value={data.summary?.running || 0}
                  icon={<Activity />}
                  onClick={() => {
                    setTab("execution");
                    setWorkFilter("running");
                    setFeatureId("");
                    setTaskFilter("");
                    setFeaturePage(0);
                    setExecutionTab("execution");
                  }}
                />
                <Metric
                  title="等待评审"
                  value={data.summary?.waiting_review || 0}
                  icon={<ShieldCheck />}
                  onClick={() => {
                    setTab("execution");
                    setWorkFilter("waiting_review");
                    setFeatureId("");
                    setTaskFilter("");
                    setFeaturePage(0);
                    setExecutionTab("delivery");
                  }}
                />
                <Metric
                  title="等待人确认"
                  value={data.summary?.waiting_confirmation || 0}
                  icon={<Pause />}
                  onClick={() => {
                    setTab("execution");
                    setWorkFilter("waiting_confirmation");
                    setFeatureId("");
                    setTaskFilter("");
                    setFeaturePage(0);
                    setExecutionTab("overview");
                  }}
                />
                <Metric
                  title="需要处理"
                  value={data.summary?.blocked || 0}
                  icon={<AlertTriangle />}
                  onClick={() => {
                    setTab("execution");
                    setWorkFilter("blocked");
                    setFeatureId("");
                    setTaskFilter("");
                    setFeaturePage(0);
                    setExecutionTab("overview");
                  }}
                />
              </div>
              <div className="overview-grid">
                <section className="panel">
                  <div className="panel-heading">
                    <h2>
                      Feature 工作队列 <span>{data.total_features}</span>
                    </h2>
                    <span className="muted">持久化工程库</span>
                  </div>
                  {features.length === 0 ? (
                    <Empty>创建第一个 Feature，开始规划与交付。</Empty>
                  ) : (
                    <div className="feature-list">
                      {features.map((feature) => (
                        <ActionButton
                          className={`feature-row ${feature.id === featureId ? "selected" : ""}`}
                          key={feature.id}
                          onClick={() => {
                            setFeatureId(feature.id);
                            setTaskFilter("");
                            setDetailPages({});
                          }}
                        >
                          <div className="feature-icon">
                            <Layers3 size={18} />
                          </div>
                          <div>
                            <strong>{feature.title}</strong>
                            <p>
                              {short(feature.id)} · 输入 v
                              {feature.input_version} · revision{" "}
                              {feature.revision}
                            </p>
                          </div>
                          <Badge value={feature.status} />
                          <ChevronRight size={16} />
                        </ActionButton>
                      ))}
                    </div>
                  )}
                  <Pagination
                    page={featurePage}
                    setPage={setFeaturePage}
                    hasMore={(featurePage + 1) * 50 < data.total_features}
                  />
                </section>
                <section className="panel journey">
                  <span className="eyebrow">THE ENGINEERING LOOP</span>
                  <h2>完整工程闭环</h2>
                  {[
                    "需求与设计规划",
                    "版本化交付与 Review",
                    "独立 Task 并行执行",
                    "候选验证与 Task Merge",
                    "Feature 收尾 → mainline",
                  ].map((step, i) => (
                    <div className="journey-step" key={step}>
                      <span>{String(i + 1).padStart(2, "0")}</span>
                      <div>
                        <strong>{step}</strong>
                        <p>
                          {
                            [
                              "锁定输入、规范与绑定",
                              "冻结证据，记录逐项决定",
                              "Red → 实现 → Green",
                              "目标变化后重新验收",
                              "保存报告、快照与历史",
                            ][i]
                          }
                        </p>
                      </div>
                    </div>
                  ))}
                  <div className="notice">
                    项目连接 GitHub，运行、测试和交付使用实际执行证据。
                  </div>
                </section>
              </div>
              {f && (
                <section className="panel feature-summary">
                  <div className="panel-heading">
                    <div>
                      <span className="eyebrow">SELECTED FEATURE</span>
                      <h2>{f.title}</h2>
                    </div>
                    <Badge value={f.status} />
                  </div>
                  <div className="actions">
                    {act(f.id, "task.create", "创建 Task", () =>
                      show({
                        title: "创建独立 Task",
                        method: "task.create",
                        target: f.id,
                        fields: [
                          { key: "title", label: "Task 名称" },
                          {
                            key: "acceptance_keys",
                            label:
                              "实现的验收条目标识（逗号分隔，留空继承全部）",
                            optional: true,
                          },
                          {
                            key: "dependencies",
                            label: "依赖 Task ID（逗号分隔，可空）",
                            optional: true,
                          },
                        ],
                      }),
                    )}
                    <ActionButton onClick={() => setTab("execution")}>
                      进入执行工作台 <ArrowRight size={15} />
                    </ActionButton>
                  </div>
                  <p className="muted">
                    {detail.tasks.length} Tasks · {detail.attempts.length}{" "}
                    Attempts · {detail.deliveries.length} Deliveries ·{" "}
                    {detail.snapshots.length} Snapshots
                  </p>
                </section>
              )}
            </>
          )}
          {data && repositoryReady && tab === "execution" && (
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
                    <option value={f.id}>{f.title}</option>
                  )}
                  {features.map((x) => (
                    <option key={x.id} value={x.id}>
                      {x.title}
                    </option>
                  ))}
                </ActionSelect>
              </label>
              {f && (
                <>
                  <Badge value={f.status} />
                  <span className="muted">
                    输入 v{f.input_version} · revision {f.revision}
                  </span>
                </>
              )}
            </div>
          )}
          {data && tab === "discussion" && (
            <RoleConversation
              key={
                (project?.id || "global") +
                (focusedSession && focusedSession.project_id === project?.id
                  ? focusedSession.id
                  : "")
              }
              initialOwner={
                focusedSession && focusedSession.project_id === project?.id
                  ? focusedSession.owner_id
                  : featureId || undefined
              }
              initialSession={
                focusedSession && focusedSession.project_id === project?.id
                  ? focusedSession.id
                  : undefined
              }
              project={project}
              feature={f}
              tasks={detail?.tasks || []}
              agents={definitions}
              configurations={data.configurations || []}
              onLocateExperience={async (entry) => {
                if (entry.project_id === "00000000-0000-4000-8000-000000000000")
                  return;
                if (entry.project_id !== projectId)
                  await openProject(entry.project_id);
                setFeatureId(entry.feature_id);
                setTaskFilter(entry.task_id || "");
                setExecutionTab("context");
                setTab("execution");
              }}
              installations={data.runtime_installations || []}
              request={rpc}
              refresh={load}
            />
          )}
          {tab === "execution" && f && repositoryReady && (
            <>
              <div className="object-heading">
                <div>
                  <h2>
                    {taskFilter
                      ? detail.tasks.find(
                          (t: RecordData) => t.id === taskFilter,
                        )?.title || taskFilter
                      : f.title}
                  </h2>
                  <p>
                    {taskFilter ? "Task" : "Feature"} · {f.title} ·{" "}
                    {configurationLock
                      ? "配置已冻结"
                      : "历史配置（保留原执行锁）"}
                  </p>
                </div>
                <label>
                  查看对象
                  <ActionSelect
                    aria-label="执行对象"
                    value={taskFilter}
                    onChange={(e) => setTaskFilter(e.target.value)}
                  >
                    <option value="">Feature · {f.title}</option>
                    {detail.tasks.map((t: RecordData) => (
                      <option key={t.id} value={t.id}>
                        Task · {t.title}
                      </option>
                    ))}
                  </ActionSelect>
                </label>
              </div>
              <div className="detail-tabs" role="tablist" aria-label="执行详情">
                {[
                  ["overview", "总览"],
                  ["execution", "执行"],
                  ["delivery", "交付与评审"],
                  ["documents", "文档"],
                  ["context", "上下文"],
                  ["tests", "测试"],
                ].map(([key, label]) => (
                  <ActionButton
                    key={key}
                    role="tab"
                    aria-selected={executionTab === key}
                    onClick={() => setExecutionTab(key)}
                  >
                    {label}
                  </ActionButton>
                ))}
              </div>
              {executionTab === "overview" && (
                <>
                  <p className="notice">
                    {f.title} ·{" "}
                    {taskFilter
                      ? "独立 Task 范围与继承基线"
                      : "需求、规划、Task 执行到主干交付"}
                    。文档、上下文和测试分别在上方页签中查看。
                  </p>
                  {data?.configurations
                    ?.filter(
                      (r: RecordData) =>
                        r.kind === "workflow_instance" &&
                        r.definition_id === (taskFilter || f.id) &&
                        r.data.status !== "cancelled",
                    )
                    .map((r: RecordData) => (
                      <WorkflowInstanceView
                        key={r.id}
                        record={r}
                        projectId={projectId}
                        request={rpc}
                        refresh={load}
                        onDiscussion={(id, owner) => {
                          setTaskFilter(owner === f.id ? "" : owner);
                          setFocusedSession({
                            id,
                            owner_id: owner,
                            project_id: projectId,
                          });
                          setTab("discussion");
                        }}
                      />
                    ))}
                  {configurationLock && (
                    <ActionButton
                      onClick={() =>
                        show({
                          title: "基于当前 Project 配置创建后继 Feature",
                          method: "feature.create",
                          payload: {
                            project_id: projectId,
                            predecessor: f.id,
                            require_configuration: true,
                          },
                          fields: [
                            {
                              key: "title",
                              label: "后继 Feature 名称",
                              value: f.title + " · 后继",
                            },
                          ],
                        })
                      }
                    >
                      创建后继 Feature
                    </ActionButton>
                  )}
                </>
              )}
              {executionTab === "tests" && (
                <section className="panel">
                  <h2>测试定义与执行结果</h2>
                  <p className="muted">
                    定义修订、Red / Green、代码版本、环境及证据有效性分别保留。
                  </p>
                  {detail.documents
                    .filter(
                      (d: RecordData) =>
                        d.kind === "test" &&
                        (!taskFilter || !d.task_id || d.task_id === taskFilter),
                    )
                    .map((d: RecordData) => (
                      <details key={d.id}>
                        <summary>
                          {d.title} v{d.version} ·{" "}
                          {d.task_id ? "Task" : "Feature"} ·{" "}
                          {d.verifies?.join("、")}
                        </summary>
                        <pre>{pretty(d.test_recipe || {})}</pre>
                        <MarkdownPreview text={d.body} />
                      </details>
                    ))}
                  {detail.runs
                    .filter(
                      (r: RecordData) =>
                        [
                          "red",
                          "green",
                          "feature_tests",
                          "merge_verify",
                          "test_author",
                          "verify",
                        ].includes(r.node) &&
                        (!taskFilter ||
                          detail.attempts.some(
                            (a: RecordData) =>
                              a.id === r.attempt_id && a.task_id === taskFilter,
                          ) ||
                          detail.merges.some(
                            (m: RecordData) =>
                              m.id === r.merge_id && m.task_id === taskFilter,
                          )),
                    )
                    .map((r: RecordData) => (
                      <details key={r.id}>
                        <summary>
                          {labels[r.node] || r.node} ·{" "}
                          {labels[r.status] || r.status} · {r.origin}
                        </summary>
                        <pre>{pretty(r)}</pre>
                        {r.artifact_id && (
                          <ActionButton
                            onClick={() => openArtifact(r.artifact_id)}
                          >
                            读取证据
                          </ActionButton>
                        )}
                      </details>
                    ))}
                </section>
              )}
            </>
          )}
          {tab === "execution" &&
            f &&
            repositoryReady &&
            ["overview", "execution", "delivery"].includes(executionTab) && (
              <>
                {detail.pending_interactions?.length > 0 && (
                  <section className="notice">
                    <h3>工程执行等待输入</h3>
                    {detail.pending_interactions.map((i: RecordData) => (
                      <ActionButton
                        key={i.id}
                        onClick={() => {
                          const session = detail.sessions.find(
                            (s: RecordData) => s.id === i.session_id,
                          );
                          if (session) {
                            setFocusedSession(session);
                            setTab("discussion");
                          }
                        }}
                      >
                        {i.kind === "tool_approval"
                          ? "处理工具审批"
                          : "回答 Agent 问题"}{" "}
                        · {i.reason}
                      </ActionButton>
                    ))}
                  </section>
                )}
                <div className="panel toolbar">
                  <div className="actions">
                    {act(f.id, "task.create", "创建 Task", () =>
                      show({
                        title: "创建独立 Task",
                        method: "task.create",
                        target: f.id,
                        fields: [
                          { key: "title", label: "Task 名称" },
                          {
                            key: "acceptance_keys",
                            label:
                              "实现的验收条目标识（逗号分隔，留空继承全部）",
                            optional: true,
                          },
                          {
                            key: "dependencies",
                            label: "依赖 Task ID（逗号分隔，可空）",
                            optional: true,
                          },
                        ],
                      }),
                    )}
                    {detail.allowed_actions[f.id]?.attempt_phases?.includes(
                      "closure",
                    ) &&
                      act(f.id, "attempt.start", "收尾 Attempt", () =>
                        begin(f.id, "closure"),
                      )}
                    {act(f.id, "merge.request", "Feature → mainline", () =>
                      command("merge.request", f.id, {}).catch(() => {}),
                    )}
                  </div>
                </div>
                <section
                  className="panel"
                  hidden={!["overview"].includes(executionTab)}
                >
                  <div className="panel-heading">
                    <h2>独立 Tasks</h2>
                    <span className="muted">
                      {"Git worktree · 独立角色会话与 Docker 实例"}
                    </span>
                  </div>
                  {detail.tasks.length === 0 ? (
                    <Empty>规划交付接受后，创建独立 Task。</Empty>
                  ) : (
                    <div className="task-grid">
                      {detail.tasks.map((t: RecordData) => (
                        <article className="task-card" key={t.id}>
                          <div className="row">
                            <h3 title={t.title}>{t.title}</h3>
                            <Badge value={t.status} />
                          </div>
                          {t.title.length > 180 && (
                            <details>
                              <summary>完整任务说明</summary>
                              <p>{t.title}</p>
                            </details>
                          )}
                          <p className="mono">{t.worktree}</p>
                          <p className="muted">
                            继承快照 {short(t.inherited_snapshot)} ·{" "}
                            {t.dependencies.length} 个依赖
                          </p>
                          <p className="muted">
                            验收范围：
                            {t.scope?.acceptance?.join("、") ||
                              "历史范围待补齐"}{" "}
                            · 需求 {short(t.scope?.requirement)} · 设计{" "}
                            {short(t.scope?.design)}
                          </p>
                          <div className="actions">
                            {act(
                              t.id,
                              "attempt.start",
                              "启动 Attempt",
                              () => begin(t.id),
                              true,
                            )}
                            {act(t.id, "merge.request", "申请 Merge", () =>
                              command("merge.request", t.id, {}).catch(
                                () => {},
                              ),
                            )}
                            {act(t.id, "task.replan", "更新规划范围", () =>
                              show({
                                title: "采用当前已接受规划",
                                method: "task.replan",
                                target: t.id,
                                fields: [
                                  {
                                    key: "acceptance_keys",
                                    label:
                                      "验收条目标识（逗号分隔，留空继承全部）",
                                    optional: true,
                                  },
                                ],
                              }),
                            )}
                            <ActionButton
                              onClick={() =>
                                setTaskFilter(taskFilter === t.id ? "" : t.id)
                              }
                            >
                              {taskFilter === t.id ? "显示全部" : "查看执行"}
                            </ActionButton>
                          </div>
                          <ActionButton
                            className="text-button"
                            onClick={() =>
                              setView({
                                title: "Task · 绑定与继承",
                                body: pretty(t),
                              })
                            }
                          >
                            查看锁定版本
                          </ActionButton>
                        </article>
                      ))}
                    </div>
                  )}
                  {pager("tasks")}
                </section>
                <section
                  className="panel"
                  hidden={executionTab !== "execution"}
                >
                  <div className="panel-heading">
                    <h2>Attempts 与 Run DAG</h2>
                    <ActionButton
                      className="text-button"
                      onClick={() => setTaskFilter("")}
                    >
                      Feature 自身 + 全部 Tasks
                    </ActionButton>
                  </div>
                  {detail.attempts.filter(
                    (a: RecordData) => !taskFilter || a.task_id === taskFilter,
                  ).length === 0 ? (
                    <Empty>启动一个 Attempt，后端将异步调度 Run DAG。</Empty>
                  ) : (
                    detail.attempts
                      .filter(
                        (a: RecordData) =>
                          !taskFilter || a.task_id === taskFilter,
                      )
                      .map((a: RecordData) => (
                        <div className="attempt" key={a.id}>
                          <div className="row">
                            <div>
                              <span className="eyebrow">
                                {a.task_id
                                  ? detail.tasks.find(
                                      (t: RecordData) => t.id === a.task_id,
                                    )?.title
                                  : "FEATURE 自身执行"}
                              </span>
                              <h3>
                                {labels[a.phase]} <code>{short(a.id)}</code>
                              </h3>
                            </div>
                            <div className="actions">
                              <Badge value={a.status} />
                              {a.paused && <Badge value="paused" />}
                              {act(
                                a.id,
                                "attempt.pause",
                                a.paused ? "恢复派发" : "暂停派发",
                                () =>
                                  command("attempt.pause", a.id, {
                                    paused: !a.paused,
                                  }).catch(() => {}),
                              )}
                              {act(a.id, "attempt.cancel", "取消 Attempt", () =>
                                command("attempt.cancel", a.id).catch(() => {}),
                              )}
                              <ActionButton
                                onClick={() =>
                                  setView({
                                    title:
                                      "Attempt 固定输入 / StandardLock / BindingLock",
                                    body: pretty(a),
                                  })
                                }
                              >
                                输入与锁
                              </ActionButton>
                            </div>
                          </div>
                          <div className="run-table">
                            {detail.runs
                              .filter((r: RecordData) => r.attempt_id === a.id)
                              .slice()
                              .reverse()
                              .map((r: RecordData) => (
                                <div className="run-row" key={r.id}>
                                  <span className={`run-dot ${r.status}`} />
                                  <div className="run-name">
                                    <strong>{labels[r.node] || r.node}</strong>
                                    <span>
                                      {short(r.id)}
                                      {r.retry_of &&
                                        ` · retry_of ${short(r.retry_of)}`}
                                    </span>
                                  </div>
                                  <span className="run-deps">
                                    {r.dependencies.length
                                      ? `← ${r.dependencies.join(", ")}`
                                      : "独立节点"}
                                    {r.writes ? " · 写许可" : ""}
                                  </span>
                                  <Badge value={r.status} />
                                  <div className="actions">
                                    {[
                                      "failed",
                                      "cancelled",
                                      "interrupted",
                                    ].includes(r.status) && (
                                      <ActionButton
                                        title="技术重试"
                                        onClick={() =>
                                          command("run.retry", r.id).catch(
                                            () => {},
                                          )
                                        }
                                      >
                                        <RotateCcw size={13} /> 重试
                                      </ActionButton>
                                    )}
                                    {["running", "queued"].includes(
                                      r.status,
                                    ) && (
                                      <ActionButton
                                        onClick={() =>
                                          command("run.cancel", r.id).catch(
                                            () => {},
                                          )
                                        }
                                      >
                                        取消
                                      </ActionButton>
                                    )}
                                    <ActionButton
                                      onClick={() =>
                                        setView({
                                          title: "Run 日志 / 输出 / Session",
                                          body: pretty({
                                            run: r,
                                            session: detail.sessions.find(
                                              (x: RecordData) =>
                                                x.id === r.session_id,
                                            ),
                                            resources: detail.resources.filter(
                                              (x: RecordData) =>
                                                x.run_id === r.id,
                                            ),
                                            blocked:
                                              detail.blocked_reasons[r.id],
                                          }),
                                        })
                                      }
                                    >
                                      详情
                                    </ActionButton>
                                  </div>
                                  {detail.blocked_reasons[r.id] && (
                                    <div className="run-blocked">
                                      {detail.blocked_reasons[r.id]}
                                    </div>
                                  )}
                                </div>
                              ))}
                          </div>
                          <div className="actions delivery-actions">
                            {act(
                              a.id,
                              "delivery.submit",
                              "提交交付",
                              () =>
                                command("delivery.submit", a.id).catch(
                                  () => {},
                                ),
                              true,
                            )}
                          </div>
                        </div>
                      ))
                  )}
                  {pager("attempts")}
                  {pager("runs")}
                </section>
                <section
                  className="panel"
                  hidden={!["delivery"].includes(executionTab)}
                >
                  <div className="panel-heading">
                    <h2>交付与 Review</h2>
                    <span className="muted">
                      冻结版本 · 逐项证据 · 人类例外
                    </span>
                  </div>
                  {detail.deliveries.length === 0 ? (
                    <Empty>执行结束后提交正式交付。</Empty>
                  ) : (
                    detail.deliveries.map(deliveryCard)
                  )}
                  {pager("deliveries")}
                </section>
                <section
                  className="panel"
                  hidden={!["delivery"].includes(executionTab)}
                >
                  <div className="panel-heading">
                    <h2>Merge 候选</h2>
                    {project?.execution_mode === "live" && (
                      <ActionButton
                        disabled={busy}
                        onClick={async () => {
                          const target = currentActionTarget();
                          try {
                            const policy = await rpc("github.diagnose", {
                              repository:
                                project.workspace_definition.repository,
                              branch: project.workspace_definition.mainline,
                            });
                            if (!policy.head)
                              throw new Error("远端主干版本不可用");
                            await command(
                              "repository.refresh",
                              project.id,
                              { revision: policy.head },
                              project.revision,
                            );
                          } catch (error) {
                            publishActionFeedback(target, String(error), true);
                          }
                        }}
                      >
                        核对并快进主干
                      </ActionButton>
                    )}
                    <GitMerge size={18} />
                  </div>
                  {detail.mainline_sync?.map((operation: RecordData) => (
                    <p className="notice" key={operation.id}>
                      主干同步 · {operation.status} · {operation.reason}
                      {operation.status === "needs_reconciliation" && (
                        <ActionButton
                          disabled={busy}
                          onClick={() =>
                            command(
                              "repository.reconcile",
                              operation.id,
                              {},
                              operation.revision,
                            ).catch(() => {})
                          }
                        >
                          核对主干同步
                        </ActionButton>
                      )}
                    </p>
                  ))}
                  {detail.merges.length === 0 ? (
                    <Empty>接受交付后，申请候选并验证。</Empty>
                  ) : (
                    detail.merges.map((m: RecordData) => (
                      <article className="delivery" key={m.id}>
                        <div className="row">
                          <h3>
                            {m.task_id
                              ? "Task → Feature"
                              : "Feature → mainline"}
                          </h3>
                          <Badge value={m.status} />
                        </div>
                        <div className="run-table">
                          {detail.runs
                            .filter((r: RecordData) => r.merge_id === m.id)
                            .slice()
                            .reverse()
                            .map((r: RecordData) => (
                              <RunControls
                                key={r.id}
                                run={r}
                                perform={(method, payload) =>
                                  command(method, r.id, payload).catch(() => {})
                                }
                                inspect={() =>
                                  setView({
                                    title: "Merge Run / Session / Resource",
                                    body: pretty({
                                      run: r,
                                      session: detail.sessions.find(
                                        (x: RecordData) =>
                                          x.id === r.session_id,
                                      ),
                                      resources: detail.resources.filter(
                                        (x: RecordData) => x.run_id === r.id,
                                      ),
                                    }),
                                  })
                                }
                              />
                            ))}
                        </div>
                        <p className="mono">
                          source {m.source}
                          <br />
                          expected target {m.expected_target}
                          <br />
                          candidate {m.candidate}
                        </p>
                        {m.change_request?.provider === "github" && (
                          <p>
                            <a
                              href={`https://github.com/${m.change_request.repository}/pull/${m.change_request.external_id}`}
                              target="_blank"
                              rel="noopener noreferrer"
                            >
                              GitHub PR #{m.change_request.external_id}
                            </a>{" "}
                            · 本地{" "}
                            {m.committed_revision ? "已接纳" : "尚未接纳"} ·
                            远端{" "}
                            {m.status === "completed"
                              ? "已核验 merged"
                              : m.committed_revision
                                ? "同步 / merged 核对中"
                                : "等待接纳"}
                          </p>
                        )}
                        {detail.merge_progress
                          ?.find((p: RecordData) => p.merge === m.id)
                          ?.remote_operations?.filter(
                            (op: RecordData) => op.status !== "completed",
                          )
                          .map((op: RecordData) => (
                            <p key={op.id} className="notice">
                              {op.action.kind} · {op.status} · {op.reason}
                              {op.status === "needs_reconciliation" &&
                                op.reason !== "superseded_target" && (
                                  <ActionButton
                                    onClick={() =>
                                      command(
                                        "repository.reconcile",
                                        op.id,
                                        {},
                                        op.revision,
                                      ).catch(() => {})
                                    }
                                  >
                                    核对远端操作
                                  </ActionButton>
                                )}
                            </p>
                          ))}
                        {detail.advances?.find(
                          (intent: RecordData) => intent.id === m.id,
                        )?.status === "needs_reconciliation" && (
                          <p className="error">
                            {
                              detail.advances.find(
                                (intent: RecordData) => intent.id === m.id,
                              ).reason
                            }
                          </p>
                        )}
                        <div className="actions">
                          {act(m.id, "merge.reconcile", "核对合入结果", () =>
                            command("merge.reconcile", m.id).catch(() => {}),
                          )}
                          {act(
                            m.id,
                            "merge.apply",
                            "接纳候选并同步 GitHub",
                            () => command("merge.apply", m.id).catch(() => {}),
                            true,
                          )}
                          <ActionButton
                            onClick={() =>
                              setView({
                                title: "Merge 固定源/目标与验证 Runs",
                                body: pretty({
                                  merge: m,
                                  advance: detail.advances?.find(
                                    (intent: RecordData) => intent.id === m.id,
                                  ),
                                  runs: detail.runs.filter(
                                    (r: RecordData) => r.merge_id === m.id,
                                  ),
                                }),
                              })
                            }
                          >
                            候选与验证
                          </ActionButton>
                          <span className="muted">
                            {detail.allowed_actions[m.id]?.blocked.join(" · ")}
                          </span>
                        </div>
                      </article>
                    ))
                  )}
                  {pager("merges")}
                </section>
                <section
                  className="panel"
                  hidden={executionTab !== "execution"}
                  aria-label="外部操作核对"
                >
                  <h2>外部操作与资源核对</h2>
                  {(detail.effects || []).map((effect: RecordData) => (
                    <div className="document-row" key={effect.id}>
                      <div>
                        <strong>
                          {effect.kind} · {short(effect.run_id)}
                        </strong>
                        <p>
                          {effect.status} · {effect.reason}
                        </p>
                      </div>
                      <ActionButton
                        onClick={() =>
                          setView({
                            title: "外部调用意图与回执",
                            body: pretty(effect),
                          })
                        }
                      >
                        查看回执
                      </ActionButton>
                    </div>
                  ))}
                  {pager("effects")}
                </section>
              </>
            )}
          {tab === "execution" &&
            ["documents", "context", "tests"].includes(executionTab) &&
            f &&
            repositoryReady && (
              <>
                <section
                  className="panel"
                  hidden={executionTab !== "documents"}
                >
                  <div className="panel-heading">
                    <h2>需求 / 设计 / 测试 / 文档修订</h2>
                    {act(f.id, "document.revise", "新建修订", () =>
                      show({
                        title: "保存不可变文档修订 · 旧证据将失效",
                        method: "document.revise",
                        target: f.id,
                        fields: [
                          {
                            key: "kind",
                            label: "类型",
                            type: "select",
                            options: [
                              "requirement",
                              "design",
                              "test",
                              "doc",
                            ].map((value) => ({ value, label: labels[value] })),
                          },
                          { key: "title", label: "标题" },
                          {
                            key: "acceptance",
                            label: "需求验收条目（每行：标识 | 可验证条件）",
                            type: "textarea",
                            optional: true,
                          },
                          {
                            key: "verifies",
                            label: "测试验证的验收条目标识（逗号分隔）",
                            optional: true,
                          },
                          {
                            key: "task_id",
                            label: "Task ID（可空）",
                            optional: true,
                          },
                          {
                            key: "body",
                            label: "内容",
                            type: "markdown",
                          },
                        ],
                      }),
                    )}
                  </div>
                  {detail.documents.length === 0 ? (
                    <Empty>先输入需求，或启动规划生成可编辑建议。</Empty>
                  ) : (
                    detail.documents
                      .filter(
                        (d: RecordData) =>
                          !taskFilter || !d.task_id || d.task_id === taskFilter,
                      )
                      .map((d: RecordData) => (
                        <div className="document-row" key={d.id}>
                          <FileText size={19} />
                          <div>
                            <strong>{d.title}</strong>
                            <p>
                              {labels[d.kind]} v{d.version} · {d.origin} ·
                              Requirement {short(d.requirement_id)} → Design{" "}
                              {short(d.design_id)}
                              {d.task_id && ` → Task ${short(d.task_id)}`}
                              {d.verifies?.length > 0 &&
                                ` · 验证 ${d.verifies.join("、")}`}
                              {d.acceptance?.length > 0 &&
                                ` · 验收条目 ${d.acceptance.map((c: RecordData) => c.key).join("、")}`}
                            </p>
                          </div>
                          <ActionButton
                            onClick={() =>
                              setView({
                                title: `${d.title} · v${d.version}`,
                                body: d.body,
                                format: "markdown",
                                metadata: d,
                              })
                            }
                          >
                            阅读
                          </ActionButton>
                          {act(f.id, "document.revise", "修订", () =>
                            show({
                              title: "修订文档 · 旧证据将失效",
                              method: "document.revise",
                              target: f.id,
                              payload: {
                                kind: d.kind,
                                task_id: d.task_id,
                                title: d.title,
                              },
                              fields: [
                                {
                                  key: "acceptance",
                                  label:
                                    "需求验收条目（每行：标识 | 可验证条件）",
                                  type: "textarea",
                                  optional: true,
                                  value: (d.acceptance || [])
                                    .map(
                                      (c: RecordData) =>
                                        `${c.key} | ${c.description}`,
                                    )
                                    .join("\n"),
                                },
                                {
                                  key: "verifies",
                                  label: "测试验证的验收条目标识（逗号分隔）",
                                  optional: true,
                                  value: (d.verifies || []).join(","),
                                },
                                {
                                  key: "body",
                                  label: "内容",
                                  type: "markdown",
                                  value: d.body,
                                },
                              ],
                            }),
                          )}
                        </div>
                      ))
                  )}
                  {pager("documents")}
                </section>
                <section className="panel" hidden={executionTab !== "context"}>
                  <div className="panel-heading">
                    <h2>动态 Context 与不可变 Snapshot</h2>
                    {act(f.id, "snapshot.save", "保存 Feature Snapshot", () =>
                      command("snapshot.save", f.id).catch(() => {}),
                    )}
                  </div>
                  {detail.contexts
                    .filter(
                      (c: RecordData) =>
                        !taskFilter || c.owner_id === taskFilter,
                    )
                    .map((c: RecordData) => (
                      <div className="document-row" key={c.id}>
                        <Terminal size={18} />
                        <div>
                          <strong>
                            {c.owner_id === f.id
                              ? "Feature Context"
                              : `Task Context · ${short(c.owner_id)}`}
                          </strong>
                          <p>
                            revision {c.revision} · {c.content.slice(0, 100)}
                          </p>
                        </div>
                        <ActionButton
                          onClick={() =>
                            show({
                              title: "更新独立 Context",
                              method: "context.update",
                              target: c.id,
                              fields: [
                                {
                                  key: "content",
                                  label: "上下文内容",
                                  type: "textarea",
                                  value: c.content,
                                },
                              ],
                            })
                          }
                        >
                          编辑
                        </ActionButton>
                        <ActionButton
                          onClick={() =>
                            command("snapshot.save", c.owner_id).catch(() => {})
                          }
                        >
                          保存 Snapshot
                        </ActionButton>
                      </div>
                    ))}
                  {detail.snapshots
                    .filter(
                      (s: RecordData) =>
                        !taskFilter || s.task_id === taskFilter,
                    )
                    .map((s: RecordData) => (
                      <div className="document-row" key={s.id}>
                        <Copy size={18} />
                        <div>
                          <strong>
                            {s.kind} Snapshot · {short(s.id)}
                          </strong>
                          <p>
                            {s.task_id ? "Task" : "Feature"} ·{" "}
                            {s.documents.length} 个固定文档 · inherited{" "}
                            {short(s.inherited_from)}
                          </p>
                        </div>
                        <ActionButton
                          onClick={() =>
                            setView({
                              title: "不可变 Snapshot / 重建引用",
                              body: pretty(s),
                            })
                          }
                        >
                          查看
                        </ActionButton>
                      </div>
                    ))}
                  {pager("tasks")}
                  {pager("snapshots")}
                  {detail.snapshots.length > 1 && (
                    <div className="snapshot-compare">
                      <label>
                        快照 A
                        <ActionSelect
                          value={snapshotA}
                          onChange={(e) => setSnapshotA(e.target.value)}
                        >
                          <option value="">请选择</option>
                          {detail.snapshots
                            .filter(
                              (s: RecordData) =>
                                !taskFilter || s.task_id === taskFilter,
                            )
                            .map((s: RecordData) => (
                              <option value={s.id} key={s.id}>
                                {s.kind} · {short(s.id)}
                              </option>
                            ))}
                        </ActionSelect>
                      </label>
                      <label>
                        快照 B
                        <ActionSelect
                          value={snapshotB}
                          onChange={(e) => setSnapshotB(e.target.value)}
                        >
                          <option value="">请选择</option>
                          {detail.snapshots
                            .filter(
                              (s: RecordData) =>
                                !taskFilter || s.task_id === taskFilter,
                            )
                            .map((s: RecordData) => (
                              <option value={s.id} key={s.id}>
                                {s.kind} · {short(s.id)}
                              </option>
                            ))}
                        </ActionSelect>
                      </label>
                      <ActionButton
                        disabled={!snapshotA || !snapshotB}
                        onClick={() =>
                          setView({
                            title: "Snapshot 比较",
                            body: pretty({
                              A: detail.snapshots.find(
                                (s: RecordData) => s.id === snapshotA,
                              ),
                              B: detail.snapshots.find(
                                (s: RecordData) => s.id === snapshotB,
                              ),
                            }),
                          })
                        }
                      >
                        比较
                      </ActionButton>
                    </div>
                  )}
                </section>
                <section className="panel">
                  <div className="panel-heading">
                    <h2>Artifact 文件与报告</h2>
                    <span className="muted">已封存 · SHA256 校验</span>
                  </div>
                  {detail.artifacts.map((a: RecordData) => (
                    <div className="document-row" key={a.id}>
                      <FileText size={16} />
                      <div>
                        <strong>{a.relative_path}</strong>
                        <p>SHA256 {a.sha256}</p>
                      </div>
                      <ActionButton onClick={() => openArtifact(a.id)}>
                        打开
                      </ActionButton>
                    </div>
                  ))}
                  {pager("artifacts")}
                </section>
              </>
            )}
          {tab === "execution" && !f && (
            <section className="panel">
              <h2>Feature 工作队列{workFilter ? ` · ${workFilter}` : ""}</h2>
              {features.map((feature) => (
                <ActionButton
                  className="feature-row"
                  key={feature.id}
                  onClick={() => setFeatureId(feature.id)}
                >
                  {feature.title} · {labels[feature.status] || feature.status}
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
              key={`${tab}:${projectId}`}
              group={tab === "workflows" ? "processes" : "foundations"}
              request={rpc}
              project={project}
              installations={data.runtime_installations || []}
              refresh={load}
            />
          )}
          {data && ["actors", "definitions"].includes(tab) && (
            <AgentCatalog
              key={`actors:${projectId}`}
              request={rpc}
              project={project}
              installations={data.runtime_installations || []}
              group="actors"
            />
          )}
          {data && tab === "projects" && (
            <ProjectManager
              key={`project-manager:${projectId || "global"}`}
              request={rpc}
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
                  historyTarget.current = currentActionTarget();
                  setHistoryScope(e.target.value);
                  setHistoryPage(0);
                }}
              >
                <option value="project">当前项目</option>
                <option value="global">Global 配置与发布</option>
                <option value="tools">Tools调用记录</option>
              </ActionSelect>
            </label>
          )}
          {data && tab === "history" && historyScope === "tools" && (
            <>
              <label>
                调用记录归属
                <ActionSelect
                  value={project ? toolsHistoryScope : "global"}
                  onChange={(e) => setToolsHistoryScope(e.target.value)}
                >
                  <option value="project" disabled={!project}>
                    当前项目
                  </option>
                  <option value="global">Global 接入</option>
                </ActionSelect>
              </label>
              <CallHistory
                key={`${projectId}:${toolsHistoryScope}`}
                request={rpc}
                project={toolsHistoryScope === "global" ? undefined : project}
              />
            </>
          )}
          {data && tab === "history" && historyScope !== "tools" && (
            <section className="panel">
              <div className="panel-heading">
                <h2>持久化事件</h2>
                <span className="muted">按时间倒序 · 每页 50 条</span>
              </div>
              {history.length === 0 ? (
                <Empty>尚无事件记录。</Empty>
              ) : (
                history.map((e) => (
                  <div className="history-row" key={e.id}>
                    <span className="event-id">#{e.id}</span>
                    <div>
                      <strong>{e.kind}</strong>
                      <p>{e.detail}</p>
                      <code>{e.target}</code>
                    </div>
                    <time>{fmt(e.time)}</time>
                  </div>
                ))
              )}
              <Pagination
                page={historyPage}
                setPage={(page) => {
                  historyTarget.current = currentActionTarget();
                  setHistoryPage(page);
                }}
                hasMore={history.length === 50}
              />
            </section>
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
                    <code>{data.data_root}</code>
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
                    <code>{data.global_database}</code>
                    <p className="muted">并发配置、共享定义与项目索引。</p>
                  </div>
                </div>
                {data.project_database && (
                  <div className="setting-row">
                    <div>
                      <h3>当前项目数据库</h3>
                      <code>{data.project_database}</code>
                      <p className="muted">
                        工程状态、执行历史和产物归属仅限当前项目。
                      </p>
                    </div>
                  </div>
                )}
                {project && (
                  <RepositorySettings
                    key={project.id}
                    project={project}
                    ready={repositoryReady}
                    request={rpc}
                    refresh={load}
                  />
                )}
                {project && (
                  <RuntimePreferences
                    key={project.id}
                    project={project}
                    request={rpc}
                    refresh={load}
                  />
                )}
                <PermissionSettings
                  request={rpc}
                  project={project ?? undefined}
                  refresh={load}
                />
                <RuntimeSettings
                  provider={data.default_harness || "codex"}
                  installations={data.runtime_installations || []}
                  request={rpc}
                  refresh={load}
                />
                <div className="setting-row">
                  <div>
                    <h3>全局并发额度</h3>
                    <p>{data.global_concurrency} 个 Run · 单项目默认 2 个</p>
                  </div>
                  <ActionButton
                    onClick={() =>
                      show({
                        title: "修改全局并发",
                        method: "settings.update",
                        fields: [
                          {
                            key: "global_concurrency",
                            label: "并发额度 1—32",
                            type: "number",
                            value: String(data.global_concurrency),
                          },
                        ],
                      })
                    }
                  >
                    调整
                  </ActionButton>
                </div>
                {project && (
                  <div className="setting-row">
                    <div>
                      <h3>{project.title} · Project 设置</h3>
                      <p className="muted">
                        Workspace: {project.workspace || "未设置"} · 项目并发{" "}
                        {project.concurrency}
                      </p>
                    </div>
                    <div className="actions">
                      <ActionButton onClick={() => setTab("workflows")}>
                        默认绑定
                      </ActionButton>
                      <ActionButton
                        onClick={() =>
                          show({
                            title: "编辑 Project",
                            method: "project.update",
                            target: project.id,
                            fields: [
                              {
                                key: "title",
                                label: "名称",
                                value: project.title,
                              },
                              {
                                key: "workspace",
                                label: "Workspace 描述",
                                value: project.workspace,
                                optional: true,
                              },
                              {
                                key: "concurrency",
                                label: "项目并发额度",
                                type: "number",
                                value: String(project.concurrency),
                              },
                            ],
                          })
                        }
                      >
                        编辑
                      </ActionButton>
                    </div>
                  </div>
                )}
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
              </section>
              <div className="notice">
                Web UI 连接当前 Rust Host。真实工程需配置 Harness、Docker 和
                GitHub；平台验收记录见文档。
              </div>
            </>
          )}
          <footer>
            DevDesk 0.2 <span>工程内核 · 持久化历史 · GitHub 工程接入</span>
          </footer>
        </main>
      </div>
      {dialog && (
        <CommandDialog
          key={dialog.method + (dialog.target || "")}
          spec={dialog}
          busy={busy}
          close={() => setDialog(undefined)}
          submit={async (payload) => {
            try {
              let result;
              if (dialog.method === "shutdown") {
                const response = await fetch("/shutdown", { method: "POST" });
                if (!response.ok)
                  throw new Error(`退出失败 HTTP ${response.status}`);
                kernelStopped.current = true;
                ++requestGeneration.current;
                ++projectGeneration.current;
                setView(undefined);
                setConnection("已断开");
                setCaps(undefined);
                setData(undefined);
              } else if (dialog.method === "settings.update") {
                result = await rpc(dialog.method, payload);
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
            } catch (e) {
              throw e;
            }
          }}
        />
      )}
      {view && (
        <Reader
          title={view.title}
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
  onClick?: () => void;
}) {
  return (
    <ActionButton className="metric" onClick={onClick}>
      <div>
        <span>{title}</span>
        <strong>{value.toString().padStart(2, "0")}</strong>
      </div>
      <div className="metric-icon">{icon}</div>
    </ActionButton>
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
  submit: (p: RecordData) => Promise<void>;
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
      const payload: RecordData = { ...spec.payload };
      for (const field of spec.fields) {
        const value = values[field.key];
        if (field.optional && !value.trim()) continue;
        if (field.type === "markdown" && !field.optional && !value.trim())
          throw new Error(`请填写${field.label}`);
        if (field.key === "acceptance" && value.trim())
          payload.acceptance = value
            .split("\n")
            .filter((line) => line.trim())
            .map((line) => {
              const split = line.indexOf("|");
              if (split < 1) throw new Error("验收条目格式：标识 | 可验证条件");
              return {
                key: line.slice(0, split).trim(),
                description: line.slice(split + 1).trim(),
              };
            });
        else if (field.key === "acceptance" && !value.trim()) continue;
        else if (field.key === "slots") payload[field.key] = JSON.parse(value);
        else if (
          [
            "skills",
            "rules",
            "dependencies",
            "items",
            "verifies",
            "acceptance_keys",
          ].includes(field.key)
        )
          payload[field.key] = value
            .split(",")
            .map((s) => s.trim())
            .filter(Boolean);
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
  return (
    <div className="modal-backdrop">
      <ActionForm
        className="modal"
        ref={formRef}
        role="dialog"
        aria-modal="true"
        aria-label={spec.title}
        onSubmit={send}
      >
        <div className="modal-heading">
          <h2>{spec.title}</h2>
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
            {localError}
          </div>
        )}
        {spec.fields.length === 0 && (
          <p className="muted">确认执行此领域操作。结果将保存到工程历史。</p>
        )}
        {spec.fields.map((f, i) =>
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
              {f.label}
              {f.type === "textarea" ? (
                <textarea
                  autoFocus={i === 0}
                  required={!f.optional}
                  value={values[f.key]}
                  onChange={(e) =>
                    setValues({ ...values, [f.key]: e.target.value })
                  }
                />
              ) : f.type === "select" ? (
                <ActionSelect
                  aria-label={f.label}
                  value={values[f.key]}
                  onChange={(e) =>
                    setValues({ ...values, [f.key]: e.target.value })
                  }
                >
                  {f.options?.map((o) => (
                    <option value={o.value} key={o.value}>
                      {o.label}
                    </option>
                  ))}
                </ActionSelect>
              ) : (
                <input
                  autoFocus={i === 0}
                  required={!f.optional}
                  type={f.type === "number" ? "number" : "text"}
                  value={values[f.key]}
                  onChange={(e) =>
                    setValues({ ...values, [f.key]: e.target.value })
                  }
                />
              )}
              {f.description && <span className="muted">{f.description}</span>}
            </label>
          ),
        )}
        <div className="modal-actions">
          <ActionButton type="button" disabled={busy} onClick={close}>
            取消
          </ActionButton>
          <ActionButton className="primary" disabled={busy}>
            {busy ? "保存中…" : "确认"}
          </ActionButton>
        </div>
      </ActionForm>
    </div>
  );
}
function RunControls({
  run,
  perform,
  inspect,
}: {
  run: RecordData;
  perform: (method: string, payload?: RecordData) => void;
  inspect: () => void;
}) {
  return (
    <div className="run-row">
      <span className={`run-dot ${run.status}`} />
      <div className="run-name">
        <strong>{labels[run.node] || run.node}</strong>
        <span>
          {short(run.id)}
          {run.retry_of && ` · retry_of ${short(run.retry_of)}`}
        </span>
      </div>
      <Badge value={run.status} />
      <div className="actions">
        {["failed", "interrupted", "cancelled"].includes(run.status) && (
          <ActionButton onClick={() => perform("run.retry")}>重试</ActionButton>
        )}
        {["running", "queued"].includes(run.status) && (
          <ActionButton onClick={() => perform("run.cancel")}>
            取消
          </ActionButton>
        )}
        <ActionButton onClick={inspect}>详情</ActionButton>
      </div>
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
  metadata?: RecordData;
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
        aria-label={title}
        onClick={(e) => e.stopPropagation()}
      >
        <div className="modal-heading">
          <h2>{title}</h2>
          <ActionButton aria-label="关闭" onClick={close}>
            <X size={18} />
          </ActionButton>
        </div>
        {format === "markdown" ? (
          <MarkdownPreview text={body} />
        ) : (
          <pre>{body}</pre>
        )}
        {metadata && (
          <details>
            <summary>文档信息</summary>
            <pre>{pretty(metadata)}</pre>
          </details>
        )}
      </section>
    </div>
  );
}
createRoot(document.getElementById("root")!).render(
  <React.StrictMode>
    <App />
  </React.StrictMode>,
);

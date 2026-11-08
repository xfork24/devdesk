import React, {
  useState,
  useEffect,
  useCallback,
  useRef,
  FormEvent,
} from "react";
import { createRoot } from "react-dom/client";
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

type RecordData = { [key: string]: any };
type Field = {
  key: string;
  label: string;
  value?: string;
  type?: "textarea" | "select" | "number";
  options?: { value: string; label: string }[];
  optional?: boolean;
};
type DialogSpec = {
  title: string;
  method: string;
  target?: string;
  payload?: RecordData;
  fields: Field[];
  revision?: number;
};
const labels: Record<string, string> = {
  draft: "草稿",
  planning: "规划中",
  ready: "就绪",
  developing: "开发中",
  validating: "验收中",
  accepted: "已接受",
  merging: "合入中",
  completed: "模拟流程已完成",
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
  });
  if (!res.ok) throw new Error(`HTTP ${res.status} · 本地服务连接失败`);
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
function App() {
  const requestGeneration = useRef(0);
  const projectGeneration = useRef(0);
  const [detailPages, setDetailPages] = useState<Record<string, number>>({});
  const [dialogError, setDialogError] = useState("");
  const [data, setData] = useState<RecordData>();
  const [caps, setCaps] = useState<RecordData>();
  const [projectId, setProjectId] = useState("");
  const [featureId, setFeatureId] = useState("");
  const [tab, setTab] = useState("overview");
  const [error, setError] = useState("");
  const [toast, setToast] = useState("");
  const [connection, setConnection] = useState("连接中");
  const [dialog, setDialog] = useState<DialogSpec>();
  const [busy, setBusy] = useState(false);
  const [history, setHistory] = useState<RecordData[]>([]);
  const [historyPage, setHistoryPage] = useState(0);
  const [featurePage, setFeaturePage] = useState(0);
  const [projectPage, setProjectPage] = useState(0);
  const [view, setView] = useState<{ title: string; body: string }>();
  const [setupPath, setSetupPath] = useState("./data");
  const [scenario, setScenario] = useState("success");
  const [taskFilter, setTaskFilter] = useState("");
  const [snapshotA, setSnapshotA] = useState("");
  const [snapshotB, setSnapshotB] = useState("");
  const load = useCallback(async () => {
    const generation = ++requestGeneration.current;
    try {
      const c = await fetch("/capabilities").then((r) => r.json());
      if (generation !== requestGeneration.current) return;
      setCaps(c);
      if (!c.setup_required) {
        const active = c.active_project_id || "";
        if (projectId !== active) {
          ++projectGeneration.current;
          setProjectId(active);
          setFeatureId("");
          setFeaturePage(0);
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
          feature_id: featureId || undefined,
          offset: featurePage * 50,
          project_offset: projectPage * 50,
          limit: 50,
          detail_pages: detailPages,
        });
        if (generation !== requestGeneration.current) return;
        setData(d);
      }
    } catch (e) {
      if (generation === requestGeneration.current) setError(String(e));
    }
  }, [projectId, featureId, featurePage, projectPage, detailPages]);
  useEffect(() => {
    load();
  }, [load]);
  useEffect(() => {
    if (caps?.setup_required !== false) return;
    let timer: ReturnType<typeof setTimeout> | undefined;
    const stream = new EventSource(
      `/events${projectId ? `?project_id=${encodeURIComponent(projectId)}` : ""}`,
    );
    stream.onopen = () => setConnection("已连接");
    stream.onmessage = () => {
      if (!timer)
        timer = setTimeout(() => {
          timer = undefined;
          load();
        }, 180);
    };
    stream.onerror = () => setConnection("连接中断 · 自动重连");
    return () => {
      stream.close();
      if (timer) clearTimeout(timer);
    };
  }, [load, projectId, caps?.setup_required]);
  useEffect(() => {
    let current = true;
    if (tab === "history" && data)
      rpc("query.history", {
        project_id: projectId || undefined,
        offset: historyPage * 50,
        limit: 50,
      })
        .then((d) => {
          if (current) setHistory(d.events);
        })
        .catch((e) => {
          if (current) setError(String(e));
        });
    return () => {
      current = false;
    };
  }, [tab, historyPage, data, projectId]);
  useEffect(() => {
    if (!toast) return;
    const t = setTimeout(() => setToast(""), 4000);
    return () => clearTimeout(t);
  }, [toast]);
  const detail = data?.detail;
  const f = detail?.feature;
  const projects: RecordData[] = data?.projects || [];
  const features: RecordData[] = data?.features || [];
  const definitions: RecordData[] = data?.definitions || [];
  const project = data?.active_project;
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
    setBusy(true);
    setError("");
    try {
      await rpc("project.open", {
        command_id: crypto.randomUUID(),
        target: id,
        payload: {},
      });
      resetProjectView(id);
      setDialog(undefined);
    } catch (e) {
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
    setDialog({ ...spec, revision: getRevision(spec.target) });
  const command = async (
    method: string,
    target?: string,
    payload: RecordData = {},
    revision?: number,
  ) => {
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
      setToast("操作已保存");
      if (method === "project.create") resetProjectView(result.id);
      else await load();
      return result;
    } catch (e) {
      setError(String(e));
      throw e;
    } finally {
      setBusy(false);
    }
  };
  const begin = (target: string, phase?: string) => {
    show({
      title:
        phase === "planning"
          ? "启动 Feature 规划"
          : phase === "closure"
            ? "启动 Feature 收尾"
            : "启动 Task Attempt",
      method: "attempt.start",
      target,
      payload: { phase, scenario },
      fields: [],
    });
  };
  const createProject = () =>
    show({
      title: "创建 Project",
      method: "project.create",
      fields: [
        { key: "title", label: "项目名称" },
        {
          key: "workspace",
          label: "Workspace 路径 / 仓库描述（仅记录，不执行）",
          optional: true,
        },
      ],
    });
  const createFeature = () =>
    show({
      title: "创建 Feature",
      method: "feature.create",
      payload: { project_id: project?.id },
      fields: [
        { key: "title", label: "Feature 名称" },
        { key: "predecessor", label: "关联前序 Feature ID", optional: true },
      ],
    });
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
  const editBinding = (object: RecordData) =>
    show({
      title: "配置绑定 · 新 Attempt 使用新版本",
      method: "binding.update",
      target: object.id,
      payload: { harness: "sim-harness-v1", sandbox: "sim-sandbox-v1" },
      fields: [
        {
          key: "agent",
          label: "Agent 身份版本",
          type: "select",
          value: object.binding.agent,
          options: definitions
            .filter((d) => d.family === "agent")
            .map((d) => ({ value: d.id, label: `${d.name} v${d.version}` })),
        },
        {
          key: "standard",
          label: "Standard 固定版本",
          type: "select",
          value: object.binding.standard,
          options: definitions
            .filter((d) => d.family === "standard")
            .map((d) => ({ value: d.id, label: `${d.name} v${d.version}` })),
        },
        {
          key: "skills",
          label: "Skills 版本 ID（逗号分隔）",
          value: object.binding.skills.join(","),
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
      <button className={primary ? "primary" : ""} disabled={busy} onClick={fn}>
        {title}
      </button>
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
          {d.evidence.length} 证据 · origin=simulated
        </p>
        {d.candidate && <code>{d.candidate}</code>}
        <div className="actions">
          <button onClick={() => openArtifact(d.artifact_id)}>
            交付报告 <ArrowRight size={13} />
          </button>
          <button
            onClick={() => setView({ title: "冻结交付版本", body: pretty(d) })}
          >
            冻结引用
          </button>
          {act(d.id, "review.request", "Agent 评审", () =>
            command("review.request", d.id, {
              scenario: scenario === "review_reject" ? scenario : "success",
            }).catch(() => {}),
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
            <button
              className="text-button"
              onClick={() => setView({ title: "逐项 Review", body: pretty(r) })}
            >
              逐项标准
            </button>
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
    const generation = projectGeneration.current;
    try {
      const r = await fetch(
        `/artifacts/${id}?project_id=${encodeURIComponent(projectId)}`,
      );
      if (!r.ok) throw new Error("产物读取失败或哈希校验失败");
      const body = await r.text();
      if (generation === projectGeneration.current)
        setView({ title: `Artifact · ${short(id)}`, body });
    } catch (e) {
      if (generation === projectGeneration.current) setError(String(e));
    }
  };
  const nav = [
    ["overview", "工作总览", LayoutDashboard],
    ["workflow", "执行与交付", Workflow],
    ["documents", "文档与上下文", FileText],
    ["definitions", "规范与身份", Library],
    ["history", "历史记录", History],
    ["settings", "设置", Settings],
  ] as const;
  if (caps?.setup_required)
    return (
      <div className="setup">
        <div className="setup-card">
          <div className="brand">
            <Layers3 />
            <strong>DevDesk</strong>
            <span>PROTOTYPE 0.2</span>
          </div>
          <h1>为工程建立一个工作空间</h1>
          <p>选择本地 data 目录。工程状态、文档和执行历史将真实保存在这里。</p>
          <div className="notice">外部能力为模拟模式 · 不修改你的 Git 仓库</div>
          <form
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
            <button className="primary" disabled={busy}>
              初始化 / 打开数据集 <ArrowRight size={16} />
            </button>
          </form>
          {error && (
            <div className="error" role="alert">
              {error}
            </div>
          )}
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
          软件工程工作台 <span>v0.2</span>
        </div>
        <div className="project-selector">
          <span>当前项目</span>
          <select
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
          </select>
          <button aria-label="创建项目" onClick={createProject}>
            <Plus size={16} />
          </button>
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
            <button
              key={id}
              className={tab === id ? "nav active" : "nav"}
              onClick={() => setTab(id)}
            >
              <Icon size={18} />
              {title}
              {id === "workflow" &&
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
            </button>
          ))}
        </nav>
        <div className="sidebar-bottom">
          <div className="sim-dot" />
          <div>
            <strong>Simulation mode</strong>
            <p>真实规则 · 模拟外部执行</p>
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
            <span className="simulation">模拟项目</span>
          </div>
        </header>
        <main>
          {error && (
            <div className="error" role="alert">
              <AlertTriangle size={16} />
              <span>{error}</span>
              <button aria-label="关闭错误" onClick={() => setError("")}>
                <X size={16} />
              </button>
            </div>
          )}
          <div className="page-heading">
            <div>
              <span className="eyebrow">ENGINEERING WORKSPACE</span>
              <h1>{nav.find((n) => n[0] === tab)?.[1]}</h1>
              <p>
                {tab === "overview"
                  ? "从需求到交付，每一次决定都有依据。"
                  : tab === "workflow"
                    ? "运行、验证与评审，由领域规则驱动。"
                    : tab === "documents"
                      ? "可编辑的上下文，不可变的交付版本。"
                      : tab === "definitions"
                        ? "锁定规范与身份，为每次执行建立确定起点。"
                        : tab === "history"
                          ? "持久记录命令、执行边界与关键决定。"
                          : "本地数据与执行能力。"}
              </p>
            </div>
            <div className="actions">
              {tab === "overview" && (
                <>
                  <button onClick={createProject}>
                    <Plus size={16} /> 新项目
                  </button>
                  <button
                    className="primary"
                    disabled={!project}
                    onClick={createFeature}
                  >
                    <Plus size={16} /> 新 Feature
                  </button>
                </>
              )}
            </div>
          </div>
          {!data && <Empty>正在连接本地内核…</Empty>}
          {data && tab === "overview" && (
            <>
              <div className="metrics">
                <Metric
                  title="项目 Features"
                  value={data.total_features}
                  icon={<Layers3 />}
                />
                <Metric
                  title="当前 Task"
                  value={detail?.tasks.length || 0}
                  icon={<Workflow />}
                />
                <Metric
                  title="运行中"
                  value={
                    detail?.runs.filter(
                      (r: RecordData) => r.status === "running",
                    ).length || 0
                  }
                  icon={<Activity />}
                />
                <Metric
                  title="等待评审"
                  value={
                    detail?.deliveries.filter(
                      (d: RecordData) =>
                        !d.stale && !detail.accepted_deliveries.includes(d.id),
                    ).length || 0
                  }
                  icon={<ShieldCheck />}
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
                        <button
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
                        </button>
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
                    所有 Git / Agent /
                    项目测试结果均为模拟来源。工程规则、数据与人类决定真实保存。
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
                    {detail.allowed_actions[f.id]?.attempt_phases?.includes(
                      "planning",
                    ) &&
                      act(
                        f.id,
                        "attempt.start",
                        "启动规划",
                        () => begin(f.id, "planning"),
                        true,
                      )}
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
                    {act(f.id, "binding.update", "配置绑定", () =>
                      editBinding(f),
                    )}
                    <button onClick={() => setTab("workflow")}>
                      进入执行工作台 <ArrowRight size={15} />
                    </button>
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
          {data && ["workflow", "documents"].includes(tab) && (
            <div className="feature-picker">
              <label>
                当前 Feature
                <select
                  aria-label="当前 Feature"
                  value={featureId}
                  onChange={(e) => {
                    setFeatureId(e.target.value);
                    setTaskFilter("");
                    setDetailPages({});
                  }}
                >
                  <option value="">请选择 Feature</option>
                  {features.map((x) => (
                    <option key={x.id} value={x.id}>
                      {x.title}
                    </option>
                  ))}
                </select>
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
          {tab === "workflow" && f && (
            <>
              <div className="panel toolbar">
                <label>
                  模拟场景
                  <select
                    aria-label="模拟场景"
                    value={scenario}
                    onChange={(e) => setScenario(e.target.value)}
                  >
                    {[
                      ["success", "正常执行"],
                      ["technical_failure", "技术失败"],
                      ["assertion_failure", "断言质量失败"],
                      ["missing_fields", "供应商缺字段"],
                      ["hold", "受控等待 / 手动继续"],
                      ["cleanup_failure", "资源清理失败"],
                      ["review_reject", "Agent Review 退回"],
                    ].map(([value, label]) => (
                      <option value={value} key={value}>
                        {label}
                      </option>
                    ))}
                  </select>
                </label>
                <div className="actions">
                  {detail.allowed_actions[f.id]?.attempt_phases?.includes(
                    "planning",
                  ) &&
                    act(f.id, "attempt.start", "规划 Attempt", () =>
                      begin(f.id, "planning"),
                    )}
                  {act(f.id, "task.create", "创建 Task", () =>
                    show({
                      title: "创建独立 Task",
                      method: "task.create",
                      target: f.id,
                      fields: [
                        { key: "title", label: "Task 名称" },
                        {
                          key: "acceptance_keys",
                          label: "实现的验收条目标识（逗号分隔，留空继承全部）",
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
                    command("merge.request", f.id, { scenario }).catch(
                      () => {},
                    ),
                  )}
                  {act(f.id, "binding.update", "Feature 绑定", () =>
                    editBinding(f),
                  )}
                </div>
              </div>
              <section className="panel">
                <div className="panel-heading">
                  <h2>独立 Tasks</h2>
                  <span className="muted">
                    逻辑 worktree · 独立 Context / Agent / Skills
                  </span>
                </div>
                {detail.tasks.length === 0 ? (
                  <Empty>规划交付接受后，创建独立 Task。</Empty>
                ) : (
                  <div className="task-grid">
                    {detail.tasks.map((t: RecordData) => (
                      <article className="task-card" key={t.id}>
                        <div className="row">
                          <h3>{t.title}</h3>
                          <Badge value={t.status} />
                        </div>
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
                            command("merge.request", t.id, { scenario }).catch(
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
                          {act(t.id, "binding.update", "绑定", () =>
                            editBinding(t),
                          )}
                          <button
                            onClick={() =>
                              setTaskFilter(taskFilter === t.id ? "" : t.id)
                            }
                          >
                            {taskFilter === t.id ? "显示全部" : "查看执行"}
                          </button>
                        </div>
                        <button
                          className="text-button"
                          onClick={() =>
                            setView({
                              title: "Task · 绑定与继承",
                              body: pretty(t),
                            })
                          }
                        >
                          查看锁定版本
                        </button>
                      </article>
                    ))}
                  </div>
                )}
                {pager("tasks")}
              </section>
              <section className="panel">
                <div className="panel-heading">
                  <h2>Attempts 与 Run DAG</h2>
                  <button
                    className="text-button"
                    onClick={() => setTaskFilter("")}
                  >
                    Feature 自身 + 全部 Tasks
                  </button>
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
                            <button
                              onClick={() =>
                                setView({
                                  title:
                                    "Attempt 固定输入 / StandardLock / BindingLock",
                                  body: pretty(a),
                                })
                              }
                            >
                              输入与锁
                            </button>
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
                                    <button
                                      title="技术重试"
                                      onClick={() =>
                                        command("run.retry", r.id).catch(
                                          () => {},
                                        )
                                      }
                                    >
                                      <RotateCcw size={13} /> 重试
                                    </button>
                                  )}
                                  {["running", "queued"].includes(r.status) && (
                                    <button
                                      onClick={() =>
                                        command("run.cancel", r.id).catch(
                                          () => {},
                                        )
                                      }
                                    >
                                      取消
                                    </button>
                                  )}
                                  {r.outcome === "hold" &&
                                    ["running", "queued"].includes(
                                      r.status,
                                    ) && (
                                      <button
                                        onClick={() =>
                                          command("run.continue", r.id).catch(
                                            () => {},
                                          )
                                        }
                                      >
                                        继续下一步
                                      </button>
                                    )}
                                  <button
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
                                          blocked: detail.blocked_reasons[r.id],
                                        }),
                                      })
                                    }
                                  >
                                    详情
                                  </button>
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
                            a.phase === "planning"
                              ? "提交 Requirement"
                              : "提交交付",
                            () =>
                              command("delivery.submit", a.id, {
                                kind:
                                  a.phase === "planning"
                                    ? "requirement"
                                    : a.phase,
                              }).catch(() => {}),
                            true,
                          )}
                          {a.phase === "planning" &&
                            act(
                              a.id,
                              "delivery.submit",
                              "提交 Design / Task / Test 计划",
                              () =>
                                command("delivery.submit", a.id, {
                                  kind: "plan",
                                }).catch(() => {}),
                            )}
                        </div>
                      </div>
                    ))
                )}
                {pager("attempts")}
                {pager("runs")}
              </section>
              <section className="panel">
                <div className="panel-heading">
                  <h2>交付与 Review</h2>
                  <span className="muted">冻结版本 · 逐项证据 · 人类例外</span>
                </div>
                {detail.deliveries.length === 0 ? (
                  <Empty>执行结束后提交正式交付。</Empty>
                ) : (
                  detail.deliveries.map(deliveryCard)
                )}
                {pager("deliveries")}
              </section>
              <section className="panel">
                <div className="panel-heading">
                  <h2>Merge 候选</h2>
                  <GitMerge size={18} />
                </div>
                {detail.merges.length === 0 ? (
                  <Empty>接受交付后，申请候选并验证。</Empty>
                ) : (
                  detail.merges.map((m: RecordData) => (
                    <article className="delivery" key={m.id}>
                      <div className="row">
                        <h3>
                          {m.task_id ? "Task → Feature" : "Feature → mainline"}
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
                                      (x: RecordData) => x.id === r.session_id,
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
                          "接纳模拟候选",
                          () => command("merge.apply", m.id).catch(() => {}),
                          true,
                        )}
                        <button
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
                        </button>
                        <span className="muted">
                          {detail.allowed_actions[m.id]?.blocked.join(" · ")}
                        </span>
                      </div>
                    </article>
                  ))
                )}
                {pager("merges")}
              </section>
              <section className="panel" aria-label="外部操作核对">
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
                    <button
                      onClick={() =>
                        setView({
                          title: "外部调用意图与回执",
                          body: pretty(effect),
                        })
                      }
                    >
                      查看回执
                    </button>
                    {allowed(effect.id, "effect.reconcile") && (
                      <button
                        onClick={() =>
                          command("effect.reconcile", effect.id).catch(() => {})
                        }
                      >
                        核对外部结果
                      </button>
                    )}
                    {allowed(effect.id, "effect.retry_cleanup") && (
                      <button
                        onClick={() =>
                          command("effect.retry_cleanup", effect.id).catch(
                            () => {},
                          )
                        }
                      >
                        重试资源清理
                      </button>
                    )}
                  </div>
                ))}
                {pager("effects")}
              </section>
            </>
          )}
          {tab === "documents" && f && (
            <>
              <section className="panel">
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
                          options: ["requirement", "design", "test", "doc"].map(
                            (value) => ({ value, label: labels[value] }),
                          ),
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
                          label: "Markdown 内容",
                          type: "textarea",
                        },
                      ],
                    }),
                  )}
                </div>
                {detail.documents.length === 0 ? (
                  <Empty>先输入需求，或启动规划生成可编辑建议。</Empty>
                ) : (
                  detail.documents.map((d: RecordData) => (
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
                      <button
                        onClick={() =>
                          setView({
                            title: `${d.title} · v${d.version}`,
                            body: d.body + "\n\n---\n" + pretty(d),
                          })
                        }
                      >
                        阅读
                      </button>
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
                              label: "需求验收条目（每行：标识 | 可验证条件）",
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
                              label: "Markdown 内容",
                              type: "textarea",
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
              <section className="panel">
                <div className="panel-heading">
                  <h2>动态 Context 与不可变 Snapshot</h2>
                  {act(f.id, "snapshot.save", "保存 Feature Snapshot", () =>
                    command("snapshot.save", f.id).catch(() => {}),
                  )}
                </div>
                {detail.contexts.map((c: RecordData) => (
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
                    <button
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
                    </button>
                    <button
                      onClick={() =>
                        command("snapshot.save", c.owner_id).catch(() => {})
                      }
                    >
                      保存 Snapshot
                    </button>
                  </div>
                ))}
                {detail.snapshots.map((s: RecordData) => (
                  <div className="document-row" key={s.id}>
                    <Copy size={18} />
                    <div>
                      <strong>
                        {s.kind} Snapshot · {short(s.id)}
                      </strong>
                      <p>
                        {s.task_id ? "Task" : "Feature"} · {s.documents.length}{" "}
                        个固定文档 · inherited {short(s.inherited_from)}
                      </p>
                    </div>
                    <button
                      onClick={() =>
                        setView({
                          title: "不可变 Snapshot / 重建引用",
                          body: pretty(s),
                        })
                      }
                    >
                      查看
                    </button>
                  </div>
                ))}
                {pager("tasks")}
                {pager("snapshots")}
                {detail.snapshots.length > 1 && (
                  <div className="snapshot-compare">
                    <label>
                      快照 A
                      <select
                        value={snapshotA}
                        onChange={(e) => setSnapshotA(e.target.value)}
                      >
                        <option value="">请选择</option>
                        {detail.snapshots.map((s: RecordData) => (
                          <option value={s.id} key={s.id}>
                            {s.kind} · {short(s.id)}
                          </option>
                        ))}
                      </select>
                    </label>
                    <label>
                      快照 B
                      <select
                        value={snapshotB}
                        onChange={(e) => setSnapshotB(e.target.value)}
                      >
                        <option value="">请选择</option>
                        {detail.snapshots.map((s: RecordData) => (
                          <option value={s.id} key={s.id}>
                            {s.kind} · {short(s.id)}
                          </option>
                        ))}
                      </select>
                    </label>
                    <button
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
                    </button>
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
                    <button onClick={() => openArtifact(a.id)}>打开</button>
                  </div>
                ))}
                {pager("artifacts")}
              </section>
            </>
          )}
          {["workflow", "documents"].includes(tab) && !f && (
            <Empty>选择一个 Feature 后查看工作内容。</Empty>
          )}
          {data && tab === "definitions" && (
            <>
              <div className="notice">
                定义修订不可变。克隆生成新版本，再显式绑定到 Project、Feature 或
                Task；旧 Attempt 的锁保持原版本。
              </div>
              {["agent", "skill", "spec", "standard"].map((family) => (
                <section className="panel" key={family}>
                  <div className="panel-heading">
                    <h2>
                      {
                        (
                          {
                            agent: "Agent 身份",
                            skill: "Skills",
                            spec: "Specs",
                            standard: "Standards",
                          } as RecordData
                        )[family]
                      }
                    </h2>
                  </div>
                  {definitions
                    .filter((d) => d.family === family)
                    .map((d) => (
                      <div className="document-row" key={d.id}>
                        <Library size={18} />
                        <div>
                          <strong>
                            {d.name}{" "}
                            <span className="version">v{d.version}</span>
                          </strong>
                          <p>{d.body}</p>
                          <code>
                            {d.id}
                            {d.slots.length > 0 &&
                              ` · slots: ${d.slots.map((s: RecordData) => `${s.purpose}:${s.spec}`).join(", ")}`}
                          </code>
                        </div>
                        <button
                          onClick={() =>
                            setView({
                              title: "不可变定义版本",
                              body: pretty(d),
                            })
                          }
                        >
                          版本详情
                        </button>
                        <button
                          onClick={() =>
                            show({
                              title: "克隆为新定义版本",
                              method: "definition.revise",
                              target: d.id,
                              fields: [
                                { key: "name", label: "名称", value: d.name },
                                {
                                  key: "body",
                                  label: "定义内容",
                                  type: "textarea",
                                  value: d.body,
                                },
                                ...(family === "spec"
                                  ? ([
                                      {
                                        key: "rules",
                                        label:
                                          "内置规则（traceability / failure_paths / sections，逗号分隔）",
                                        value: d.rules.join(","),
                                      },
                                    ] as Field[])
                                  : []),
                                ...(family === "standard"
                                  ? ([
                                      {
                                        key: "slots",
                                        label:
                                          "用途 slots JSON（purpose / spec）",
                                        type: "textarea",
                                        value: pretty(d.slots),
                                      },
                                    ] as Field[])
                                  : []),
                              ],
                            })
                          }
                        >
                          克隆修订
                        </button>
                      </div>
                    ))}
                </section>
              ))}
              <section className="panel">
                <div className="panel-heading">
                  <h2>Harness / Sandbox 能力</h2>
                </div>
                <pre>
                  {pretty({
                    harnesses: data?.harnesses,
                    sandboxes: data?.sandboxes,
                  })}
                </pre>
                <p className="muted">
                  Agent 是身份，Harness 是运行时，Sandbox
                  是执行环境。未来真实适配器替换能力实现。
                </p>
              </section>
            </>
          )}
          {data && tab === "history" && (
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
                setPage={setHistoryPage}
                hasMore={history.length === 50}
              />
            </section>
          )}
          {data && tab === "settings" && (
            <>
              <section className="panel">
                <div className="panel-heading">
                  <h2>数据与运行</h2>
                  <Badge value="simulation" />
                </div>
                <div className="setting-row">
                  <div>
                    <h3>本地 data 目录</h3>
                    <code>{data.data_root}</code>
                    <p className="muted">
                      全局配置与各项目分别保存；打开项目即可切换。更改 data
                      路径需退出后以 --data 新路径重启。
                    </p>
                  </div>
                  <button
                    onClick={() =>
                      setView({
                        title: "打开其他数据集",
                        body: "1. 退出当前内核。\n2. 使用 devdesk-host --data /path/to/dataset 启动，或不传 --data 后在初始设置中选择路径。\n\n不自动搬迁原数据。",
                      })
                    }
                  >
                    更改路径说明
                  </button>
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
                <div className="setting-row">
                  <div>
                    <h3>全局并发额度</h3>
                    <p>{data.global_concurrency} 个 Run · 单项目默认 2 个</p>
                  </div>
                  <button
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
                  </button>
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
                      <button onClick={() => editBinding(project)}>
                        默认绑定
                      </button>
                      <button
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
                      </button>
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
                  <button
                    className="danger"
                    onClick={() =>
                      show({
                        title: "退出本地内核",
                        method: "shutdown",
                        fields: [],
                      })
                    }
                  >
                    退出
                  </button>
                </div>
              </section>
              <section className="panel">
                <div className="panel-heading">
                  <h2>数据生命周期</h2>
                  <span className="badge">仅交互预览</span>
                </div>
                {["Archive", "data 迁移"].map((title) => (
                  <div className="setting-row" key={title}>
                    <div>
                      <h3>{title}</h3>
                      <p className="muted">
                        本轮预览操作范围，不执行导出、删除、恢复或搬迁。
                      </p>
                    </div>
                    <button
                      onClick={() =>
                        setView({
                          title: `${title} · 预览结束`,
                          body: `当前目录：${data.data_root}\n项目数量：${projects.length}\n\n${title === "Archive" ? "未来将固定归档引用、导出清单与校验。" : "未来将校验目标、复制、核对并切换数据集。"}\n\n预览结束。未写入业务结果，未搬动或删除文件。`,
                        })
                      }
                    >
                      预览
                    </button>
                  </div>
                ))}
              </section>
              <div className="notice">
                Linux x86_64 / macOS Apple Silicon：已完成实机验证。真实 Git /
                Codex / 项目测试执行器：延期。
              </div>
            </>
          )}
          <footer>
            DevDesk 0.2{" "}
            <span>本地工程内核 · 持久化历史 · origin=simulated</span>
          </footer>
        </main>
      </div>
      {toast && (
        <div className="toast" role="status">
          <Check size={17} />
          {toast}
        </div>
      )}
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
                setToast("内核已退出");
              } else if (dialog.method === "settings.update") {
                result = await rpc(dialog.method, payload);
                await load();
              } else {
                result = await command(
                  dialog.method,
                  dialog.target,
                  payload,
                  dialog.revision,
                );
              }
              if (dialog.method === "feature.create" && result?.id) {
                setFeatureId(result.id);
                setTab("overview");
              }
              if (dialog.method === "project.create" && result?.id) {
                setProjectId(result.id);
                setFeatureId("");
              }
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
}: {
  title: string;
  value: number;
  icon: React.ReactNode;
}) {
  return (
    <article className="metric">
      <div>
        <span>{title}</span>
        <strong>{value.toString().padStart(2, "0")}</strong>
      </div>
      <div className="metric-icon">{icon}</div>
    </article>
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
      <button disabled={page === 0} onClick={() => setPage(page - 1)}>
        上一页
      </button>
      <span>第 {page + 1} 页</span>
      <button disabled={!hasMore} onClick={() => setPage(page + 1)}>
        下一页
      </button>
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
  const [localError, setLocalError] = useState("");
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
    setLocalError("");
    try {
      const payload: RecordData = { ...spec.payload };
      for (const field of spec.fields) {
        const value = values[field.key];
        if (field.optional && !value.trim()) continue;
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
      <form
        className="modal"
        ref={formRef}
        role="dialog"
        aria-modal="true"
        aria-label={spec.title}
        onSubmit={send}
      >
        <div className="modal-heading">
          <h2>{spec.title}</h2>
          <button
            type="button"
            aria-label="关闭"
            disabled={busy}
            onClick={close}
          >
            <X size={18} />
          </button>
        </div>
        {localError && (
          <div className="error" role="alert">
            {localError}
          </div>
        )}
        {spec.fields.length === 0 && (
          <p className="muted">确认执行此领域操作。结果将保存到工程历史。</p>
        )}
        {spec.fields.map((f, i) => (
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
              <select
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
              </select>
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
          </label>
        ))}
        <div className="modal-actions">
          <button type="button" disabled={busy} onClick={close}>
            取消
          </button>
          <button className="primary" disabled={busy}>
            {busy ? "保存中…" : "确认"}
          </button>
        </div>
      </form>
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
          <button onClick={() => perform("run.retry")}>重试</button>
        )}
        {["running", "queued"].includes(run.status) && (
          <button onClick={() => perform("run.cancel")}>取消</button>
        )}
        {run.outcome === "hold" &&
          ["running", "queued"].includes(run.status) && (
            <button onClick={() => perform("run.continue")}>继续下一步</button>
          )}
        <button onClick={inspect}>详情</button>
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
  close,
}: {
  title: string;
  body: string;
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
          <button aria-label="关闭" onClick={close}>
            <X size={18} />
          </button>
        </div>
        <pre>{body}</pre>
      </section>
    </div>
  );
}
createRoot(document.getElementById("root")!).render(
  <React.StrictMode>
    <App />
  </React.StrictMode>,
);

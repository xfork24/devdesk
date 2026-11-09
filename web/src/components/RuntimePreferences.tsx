import {
  ActionButton,
  ActionForm,
  ActionSelect,
  useActionFeedback,
  currentActionTarget,
  publishActionFeedback,
  type ActionTarget,
} from "./ActionFeedback";
import { FormEvent, useCallback, useEffect, useRef, useState } from "react";

type Row = Record<string, any>;
type Props = {
  project: Row;
  installations: Row[];
  request: (method: string, params?: Row) => Promise<any>;
  refresh: () => Promise<void>;
};
type Options = {
  default_harness: string;
  host_os: string;
  host_arch: string;
  installations: Row[];
  model_providers: Row[];
  recipes: Row[];
};

export function RuntimePreferences({
  project,
  installations,
  request,
  refresh,
}: Props) {
  const { message, error, setMessage, setError, beginFeedback, endFeedback } =
    useActionFeedback();
  const [scope, setScope] = useState("project");
  const [harness, setHarness] = useState("system");
  const [installation, setInstallation] = useState("");
  const [modelProvider, setModelProvider] = useState("deepseek");
  const [model, setModel] = useState("");
  const [endpoint, setEndpoint] = useState("");
  const [context, setContext] = useState<number | undefined>();
  const [output, setOutput] = useState<number | undefined>();
  const [options, setOptions] = useState<Options>();
  const [optionsError, setOptionsError] = useState("");
  const [models, setModels] = useState<Row[]>([]);
  const [source, setSource] = useState("");
  const catalogTarget = useRef<ActionTarget | null>(null);
  const [catalogError, setCatalogError] = useState("");
  const [catalogBusy, setCatalogBusy] = useState(false);
  const [catalogRevision, setCatalogRevision] = useState(0);
  const [busy, setBusy] = useState(false);
  const preferencesJson = JSON.stringify(project.runtime_preferences || null);
  const installationIds = installations.map((r) => r.id).join(",");
  const loadOptions = useCallback(async () => {
    const value = await request("query.runtime_options");
    setOptions(value);
    setOptionsError("");
    return value as Options;
  }, [request]);
  useEffect(() => {
    let active = true;
    const load = () => {
      request("query.runtime_options")
        .then((value) => {
          if (active) {
            setOptions(value);
            setOptionsError("");
          }
        })
        .catch((e) => {
          if (active) setOptionsError(String(e));
        });
    };
    load();
    const changed = () => {
      load();
      setCatalogRevision((v) => v + 1);
    };
    window.addEventListener("devdesk:runtime-options-changed", changed);
    return () => {
      active = false;
      window.removeEventListener("devdesk:runtime-options-changed", changed);
    };
  }, [request, installationIds]);
  useEffect(() => {
    const prefs = JSON.parse(preferencesJson);
    const p =
      scope === "project"
        ? prefs?.default
        : prefs?.roles?.find((r: Row) => r.role === scope)?.preference;
    setHarness(!p || p.use_system_default ? "system" : p.provider);
    setInstallation(p?.installation_id || "");
    setModel(p?.model || "");
    setModelProvider(p?.model_provider || "deepseek");
    setEndpoint(p?.endpoint || "");
    setContext(p?.context_window);
    setOutput(p?.max_output_tokens);
  }, [project.id, preferencesJson, scope]);
  const provider = harness === "system" ? options?.default_harness : harness;
  const native = options?.installations.find(
    (r) =>
      r.key.provider === "codex" &&
      r.key.os === options.host_os &&
      r.key.arch === options.host_arch &&
      r.artifact.kind === "executable",
  );
  const selectedProvider = options?.model_providers.find(
    (p) => p.id === modelProvider,
  );
  const keyReady =
    modelProvider === "ollama" ||
    (selectedProvider?.configured && selectedProvider.available);
  useEffect(() => {
    let active = true;
    const target = catalogTarget.current;
    catalogTarget.current = null;
    if (target) publishActionFeedback(target, "正在读取模型目录…");
    setModels([]);
    setCatalogError("");
    setSource("");
    setCatalogBusy(false);
    if (
      !provider ||
      (provider === "codex" && !native) ||
      (provider === "dsh" && !keyReady)
    )
      return;
    setCatalogBusy(true);
    request(
      provider === "codex" ? "codex.auth.models" : "models.list",
      provider === "codex"
        ? { installation_id: native!.id }
        : {
            model_provider: modelProvider,
            ...(modelProvider === "ollama" && endpoint ? { endpoint } : {}),
          },
    )
      .then((value) => {
        if (!active) return;
        const list = (value.models || []).map((m: Row) => ({
          ...m,
          id: m.model || m.id,
        }));
        setModels(list);
        setSource(
          provider === "codex"
            ? "Codex 账号模型目录"
            : value.source === "provider_endpoint"
              ? "服务商账号模型目录"
              : value.source === "host_endpoint"
                ? "Ollama 服务模型目录"
                : "当前 DSH 版本支持的模型目录（账号调用权限需单独验证）",
        );
        if (target)
          publishActionFeedback(
            target,
            list.length
              ? `已读取 ${list.length} 个可选模型。`
              : "目录没有返回可选模型，请检查账号或服务配置后刷新。",
            !list.length,
          );
        else if (!list.length)
          setCatalogError("目录没有返回可选模型，请检查账号或服务配置后刷新。");
      })
      .catch((e) => {
        if (active && !publishActionFeedback(target, String(e), true))
          setCatalogError(String(e));
      })
      .finally(() => {
        if (active) setCatalogBusy(false);
      });
    return () => {
      active = false;
      if (target) publishActionFeedback(target, "");
    };
  }, [
    request,
    provider,
    native?.id,
    modelProvider,
    keyReady,
    endpoint,
    catalogRevision,
  ]);
  const selectedModel = models.find((m) => m.id === model);
  const runtime = options?.installations.find(
    (r) =>
      r.id === installation &&
      r.key.provider === provider &&
      r.execution_supported,
  );
  const recipe = options?.recipes.find((r) => r.provider === provider);
  const canSave =
    !!runtime &&
    !!selectedModel &&
    (provider === "codex" || keyReady) &&
    (provider !== "dsh" ||
      modelProvider !== "ollama" ||
      (!!(selectedModel.context_window || context) &&
        !!(selectedModel.max_output_tokens || output)));
  async function save(e: FormEvent) {
    e.preventDefault();
    beginFeedback();
    setBusy(true);
    setError("");
    setMessage("");
    try {
      if (!canSave)
        throw new Error(
          "请选择可执行的共享安装和目录中的模型；本地模型还需明确容量。",
        );
      const preference = {
        provider,
        use_system_default: harness === "system",
        installation_id: installation,
        model,
        model_provider: provider === "codex" ? "openai" : modelProvider,
        ...(provider === "dsh"
          ? {
              ...(modelProvider === "ollama" && endpoint ? { endpoint } : {}),
              context_window: selectedModel.context_window || context || null,
              max_output_tokens:
                selectedModel.max_output_tokens || output || null,
            }
          : {}),
      };
      const old = project.runtime_preferences || { default: null, roles: [] };
      const next =
        scope === "project"
          ? { ...old, default: preference }
          : {
              ...old,
              roles: [
                ...old.roles.filter((r: Row) => r.role !== scope),
                { role: scope, preference },
              ],
            };
      await request("project.update", {
        project_id: project.id,
        command_id: crypto.randomUUID(),
        target: project.id,
        expected_revision: project.revision,
        payload: { runtime_preferences: next },
      });
      await refresh();
      setMessage("偏好已保存；已有会话继续使用冻结配置。");
    } catch (e) {
      setError(String(e));
    } finally {
      endFeedback();
      setBusy(false);
    }
  }
  return (
    <section className="setting-row">
      <div>
        <h3>项目与角色运行时偏好</h3>
        <p className="muted">
          {project.title} · 用于未显式指定完整 Agent 的新会话。完整 Agent 的
          Model、Harness 和既有会话配置以其冻结版本为准。
        </p>
        {optionsError && <p role="alert">{optionsError}</p>}
        <ActionForm onSubmit={save}>
          <label>
            配置范围
            <ActionSelect
              aria-label="运行时偏好范围"
              value={scope}
              onChange={(e) => setScope(e.target.value)}
              disabled={busy}
            >
              <option value="project">项目默认</option>
              {Object.entries({
                analyst: "需求分析师",
                architect: "架构师 / 项目规划",
                developer: "开发",
                tester: "测试",
                reviewer: "评审",
              }).map(([key, label]) => (
                <option key={key} value={key}>
                  {label}
                </option>
              ))}
            </ActionSelect>
          </label>
          <label>
            Harness
            <ActionSelect
              aria-label="项目角色 Harness"
              value={harness}
              disabled={busy}
              onChange={(e) => {
                setHarness(e.target.value);
                setInstallation("");
                setModel("");
                setModelProvider(
                  options?.model_providers.find(
                    (p) =>
                      p.auth_kind === "none" || (p.configured && p.available),
                  )?.id || "deepseek",
                );
                setEndpoint("");
                setContext(undefined);
                setOutput(undefined);
              }}
            >
              <option value="system">
                系统默认Harness
                {options
                  ? `（${options.default_harness === "codex" ? "Codex" : "DeepSeek Harness"}）`
                  : ""}
              </option>
              <option value="codex">Codex</option>
              <option value="dsh">DeepSeek Harness</option>
            </ActionSelect>
          </label>
          <label>
            共享安装
            <ActionSelect
              aria-label="项目角色安装"
              disabled={busy || !options}
              value={installation}
              onChange={(e) => setInstallation(e.target.value)}
              required
            >
              <option value="" disabled>
                请选择明确的 Docker 运行时安装
              </option>
              {options?.installations
                .filter((r) => r.key.provider === provider)
                .map((r) => (
                  <option
                    key={r.id}
                    value={r.id}
                    disabled={!r.execution_supported}
                  >
                    {r.key.version} · {r.key.os}/{r.key.arch} ·{" "}
                    {r.artifact.kind === "executable"
                      ? "本机登录 / 模型目录组件"
                      : "Docker 项目执行"}{" "}
                    · {r.id.slice(0, 8)}
                    {r.execution_supported ? "" : "（不可用于项目执行）"}
                  </option>
                ))}
            </ActionSelect>
          </label>
          {options?.installations
            .filter(
              (r) => r.key.provider === provider && !r.execution_supported,
            )
            .map((r) => (
              <p className="muted" key={r.id}>
                {r.key.os}/{r.key.arch} · {r.key.version}：{r.usage_reason}
              </p>
            ))}
          {recipe && (
            <div className="actions">
              <ActionButton
                type="button"
                disabled={busy}
                onClick={async () => {
                  beginFeedback();
                  setBusy(true);
                  setError("");
                  setMessage("正在准备所选 Docker 运行时…");
                  try {
                    const prepared = await request("runtime.prepare", {
                      provider,
                    });
                    await refresh();
                    await loadOptions();
                    setInstallation(prepared.id);
                    setMessage("运行时已准备，可用于新会话。");
                  } catch (e) {
                    setError(String(e));
                  } finally {
                    endFeedback();
                    setBusy(false);
                  }
                }}
              >
                准备 {provider === "codex" ? "Codex" : "DSH"} {recipe.version} ·
                Linux/{recipe.arch} Docker 运行时
              </ActionButton>
            </div>
          )}
          {provider === "dsh" && (
            <label>
              模型服务商
              <ActionSelect
                aria-label="偏好模型服务商"
                value={
                  options?.model_providers.some((p) => p.id === modelProvider)
                    ? modelProvider
                    : ""
                }
                disabled={busy}
                onChange={(e) => {
                  setModelProvider(e.target.value);
                  setModel("");
                  setEndpoint("");
                  setContext(undefined);
                  setOutput(undefined);
                }}
              >
                <option value="" disabled>
                  请选择明确的模型服务商
                </option>
                {options?.model_providers.map((p) => (
                  <option
                    key={p.id}
                    value={p.id}
                    disabled={
                      p.auth_kind !== "none" && (!p.configured || !p.available)
                    }
                  >
                    {p.label}
                    {p.auth_kind !== "none" && (!p.configured || !p.available)
                      ? "（请先配置可访问的 API key）"
                      : ""}
                  </option>
                ))}
              </ActionSelect>
            </label>
          )}
          {provider === "dsh" && !keyReady && (
            <p className="muted">
              请在上方配置所选服务商的 API key，保存后会自动刷新模型选项。
            </p>
          )}
          {provider === "dsh" && modelProvider === "ollama" && (
            <label>
              Ollama 服务地址
              <input
                type="url"
                value={endpoint}
                placeholder="http://127.0.0.1:11434/v1"
                disabled={busy}
                onChange={(e) => setEndpoint(e.target.value)}
              />
            </label>
          )}
          {provider === "codex" && options && !native && (
            <div className="actions">
              <ActionButton
                type="button"
                disabled={busy}
                onClick={async () => {
                  beginFeedback();
                  setBusy(true);
                  setError("");
                  try {
                    await request("codex.auth.status");
                    await refresh();
                    await loadOptions();
                    setCatalogRevision((v) => v + 1);
                    setMessage(
                      "本机登录组件已准备；未登录时请使用上方网页登录。",
                    );
                  } catch (e) {
                    setError(String(e));
                  } finally {
                    endFeedback();
                    setBusy(false);
                  }
                }}
              >
                准备本机 Codex 登录与模型目录组件
              </ActionButton>
            </div>
          )}
          <label>
            模型 ID
            <ActionSelect
              aria-label="项目角色模型"
              value={model}
              required
              disabled={busy || catalogBusy || !models.length}
              onChange={(e) => {
                setModel(e.target.value);
                const m = models.find((m) => m.id === e.target.value);
                setContext(m?.context_window);
                setOutput(m?.max_output_tokens);
              }}
            >
              <option value="" disabled>
                {catalogBusy ? "正在读取模型目录…" : "请选择模型"}
              </option>
              {model && !selectedModel && (
                <option value={model} disabled>
                  {model}（当前目录不可用，请重新选择）
                </option>
              )}
              {models.map((m) => (
                <option key={m.id} value={m.id}>
                  {m.id}
                  {m.displayName && m.displayName !== m.id
                    ? ` · ${m.displayName}`
                    : ""}
                </option>
              ))}
            </ActionSelect>
          </label>
          {source && <p className="muted">{source}。读取目录不会发起推理。</p>}
          {catalogError && <p role="alert">{catalogError}</p>}
          <div className="actions">
            <ActionButton
              type="button"
              disabled={
                busy ||
                catalogBusy ||
                !options ||
                (provider === "codex" ? !native : !keyReady)
              }
              onClick={() => {
                catalogTarget.current = currentActionTarget();
                setCatalogRevision((v) => v + 1);
              }}
            >
              刷新模型目录
            </ActionButton>
          </div>
          {provider === "dsh" && modelProvider === "ollama" && (
            <>
              <label>
                上下文容量
                <input
                  type="number"
                  min={1}
                  max={2000000}
                  value={context || ""}
                  onChange={(e) =>
                    setContext(Number(e.target.value) || undefined)
                  }
                  disabled={busy || !!selectedModel?.context_window}
                  required
                />
              </label>
              <label>
                最大输出容量
                <input
                  type="number"
                  min={1}
                  max={1000000}
                  value={output || ""}
                  onChange={(e) =>
                    setOutput(Number(e.target.value) || undefined)
                  }
                  disabled={busy || !!selectedModel?.max_output_tokens}
                  required
                />
              </label>
            </>
          )}
          <div className="actions">
            <ActionButton disabled={busy || !canSave}>保存偏好</ActionButton>
          </div>
        </ActionForm>
        {error && <p role="alert">{error}</p>}
        {message && <p role="status">{message}</p>}
      </div>
    </section>
  );
}

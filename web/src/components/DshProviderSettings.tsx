import {
  ActionButton,
  ActionForm,
  ActionSelect,
  useActionFeedback,
} from "./ActionFeedback";
import { useEffect, useState, type FormEvent } from "react";
import type { Request } from "./configuration-ui";

export type ModelProvider = {
  id: string;
  /** Source reference (`identity:revision`) used by Harness provider lists. */
  reference: string;
  label: string;
  harness: string;
  endpoint?: string;
  auth_kind: string;
  models: { id: string; context_window?: number; max_output_tokens?: number }[];
};
type Status = {
  configured: boolean;
  available: boolean;
  storage: string;
  auth_kind: string;
};

/** Provider keys stay in component memory until submitted to the OS vault. */
export function DshProviderSettings({ request }: { request: Request }) {
  const {
    message,
    setMessage,
    capture: captureFeedback,
    beginFeedback,
    endFeedback,
  } = useActionFeedback();

  const [providers, setProviders] = useState<ModelProvider[]>([]);
  const [provider, setProvider] = useState("minimax-cn");
  const [status, setStatus] = useState<Status>();
  const [secret, setSecret] = useState("");
  const [busy, setBusy] = useState(false);

  useEffect(() => {
    let active = true;
    request("query.model_providers")
      .then((value) => {
        if (active)
          setProviders(
            value.providers.filter((p: ModelProvider) => p.harness === "dsh"),
          );
      })
      .catch(() => {
        if (active) setMessage("无法读取模型服务商目录");
      });
    return () => {
      active = false;
    };
  }, [request]);
  useEffect(() => {
    let active = true;
    setStatus(undefined);
    request("credentials.status", { model_provider: provider })
      .then((value) => {
        if (active) setStatus(value);
      })
      .catch(() => {
        if (active) setMessage("无法读取所选服务商凭据状态");
      });
    return () => {
      active = false;
    };
  }, [request, provider]);
  async function save(event: FormEvent) {
    captureFeedback();

    event.preventDefault();
    (beginFeedback(), setBusy(true));
    setMessage("");
    try {
      setStatus(
        await request("credentials.configure", {
          provider: "dsh",
          model_provider: provider,
          api_key: secret,
        }),
      );
      window.dispatchEvent(new Event("devdesk:runtime-options-changed"));
      setMessage("已保存到系统凭据库");
    } catch (error) {
      setMessage(String(error));
    } finally {
      setSecret("");
      (endFeedback(), setBusy(false));
    }
  }
  async function check(method: string) {
    captureFeedback();

    (beginFeedback(), setBusy(true));
    setMessage("");
    try {
      await request(method, { model_provider: provider });
      setMessage("已验证凭据可读取；尚未验证模型调用");
    } catch (error) {
      setMessage(String(error));
    } finally {
      (endFeedback(), setBusy(false));
    }
  }
  const selected = providers.find((p) => p.id === provider);
  return (
    <div className="setting-row">
      <div>
        <h3>
          {provider === "deepseek"
            ? "DeepSeek 官方 API key"
            : selected?.label || "DSH 模型服务商"}
        </h3>
        <label>
          DSH 模型服务商
          <ActionSelect
            aria-label="DSH 模型服务商"
            value={provider}
            disabled={busy}
            onChange={(event) => {
              captureFeedback();

              setProvider(event.target.value);
              setSecret("");
              setStatus(undefined);
              setMessage("");
            }}
          >
            {providers.length ? (
              providers.map((p) => (
                <option key={p.id} value={p.id}>
                  {p.label}
                </option>
              ))
            ) : (
              <option value="minimax-cn">MiniMax 订阅 国内</option>
            )}
          </ActionSelect>
        </label>
        {provider === "ollama" ? (
          <p className="muted">
            本地服务无需 API key。在 Model
            中配置服务地址、模型与容量。订阅云模型使用本机 Ollama
            的登录；容器连通与推理结果分别验证。
          </p>
        ) : (
          <>
            <p>
              {status
                ? status.configured
                  ? "已配置"
                  : "尚未配置"
                : "正在读取配置状态…"}
            </p>
            <p className="muted">
              密钥保存在本机系统凭据库。
              {provider === "deepseek" ? "" : "请使用所选国内订阅套餐的 key。"}
            </p>
            {status?.available === false && (
              <p role="alert">
                {status.storage === "macos_keychain"
                  ? "凭据助手不可用或版本过旧；新服务商需要显式升级同证书签名的共享助手。"
                  : "系统凭据库不可用；Linux 请安装 libsecret-tools 并启动桌面 Secret Service。"}
              </p>
            )}
            {status?.configured && (
              <div className="actions">
                <ActionButton
                  disabled={busy}
                  onClick={() => {
                    captureFeedback();
                    return void check("credentials.check");
                  }}
                >
                  检查凭据访问
                </ActionButton>
                {status.storage === "macos_keychain" && (
                  <ActionButton
                    disabled={busy}
                    onClick={() => {
                      captureFeedback();
                      return void check("credentials.authorize");
                    }}
                  >
                    授权已保存的钥匙串凭据
                  </ActionButton>
                )}
              </div>
            )}
            <ActionForm onSubmit={save}>
              <label
                htmlFor={
                  provider === "deepseek"
                    ? "deepseek-api-key"
                    : "dsh-provider-api-key"
                }
              >
                配置或更换 API key
              </label>
              <input
                id={
                  provider === "deepseek"
                    ? "deepseek-api-key"
                    : "dsh-provider-api-key"
                }
                type="password"
                autoComplete="off"
                spellCheck={false}
                value={secret}
                onChange={(event) => {
                  captureFeedback();
                  return setSecret(event.target.value);
                }}
                disabled={busy}
                minLength={8}
                maxLength={4096}
                required
              />
              <ActionButton type="submit" disabled={busy || secret.length < 8}>
                保存密钥
              </ActionButton>
            </ActionForm>
          </>
        )}
        {selected?.endpoint && (
          <p className="muted">
            默认服务地址：<code>{selected.endpoint}</code>
          </p>
        )}
        <p role="status" aria-live="polite">
          {message}
        </p>
      </div>
    </div>
  );
}

/** Model catalogs are read automatically; choosing a candidate only updates the draft. */
export function ModelFields({
  request,
  data,
  patch,
  providers,
}: {
  request: Request;
  data: {
    provider: string;
    model?: string;
    endpoint?: string;
    context_window?: number;
    max_output_tokens?: number;
  };
  patch: (value: Record<string, unknown>) => void;
  providers: ModelProvider[];
}) {
  type Candidate = ModelProvider["models"][number] & { displayName?: string };
  const [candidates, setCandidates] = useState<Candidate[]>([]);
  const [loading, setLoading] = useState(true);
  const [error, setError] = useState("");
  const [source, setSource] = useState("");
  const [revision, setRevision] = useState(0);
  const codex = data.provider === "openai";
  const ollama = data.provider === "ollama";
  const selected = providers.find((provider) => provider.id === data.provider);

  useEffect(() => {
    const changed = () => setRevision((value) => value + 1);
    window.addEventListener("devdesk:runtime-options-changed", changed);
    return () =>
      window.removeEventListener("devdesk:runtime-options-changed", changed);
  }, []);

  useEffect(() => {
    let active = true;
    setLoading(true);
    setCandidates([]);
    setError("");
    setSource("");
    const timer = window.setTimeout(
      () => {
        request(
          codex ? "codex.auth.models" : "models.list",
          codex
            ? {}
            : {
                model_provider: data.provider,
                ...(ollama && data.endpoint ? { endpoint: data.endpoint } : {}),
              },
        )
          .then((result) => {
            if (!active) return;
            const models = (result.models || []).map(
              (model: Record<string, any>) => ({
                ...model,
                id: model.model || model.id,
              }),
            );
            setCandidates(models);
            if (!models.length)
              setError("模型目录为空，请检查后端服务或账号配置后刷新。");
            setSource(
              codex
                ? "Codex 账号模型目录"
                : result.source === "provider_endpoint"
                  ? "服务商账号模型目录"
                  : result.source === "host_endpoint"
                    ? "Ollama 服务模型目录"
                    : "当前 DSH 版本支持的模型目录",
            );
          })
          .catch((error) => {
            if (active) setError(String(error));
          })
          .finally(() => {
            if (active) setLoading(false);
          });
      },
      ollama ? 300 : 0,
    );
    return () => {
      active = false;
      window.clearTimeout(timer);
    };
  }, [request, data.provider, data.endpoint, codex, ollama, revision]);

  return (
    <div className="full-width model-fields">
      {!codex && (
        <p className="muted">
          {selected?.label} ·{" "}
          {ollama ? "通过本机 Ollama 服务连接" : selected?.endpoint}
        </p>
      )}
      {ollama && (
        <label>
          Ollama 服务地址
          <input
            type="url"
            value={data.endpoint || ""}
            placeholder="http://127.0.0.1:11434/v1"
            onChange={(event) =>
              patch({ endpoint: event.target.value || undefined })
            }
          />
        </label>
      )}
      <label>
        模型标识
        <ActionSelect
          aria-label="模型标识"
          value={data.model || ""}
          required
          disabled={loading}
          onChange={(event) => {
            const candidate = candidates.find(
              (model) => model.id === event.target.value,
            );
            if (candidate)
              patch({
                model: candidate.id,
                ...(codex
                  ? {}
                  : {
                      context_window: candidate.context_window,
                      max_output_tokens: candidate.max_output_tokens,
                    }),
              });
          }}
        >
          <option value="" disabled>
            {loading ? "正在读取模型目录…" : "请选择模型"}
          </option>
          {data.model &&
            !candidates.some((model) => model.id === data.model) && (
              <option value={data.model} disabled>
                {data.model}（当前目录未返回，保留已保存标识）
              </option>
            )}
          {candidates.map((model) => (
            <option key={model.id} value={model.id}>
              {model.id}
              {model.displayName && model.displayName !== model.id
                ? ` · ${model.displayName}`
                : ""}
            </option>
          ))}
        </ActionSelect>
      </label>
      <div className="actions">
        <ActionButton
          type="button"
          disabled={loading}
          onClick={() => setRevision((value) => value + 1)}
        >
          刷新模型目录
        </ActionButton>
        {error && (
          <p className="action-feedback error" role="alert">
            {error}
          </p>
        )}
      </div>
      {source && <p className="muted">{source} · 读取目录不会发起推理。</p>}
      {!codex && (
        <>
          {[
            { key: "context_window", label: "上下文容量", max: 2000000 },
            { key: "max_output_tokens", label: "最大输出容量", max: 1000000 },
          ].map((field) => (
            <label key={field.key}>
              {field.label}
              <input
                type="number"
                min={1}
                max={field.max}
                value={
                  (field.key === "context_window"
                    ? data.context_window
                    : data.max_output_tokens) || ""
                }
                onChange={(event) =>
                  patch({
                    [field.key]: event.target.value
                      ? Number(event.target.value)
                      : undefined,
                  })
                }
              />
            </label>
          ))}
          <p className="muted">
            目录未提供容量时请明确填写；请求输出 Tokens 不得超过输出容量。
          </p>
        </>
      )}
    </div>
  );
}

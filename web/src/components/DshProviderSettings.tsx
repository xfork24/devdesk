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
  const [provider, setProvider] = useState("deepseek");
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
              <option value="deepseek">DeepSeek 官方 API</option>
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

/** Model discovery supplies candidates; it never mutates a saved model revision. */
export function DshModelFields({
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
  const {
    message,
    setMessage,
    capture: captureFeedback,
    beginFeedback,
    endFeedback,
  } = useActionFeedback();

  const [candidates, setCandidates] = useState<ModelProvider["models"]>([]);
  const [busy, setBusy] = useState(false);
  useEffect(() => {
    setCandidates([]);
    setMessage("");
  }, [data.provider, data.endpoint]);
  const selected = providers.find((p) => p.id === data.provider);
  const ollama = data.provider === "ollama";
  return (
    <div className="full-width">
      <p className="muted">
        {selected?.label} ·{" "}
        {ollama ? "通过本机 Ollama 服务连接" : selected?.endpoint}
      </p>
      {ollama && (
        <label>
          Ollama 服务地址
          <input
            type="url"
            disabled={busy}
            value={data.endpoint || ""}
            placeholder="http://127.0.0.1:11434/v1"
            onChange={(event) => {
              captureFeedback();
              return patch({ endpoint: event.target.value || undefined });
            }}
          />
        </label>
      )}
      <div className="actions">
        <ActionButton
          type="button"
          disabled={busy}
          onClick={async () => {
            captureFeedback();

            (beginFeedback(), setBusy(true));
            setMessage("");
            try {
              const result = await request("models.list", {
                model_provider: data.provider,
                ...(ollama && data.endpoint ? { endpoint: data.endpoint } : {}),
              });
              setCandidates(result.models || []);
              setMessage(
                result.source === "provider_endpoint"
                  ? "已读取账号模型目录；尚未发起推理。"
                  : result.source === "host_endpoint"
                    ? "已读取宿主服务目录；容器与推理尚未验证。"
                    : "固定版本的模型候选；账号权限及实际调用尚未验证。",
              );
            } catch (error) {
              setMessage(String(error));
            } finally {
              (endFeedback(), setBusy(false));
            }
          }}
        >
          读取模型候选
        </ActionButton>
      </div>
      {candidates.length > 0 && (
        <label>
          模型候选
          <ActionSelect
            aria-label="模型候选"
            value=""
            onChange={(event) => {
              captureFeedback();

              const candidate = candidates.find(
                (m) => m.id === event.target.value,
              );
              if (candidate)
                patch({
                  model: candidate.id,
                  context_window: candidate.context_window,
                  max_output_tokens: candidate.max_output_tokens,
                });
            }}
          >
            <option value="">选择后填入草稿，保存才生效</option>
            {candidates.map((m) => (
              <option key={m.id} value={m.id}>
                {m.id}
              </option>
            ))}
          </ActionSelect>
        </label>
      )}
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
            onChange={(event) => {
              captureFeedback();
              return patch({
                [field.key]: event.target.value
                  ? Number(event.target.value)
                  : undefined,
              });
            }}
          />
        </label>
      ))}
      <p className="muted">
        未收录的模型需明确容量；请求输出 Tokens
        不得超过输出容量。模型能力通过完整 Agent 的讨论与工具回环验证。
      </p>
      <p role="status">{message}</p>
    </div>
  );
}

import { ActionButton, ActionSelect } from "./ActionFeedback";
import { useActionFeedback } from "./action-feedback";
import { type Request } from "./rpc-types";
import { useEffect, useState } from "react";
import { DshProviderSettings } from "./DshProviderSettings";

type Provider = "codex" | "dsh";
type Installation = {
  id: string;
  key: { provider: Provider; version: string; os: string; arch: string };
  artifact: {
    kind: string;
    canonical_path?: string;
    reference?: string;
    image_id?: string;
  };
};
type Props = {
  provider: Provider;
  installations: Installation[];
  request: Request;
  refresh: () => Promise<void>;
};

export function RuntimeSettings({
  provider,
  installations,
  request,
  refresh,
}: Props) {
  const {
    message,
    setMessage,
    capture: captureFeedback,
    beginFeedback,
    endFeedback,
  } = useActionFeedback();

  const [busy, setBusy] = useState(false);
  const [preparing, setPreparing] = useState(false);
  const [runtimePreparing, setRuntimePreparing] = useState(false);
  const [phase, setPhase] = useState("");
  useEffect(() => {
    if (!preparing && !runtimePreparing) return;
    let active = true;
    const poll = async () => {
      try {
        const status = await request("codex.auth.preparation_status");
        const auth = status.preparations?.find(
          (r: { provider: string; message: string; started?: number }) =>
            r.provider === (preparing ? "codex-auth" : provider),
        );
        if (active && auth)
          setPhase(
            `${auth.message}${auth.started ? ` · 已用 ${Math.max(0, Math.floor((Date.now() - auth.started) / 1000))} 秒` : ""}`,
          );
      } catch {
        if (active) setPhase("正在读取登录准备状态…");
      }
    };
    void poll();
    const timer = window.setInterval(() => void poll(), 1000);
    return () => {
      active = false;
      window.clearInterval(timer);
    };
  }, [preparing, runtimePreparing, provider, request]);

  const [login, setLogin] = useState<{ loginId: string; authUrl: string }>();
  useEffect(() => {
    let active = true;
    void request("codex.auth.status")
      .then((status) => {
        const pending = status.pending_login;
        if (!active || !pending?.loginId || !pending.authUrl) return;
        const url = new URL(pending.authUrl);
        if (
          url.protocol === "https:" &&
          ["auth.openai.com", "auth0.openai.com"].includes(url.hostname)
        ) {
          setLogin({ loginId: pending.loginId, authUrl: url.href });
          setMessage("已恢复待完成的官方授权，可打开下方链接继续");
        }
      })
      .catch(() => {});
    return () => {
      active = false;
    };
  }, [request, setMessage]);

  return (
    <>
      <div className="setting-row">
        <div>
          <h3>默认 Agent Harness</h3>
          <p className="muted">
            分配任务或开始讨论时自动准备所选 Harness。已有执行继续使用冻结绑定。
          </p>
        </div>
        <ActionSelect
          aria-label="默认 Agent Harness"
          value={provider}
          disabled={busy}
          onChange={async (event) => {
            captureFeedback();

            beginFeedback();
            setBusy(true);
            setMessage("");
            try {
              await request("settings.update", {
                default_harness: event.target.value,
              });
              await refresh();
              window.dispatchEvent(
                new Event("devdesk:runtime-options-changed"),
              );
            } catch {
              setMessage("默认 Harness 保存失败");
            } finally {
              endFeedback();
              setBusy(false);
            }
          }}
        >
          <option value="codex">Codex</option>
          <option value="dsh">DeepSeek Harness</option>
        </ActionSelect>
      </div>
      <div className="setting-row">
        <div>
          <h3>Codex · ChatGPT OAuth</h3>
          <p className="muted">
            登录组件由 DevDesk
            自动准备，通过官方网页授权；登录信息保存在此数据集的独立目录。
          </p>
          <div className="actions">
            <ActionButton
              disabled={busy}
              onClick={async () => {
                captureFeedback();

                // Open during the click so browsers permit navigation after the asynchronous RPC.
                let authorizationWindow: Window | null = null;
                try {
                  authorizationWindow = window.open("about:blank", "_blank");
                  if (authorizationWindow) authorizationWindow.opener = null;
                } catch {
                  // The official link below also works when popups are blocked.
                }
                beginFeedback();
                setBusy(true);
                setPreparing(true);
                setPhase("检查本地登录程序…");
                setMessage("");
                try {
                  const result = await request("codex.auth.start");
                  const url = new URL(result.authUrl);
                  if (
                    url.protocol !== "https:" ||
                    !["auth.openai.com", "auth0.openai.com"].includes(
                      url.hostname,
                    )
                  )
                    throw new Error("Invalid login origin");
                  setLogin({ loginId: result.loginId, authUrl: url.href });
                  try {
                    if (authorizationWindow)
                      authorizationWindow.location.href = url.href;
                  } catch {
                    authorizationWindow?.close();
                    authorizationWindow = null;
                  }
                  void refresh().catch(() => {});
                  window.dispatchEvent(
                    new Event("devdesk:runtime-options-changed"),
                  );
                  setMessage(
                    authorizationWindow
                      ? "请在官方网页完成授权，然后检查登录状态"
                      : "浏览器未打开新窗口；请点击下方官方授权链接，完成后检查登录状态",
                  );
                } catch (error) {
                  authorizationWindow?.close();
                  setMessage(`无法开始登录，可再次点击重试：${String(error)}`);
                } finally {
                  setPreparing(false);
                  setPhase("");
                  endFeedback();
                  setBusy(false);
                }
              }}
            >
              网页登录
            </ActionButton>
            <ActionButton
              disabled={busy}
              onClick={async () => {
                captureFeedback();

                beginFeedback();
                setBusy(true);
                try {
                  const result = await request("codex.auth.status");
                  await refresh();
                  window.dispatchEvent(
                    new Event("devdesk:runtime-options-changed"),
                  );
                  setMessage(
                    result.authenticated
                      ? result.verification === "local credential presence"
                        ? "本地已保存 Codex 登录凭据；有效性将在连接时核验"
                        : "Codex OAuth 已登录"
                      : "Codex 尚未登录",
                  );
                  if (result.authenticated && !result.pending)
                    setLogin(undefined);
                } catch {
                  setMessage("无法核对登录状态，请检查运行环境或稍后重试");
                } finally {
                  endFeedback();
                  setBusy(false);
                }
              }}
            >
              检查登录状态
            </ActionButton>
            {preparing && (
              <ActionButton
                onClick={async () => {
                  await request("codex.auth.preparation_cancel");
                  setPhase("已请求取消，正在核对停止结果…");
                }}
              >
                取消登录准备
              </ActionButton>
            )}
            {login && (
              <ActionButton
                disabled={busy}
                onClick={async () => {
                  captureFeedback();

                  beginFeedback();
                  setBusy(true);
                  try {
                    await request("codex.auth.cancel", {
                      login_id: login.loginId,
                    });
                    setLogin(undefined);
                  } catch {
                    setMessage("登录取消结果未知，请核对状态");
                  } finally {
                    endFeedback();
                    setBusy(false);
                  }
                }}
              >
                取消登录
              </ActionButton>
            )}
          </div>
          {login && (
            <p>
              <a href={login.authUrl} target="_blank" rel="noopener noreferrer">
                打开官方授权网页
              </a>
              ，完成后检查登录状态。
            </p>
          )}
        </div>
      </div>
      <DshProviderSettings request={request} />
      <div className="setting-row">
        <div>
          <h3>本地安装与执行镜像</h3>
          <ActionButton
            disabled={busy}
            onClick={async () => {
              captureFeedback();
              beginFeedback();
              setBusy(true);
              setRuntimePreparing(true);
              setMessage("");
              try {
                await request("runtime.prepare", { provider });
                await refresh();
                setMessage("所选 Harness 执行镜像已准备并通过协议核验");
              } catch (error) {
                setMessage(`执行镜像准备失败：${String(error)}`);
              } finally {
                setRuntimePreparing(false);
                setPhase("");
                setBusy(false);
                endFeedback();
              }
            }}
          >
            准备所选 Harness 执行镜像
          </ActionButton>
          <p className="muted">
            固定版本程序与镜像在本机保存复用；只有缺少匹配资产时才下载或构建。
          </p>
          {installations.length === 0 ? (
            <p className="muted">尚未准备项目执行运行时；首次执行时自动准备</p>
          ) : (
            installations.map((runtime) => (
              <p key={runtime.id}>
                {runtime.key.provider} · {runtime.key.version} ·{" "}
                {runtime.key.os}/{runtime.key.arch}
                <br />
                <code>
                  {runtime.artifact.canonical_path ||
                    runtime.artifact.reference ||
                    runtime.artifact.image_id}
                </code>
              </p>
            ))
          )}
        </div>
      </div>
      {phase && <p role="status">{phase}</p>}
      <p role="status" aria-live="polite">
        {message}
      </p>
    </>
  );
}

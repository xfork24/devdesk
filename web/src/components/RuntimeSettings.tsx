import { FormEvent, useEffect, useState } from "react";

type Provider = "codex" | "dsh";
type Installation = {
  id: string;
  key: { provider: Provider; version: string; os: string; arch: string };
  artifact: { canonical_path?: string; reference?: string; image_id?: string };
};
type CredentialStatus = { configured: boolean; storage: string; available?: boolean };
type Props = {
  provider: Provider;
  installations: Installation[];
  request: (method: string, params?: Record<string, unknown>) => Promise<any>;
  refresh: () => Promise<void>;
};

export function RuntimeSettings({
  provider,
  installations,
  request,
  refresh,
}: Props) {
  const [status, setStatus] = useState<CredentialStatus>();
  const [secret, setSecret] = useState("");
  const [busy, setBusy] = useState(false);
  const [message, setMessage] = useState("");
  const [login, setLogin] = useState<{ loginId: string; authUrl: string }>();
  useEffect(() => {
    let active = true;
    request("credentials.status")
      .then((value: CredentialStatus) => {
        if (active) setStatus(value);
      })
      .catch(() => {
        if (active) setMessage("无法读取凭据状态");
      });
    return () => {
      active = false;
    };
  }, [request]);

  async function saveSecret(event: FormEvent) {
    event.preventDefault();
    setBusy(true);
    setMessage("");
    try {
      const result: CredentialStatus = await request("credentials.configure", {
        provider: "dsh",
        api_key: secret,
      });
      setStatus(result);
      setMessage("已保存到系统凭据库");
    } catch {
      setMessage("保存失败，请检查系统凭据库权限");
    } finally {
      setSecret("");
      setBusy(false);
    }
  }
  return (
    <>
      <div className="setting-row">
        <div>
          <h3>默认 Agent Harness</h3>
          <p className="muted">
            分配任务或开始讨论时自动准备所选 Harness。已有执行继续使用冻结绑定。
          </p>
        </div>
        <select
          aria-label="默认 Agent Harness"
          value={provider}
          disabled={busy}
          onChange={async (event) => {
            setBusy(true);
            setMessage("");
            try {
              await request("settings.update", {
                default_harness: event.target.value,
              });
              await refresh();
            } catch {
              setMessage("默认 Harness 保存失败");
            } finally {
              setBusy(false);
            }
          }}
        >
          <option value="codex">Codex</option>
          <option value="dsh">DeepSeek Harness</option>
        </select>
      </div>
      <div className="setting-row">
        <div>
          <h3>Codex · ChatGPT OAuth</h3>
          <p className="muted">
            登录组件由 DevDesk 自动准备，通过官方网页授权；登录信息保存在此数据集的独立目录。
          </p>
          <div className="actions">
            <button
              disabled={busy}
              onClick={async () => {
                // Open during the click so browsers permit navigation after the asynchronous RPC.
                let authorizationWindow: Window | null = null;
                try {
                  authorizationWindow = window.open("about:blank", "_blank");
                  if (authorizationWindow) authorizationWindow.opener = null;
                } catch {
                  // The official link below also works when popups are blocked.
                }
                setBusy(true);
                setMessage("正在准备登录组件并请求官方授权地址…");
                try {
                  const result = await request("codex.auth.start");
                  await refresh();
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
                  setMessage(authorizationWindow
                    ? "请在官方网页完成授权，然后检查登录状态"
                    : "浏览器未打开新窗口；请点击下方官方授权链接，完成后检查登录状态");
                } catch (error) {
                  authorizationWindow?.close();
                  setMessage(`无法开始登录，可再次点击重试：${String(error)}`);
                } finally {
                  setBusy(false);
                }
              }}
            >
              网页登录
            </button>
            <button
              disabled={busy}
              onClick={async () => {
                setBusy(true);
                try {
                  const result = await request("codex.auth.status");
                  await refresh();
                  setMessage(
                    result.authenticated
                      ? "Codex OAuth 已登录"
                      : "Codex 尚未登录",
                  );
                  if (result.authenticated) setLogin(undefined);
                } catch {
                  setMessage("无法核对登录状态，请检查运行环境或稍后重试");
                } finally {
                  setBusy(false);
                }
              }}
            >
              检查登录状态
            </button>
            {login && (
              <button
                disabled={busy}
                onClick={async () => {
                    setBusy(true);
                  try {
                    await request("codex.auth.cancel", {
                      login_id: login.loginId,
                    });
                    setLogin(undefined);
                  } catch {
                    setMessage("登录取消结果未知，请核对状态");
                  } finally {
                    setBusy(false);
                  }
                }}
              >
                取消登录
              </button>
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
      <div className="setting-row">
        <div>
          <h3>DeepSeek 官方 API key</h3>
          <p>
            {status
              ? status.configured
                ? "已配置"
                : "尚未配置"
              : "正在读取配置状态…"}
          </p>
          <p className="muted">
            保存在本机系统凭据库，不在历史或普通查询中显示。
          </p>
          {status?.available === false && <p role="alert">{status.storage === "macos_keychain" ? "凭据助手不可用；请安装同证书签名的共享助手。" : "系统凭据库不可用；Linux 请安装 libsecret-tools 并启动桌面 Secret Service。"}</p>}
          {status?.configured && <div className="actions">
            <button disabled={busy} onClick={async()=>{setBusy(true);setMessage("");try{await request("credentials.check");setMessage("已验证凭据可读取");}catch(error){setMessage(String(error));}finally{setBusy(false);}}}>检查凭据访问</button>
            {status.storage === "macos_keychain" && <button disabled={busy} onClick={async()=>{setBusy(true);setMessage("请在系统窗口中授权；稳定签名构建通常只需一次");try{await request("credentials.authorize");setMessage("已授权并验证；后台访问不会弹窗");}catch(error){setMessage(String(error));}finally{setBusy(false);}}}>授权已保存的钥匙串凭据</button>}
          </div>}
          <form onSubmit={saveSecret}>
            <label htmlFor="deepseek-api-key">配置或更换 API key</label>
            <input
              id="deepseek-api-key"
              type="password"
              autoComplete="off"
              spellCheck={false}
              value={secret}
              onChange={(event) => setSecret(event.target.value)}
              disabled={busy}
              minLength={8}
              required
            />
            <button type="submit" disabled={busy || secret.length < 8}>
              保存密钥
            </button>
          </form>
        </div>
      </div>
      <div className="setting-row">
        <div>
          <h3>共享运行时安装</h3>
          <p className="muted">运行时由 DevDesk 按需下载、构建并复用，无需手动登记。</p>
          {installations.length === 0 ? (
            <p className="muted">尚未使用 Harness；首次执行时自动准备</p>
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
      <p role="status" aria-live="polite">
        {message}
      </p>
    </>
  );
}

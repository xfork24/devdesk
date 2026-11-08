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
  const nativeCodex = installations.find(
    (runtime) =>
      runtime.key.provider === "codex" && runtime.artifact.canonical_path,
  );
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
            后续真实会话的偏好；已有执行继续使用冻结绑定。安装登记与真实调用验收分别记录。
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
            通过官方网页授权；本机认证使用独立凭据目录。容器会话的登录能力单独核验。
          </p>
          <div className="actions">
            <button
              disabled={busy}
              onClick={async () => {
                setBusy(true);
                setMessage("正在校验本机 Codex 安装…");
                try {
                  await request("runtime.discover_codex");
                  await refresh();
                  setMessage("已登记本机 Codex 安装");
                } catch {
                  setMessage("未找到可校验的本机 Codex 安装");
                } finally {
                  setBusy(false);
                }
              }}
            >
              登记本机 Codex
            </button>
            <button
              disabled={busy || !nativeCodex}
              onClick={async () => {
                if (!nativeCodex) return;
                // Open during the click so browsers permit navigation after the asynchronous RPC.
                const authorizationWindow = window.open(
                  "about:blank",
                  "_blank",
                );
                if (authorizationWindow) authorizationWindow.opener = null;
                setBusy(true);
                setMessage("正在请求官方授权地址…");
                try {
                  const result = await request("codex.auth.start", {
                    installation_id: nativeCodex.id,
                  });
                  const url = new URL(result.authUrl);
                  if (
                    url.protocol !== "https:" ||
                    !["auth.openai.com", "auth0.openai.com"].includes(
                      url.hostname,
                    )
                  )
                    throw new Error("Invalid login origin");
                  setLogin({ loginId: result.loginId, authUrl: url.href });
                  if (authorizationWindow)
                    authorizationWindow.location.href = url.href;
                  setMessage("请在官方网页完成授权，然后检查登录状态");
                } catch {
                  authorizationWindow?.close();
                  setMessage("无法开始登录，请检查安装或当前登录流程");
                } finally {
                  setBusy(false);
                }
              }}
            >
              网页登录
            </button>
            <button
              disabled={busy || !nativeCodex}
              onClick={async () => {
                if (!nativeCodex) return;
                setBusy(true);
                try {
                  const result = await request("codex.auth.status", {
                    installation_id: nativeCodex.id,
                  });
                  setMessage(
                    result.authenticated
                      ? "Codex OAuth 已登录"
                      : "Codex 尚未登录",
                  );
                  if (result.authenticated) setLogin(undefined);
                } catch {
                  setMessage("无法核对 Codex 登录状态");
                } finally {
                  setBusy(false);
                }
              }}
            >
              检查登录状态
            </button>
            {login && (
              <button
                disabled={busy || !nativeCodex}
                onClick={async () => {
                  if (!nativeCodex) return;
                  setBusy(true);
                  try {
                    await request("codex.auth.cancel", {
                      installation_id: nativeCodex.id,
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
          <div className="actions">{(["codex","dsh"] as const).map(provider=><button key={provider} disabled={busy} onClick={async()=>{setBusy(true);try{await request("runtime.discover_docker",{provider});await refresh();setMessage("已登记固定 Linux 镜像");}catch{setMessage("未找到兼容镜像；请先按接入文档构建运行时");}finally{setBusy(false);}}}>登记 {provider=== "codex"?"Codex":"DSH"} Linux 镜像</button>)}</div>
          {installations.length === 0 ? (
            <p className="muted">尚未登记安装</p>
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

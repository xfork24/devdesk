import {
  ActionButton,
  ActionForm,
  ActionSelect,
  useActionFeedback,
} from "./ActionFeedback";
import { useState } from "react";
import { DshProviderSettings } from "./DshProviderSettings";

type Provider = "codex" | "dsh";
type Installation = {
  id: string;
  key: { provider: Provider; version: string; os: string; arch: string };
  artifact: { canonical_path?: string; reference?: string; image_id?: string };
};
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
  const {
    message,
    setMessage,
    capture: captureFeedback,
    beginFeedback,
    endFeedback,
  } = useActionFeedback();

  const [busy, setBusy] = useState(false);

  const [login, setLogin] = useState<{ loginId: string; authUrl: string }>();
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

            (beginFeedback(), setBusy(true));
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
              (endFeedback(), setBusy(false));
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
                (beginFeedback(), setBusy(true));
                setMessage("正在准备登录组件并请求官方授权地址…");
                try {
                  const result = await request("codex.auth.start");
                  await refresh();
                  window.dispatchEvent(
                    new Event("devdesk:runtime-options-changed"),
                  );
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
                  setMessage(
                    authorizationWindow
                      ? "请在官方网页完成授权，然后检查登录状态"
                      : "浏览器未打开新窗口；请点击下方官方授权链接，完成后检查登录状态",
                  );
                } catch (error) {
                  authorizationWindow?.close();
                  setMessage(`无法开始登录，可再次点击重试：${String(error)}`);
                } finally {
                  (endFeedback(), setBusy(false));
                }
              }}
            >
              网页登录
            </ActionButton>
            <ActionButton
              disabled={busy}
              onClick={async () => {
                captureFeedback();

                (beginFeedback(), setBusy(true));
                try {
                  const result = await request("codex.auth.status");
                  await refresh();
                  window.dispatchEvent(
                    new Event("devdesk:runtime-options-changed"),
                  );
                  setMessage(
                    result.authenticated
                      ? "Codex OAuth 已登录"
                      : "Codex 尚未登录",
                  );
                  if (result.authenticated) setLogin(undefined);
                } catch {
                  setMessage("无法核对登录状态，请检查运行环境或稍后重试");
                } finally {
                  (endFeedback(), setBusy(false));
                }
              }}
            >
              检查登录状态
            </ActionButton>
            {login && (
              <ActionButton
                disabled={busy}
                onClick={async () => {
                  captureFeedback();

                  (beginFeedback(), setBusy(true));
                  try {
                    await request("codex.auth.cancel", {
                      login_id: login.loginId,
                    });
                    setLogin(undefined);
                  } catch {
                    setMessage("登录取消结果未知，请核对状态");
                  } finally {
                    (endFeedback(), setBusy(false));
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
          <h3>共享运行时安装</h3>
          <p className="muted">
            运行时由 DevDesk 按需下载、构建并复用，无需手动登记。
          </p>
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

import { useEffect, useState, type FormEvent } from "react";
import { ActionButton, ActionForm } from "../sdk/ActionFeedback";
import { useActionFeedback } from "../sdk/action-feedback";
import type { Request } from "../sdk/rpc-types";

type Connection = {
  revision: string;
  source: "devdesk" | "environment" | "cli";
  status: "connected" | "unconfigured" | "invalid" | "unavailable";
  login: string | null;
  message: string | null;
};

const sourceLabels = {
  devdesk: "DevDesk 系统凭据库",
  environment: "Host 环境变量",
  cli: "GitHub CLI",
};

const statusLabels = {
  connected: "已连接",
  unconfigured: "未配置",
  invalid: "认证失效",
  unavailable: "暂时无法验证",
};

export function GitHubConnection({ request }: { request: Request }) {
  const [connection, setConnection] = useState<Connection>();
  const [draft, setDraft] = useState<{ token: string; revision: string }>();
  const [busy, setBusy] = useState(false);
  const { message, error, setMessage, setError, beginFeedback, endFeedback } =
    useActionFeedback();

  useEffect(() => {
    const controller = new AbortController();
    void request("github.auth_status", {}, { signal: controller.signal })
      .then((value: Connection) => {
        if (!controller.signal.aborted) setConnection(value);
      })
      .catch((failure) => {
        if (!controller.signal.aborted) setError(String(failure));
      });
    return () => controller.abort();
  }, [request, setError]);

  async function act(event?: FormEvent) {
    event?.preventDefault();
    beginFeedback();
    setError("");
    setBusy(true);
    try {
      if (event && draft) {
        const saved = await request("github.configure_auth", {
          expected_revision: draft.revision,
          token: draft.token.trim(),
        });
        setConnection(saved);
        setDraft(undefined);
        setMessage("GitHub Token 已保存");
      } else {
        setConnection(await request("github.auth_status"));
      }
    } catch (failure) {
      setError(String(failure));
    } finally {
      endFeedback();
      setBusy(false);
    }
  }

  return (
    <section className="panel" aria-labelledby="github-connection-heading">
      <div className="panel-heading">
        <h2 id="github-connection-heading">GitHub 连接</h2>
      </div>
      <ActionForm
        className="setting-row github-connection-form"
        onSubmit={(event) => void act(event)}
      >
        <div className="github-connection-row">
          <p role="status">
            {connection
              ? [
                  statusLabels[connection.status],
                  connection.login && `@${connection.login}`,
                  sourceLabels[connection.source],
                ]
                  .filter(Boolean)
                  .join(" · ")
              : error
                ? "暂时无法读取连接"
                : "正在检查 GitHub 连接"}
          </p>
          <ActionButton
            type="button"
            disabled={busy || (!connection && !error)}
            onClick={() => void act()}
          >
            检查连接
          </ActionButton>
          {connection?.message && (
            <p className="muted full-width">{connection.message}</p>
          )}
          {error && (
            <p className="full-width" role="alert">
              {error}
            </p>
          )}
        </div>
        <div className="github-connection-row">
          <label className="github-connection-token">
            <span>GitHub Token</span>
            <input
              type="password"
              autoComplete="new-password"
              aria-describedby="github-token-help"
              spellCheck={false}
              disabled={busy || !connection}
              required
              maxLength={4096}
              value={draft?.token ?? ""}
              onChange={(event) => {
                const token = event.target.value;
                setDraft(
                  token && connection
                    ? {
                        token,
                        revision: draft?.revision ?? connection.revision,
                      }
                    : undefined,
                );
              }}
            />
          </label>
          <p id="github-token-help" className="muted">
            使用对目标仓库有权限的个人访问令牌。{" "}
            <a
              href="https://github.com/settings/personal-access-tokens"
              target="_blank"
              rel="noopener noreferrer"
            >
              管理 GitHub Token
            </a>
          </p>
          <ActionButton type="submit" disabled={busy || !draft?.token.trim()}>
            验证并保存 Token
          </ActionButton>
          {message && (
            <p className="full-width" role="status">
              {message}
            </p>
          )}
        </div>
      </ActionForm>
    </section>
  );
}

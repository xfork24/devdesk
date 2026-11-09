import {
  ActionButton,
  ActionForm,
  ActionSelect,
  useActionFeedback,
} from "./ActionFeedback";
import { FormEvent, useEffect, useRef, useState } from "react";

const addressKey = "devdesk:v1:kernel-address";

export function normalizeKernelAddress(input: string): string {
  let url: URL;
  try {
    url = new URL(input.trim());
  } catch {
    throw new Error("请输入完整的内核地址，例如 http://192.168.1.10:4317");
  }
  if (
    !["http:", "https:"].includes(url.protocol) ||
    !url.hostname ||
    url.port === "0" ||
    url.username ||
    url.password ||
    url.pathname !== "/" ||
    url.search ||
    url.hash
  ) {
    throw new Error(
      "内核地址仅包含 http(s)://主机:端口，不包含路径、账号或参数。",
    );
  }
  return url.origin;
}

function initialAddress() {
  try {
    return normalizeKernelAddress(
      localStorage.getItem(addressKey) || window.location.origin,
    );
  } catch {
    return window.location.origin === "null"
      ? "http://127.0.0.1:4317"
      : window.location.origin;
  }
}

export function KernelConnection() {
  const {
    error,
    setError,
    capture: captureFeedback,
    beginFeedback,
    endFeedback,
  } = useActionFeedback();

  const [address, setAddress] = useState(initialAddress);
  const [busy, setBusy] = useState(false);

  const request = useRef<AbortController | null>(null);
  useEffect(() => () => request.current?.abort(), []);

  async function connect(event: FormEvent) {
    captureFeedback();

    event.preventDefault();
    setError("");
    const controller = new AbortController();
    request.current = controller;
    (beginFeedback(), setBusy(true));
    try {
      const origin = normalizeKernelAddress(address);
      let response: Response;
      try {
        response = await fetch(`${origin}/connection`, {
          credentials: "omit",
          redirect: "error",
          cache: "no-store",
          signal: AbortSignal.any([
            controller.signal,
            AbortSignal.timeout(8000),
          ]),
        });
      } catch {
        throw new Error("无法连接内核，请确认地址、网络和内核是否已启动。");
      }
      const info = await response.json().catch(() => null);
      if (!response.ok || info?.product !== "devdesk") {
        throw new Error(
          "该地址未返回 DevDesk 内核信息，请确认地址或更新内核。",
        );
      }
      if (info.protocol_version !== 1) {
        throw new Error("该内核的协议版本不兼容，请更新后重试。");
      }
      try {
        localStorage.setItem(addressKey, origin);
      } catch {
        // Connecting also works when browser storage is unavailable.
      }
      // Load the target's client and session together. Project IDs, RPC,
      // artifacts and SSE remain scoped to that kernel's origin.
      window.location.assign(`${origin}/?tab=settings`);
    } catch (failure) {
      if (!controller.signal.aborted)
        setError(
          failure instanceof Error ? failure.message : "连接失败，请重试。",
        );
    } finally {
      endFeedback();
      if (!controller.signal.aborted) setBusy(false);
    }
  }

  return (
    <section className="kernel-connection" aria-label="内核连接">
      <h3>连接内核</h3>
      <p className="muted">
        当前内核：<code>{window.location.origin}</code>
      </p>
      <ActionForm onSubmit={connect} noValidate>
        <label>
          内核地址
          <input
            type="url"
            inputMode="url"
            autoComplete="url"
            placeholder="http://192.168.1.10:4317"
            value={address}
            onChange={(event) => {
              captureFeedback();
              return setAddress(event.target.value);
            }}
            disabled={busy}
            required
          />
        </label>
        <ActionButton className="primary" disabled={busy}>
          {busy ? "正在连接…" : "连接内核"}
        </ActionButton>
      </ActionForm>
      <p className="muted">
        连接后打开目标内核的工作台，使用该内核的数据与配置。
      </p>
      {error && (
        <p className="error" role="alert">
          {error}
        </p>
      )}
    </section>
  );
}

export function KernelAuthentication({
  authenticated,
}: {
  authenticated: () => Promise<void>;
}) {
  const {
    error,
    setError,
    capture: captureFeedback,
    beginFeedback,
    endFeedback,
  } = useActionFeedback();

  const [token, setToken] = useState("");
  const [busy, setBusy] = useState(false);

  const request = useRef<AbortController | null>(null);
  useEffect(() => () => request.current?.abort(), []);

  async function login(event: FormEvent) {
    captureFeedback();

    event.preventDefault();
    (beginFeedback(), setBusy(true));
    setError("");
    const controller = new AbortController();
    request.current = controller;
    try {
      const response = await fetch("/session", {
        method: "POST",
        headers: { "Content-Type": "application/json" },
        body: JSON.stringify({ access_token: token.trim() }),
        signal: AbortSignal.any([
          controller.signal,
          AbortSignal.timeout(10000),
        ]),
      });
      if (!response.ok) {
        throw new Error(
          response.status === 401
            ? "连接密钥不正确，请重试。"
            : "内核认证失败，请重试。",
        );
      }
      setToken("");
      await authenticated();
    } catch (failure) {
      if (!controller.signal.aborted)
        setError(
          failure instanceof Error ? failure.message : "无法连接内核，请重试。",
        );
    } finally {
      endFeedback();
      if (!controller.signal.aborted) setBusy(false);
    }
  }

  return (
    <section aria-label="内核认证">
      <h1>连接内核</h1>
      <p>
        此内核需要连接密钥：<code>{window.location.origin}</code>
      </p>
      <ActionForm onSubmit={login}>
        <label>
          连接密钥
          <input
            type="password"
            autoComplete="off"
            value={token}
            onChange={(event) => {
              captureFeedback();
              return setToken(event.target.value);
            }}
            disabled={busy}
            maxLength={512}
            required
          />
        </label>
        <ActionButton className="primary" disabled={busy}>
          {busy ? "正在验证…" : "验证并连接"}
        </ActionButton>
      </ActionForm>
      {error && (
        <p className="error" role="alert">
          {error}
        </p>
      )}
    </section>
  );
}

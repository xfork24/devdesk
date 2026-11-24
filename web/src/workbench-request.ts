import {
  configurationMutation,
  configurationResponseOutdated,
  foregroundRequest,
  getConfigurationSession,
  invalidateConfigurationQueries,
  invalidateModelCatalogs,
  modelMutation,
  observeConfigurationResponse,
  readConfiguration,
} from "./configuration-client";
import { observeRuntimeCommand, readRuntime } from "./runtime-state";
import {
  getCatalogGeneration,
  isTemplateMutation,
  observeCatalogGeneration,
} from "./template-catalog";
import {
  RpcResponseTimeout,
  type Request,
  type RequestOptions,
  type Row,
} from "./components/rpc-types";

let sequence = 0;
export function createRequest(transport?: Request): Request {
  return async function rpc(
    method: string,
    params: Row = {},
    options: RequestOptions = {},
  ): ReturnType<Request> {
    if (isTemplateMutation(method)) {
      if (getCatalogGeneration() === undefined)
        await rpc("query.template_catalog");
      params = {
        ...params,
        expected_catalog_generation: getCatalogGeneration(),
      };
    }

    const preparingRuntime = [
      "runtime.prepare",
      "runtime.prepare_harness",
      "runtime.prepare_sandbox",
      "session.open",
      "experience.restore",
      "codex.auth.start",
      "codex.auth.status",
      "codex.auth.cancel",
      "codex.auth.preparation_cancel",
    ].includes(method);
    if (preparingRuntime)
      window.dispatchEvent(
        new CustomEvent("devdesk.runtime-preparation", {
          detail: { pending: true },
        }),
      );
    if (!options.prefetch) foregroundRequest(true);
    try {
      const result = await readRuntime(
        method,
        params,
        (runtimeSignal) =>
          readConfiguration(
            method,
            params,
            async (signal) => {
              const id = ++sequence;
              const traceId = globalThis.crypto
                ?.randomUUID?.()
                .replaceAll("-", "");
              const timeout = AbortSignal.timeout(
                [
                  "session.open",
                  "experience.restore",
                  "runtime.prepare",
                ].includes(method)
                  ? 25 * 60 * 1000
                  : method === "codex.auth.start" ||
                      method === "configuration.templates.reset"
                    ? 5 * 60 * 1000
                    : 30000,
              );
              let data;
              try {
                if (transport) {
                  data = {
                    result: await transport(method, params, {
                      signal: AbortSignal.any([signal, timeout]),
                    }),
                  };
                } else {
                  const res = await fetch("/rpc", {
                    method: "POST",
                    headers: {
                      "Content-Type": "application/json",
                      ...(traceId ? { "X-DevDesk-Trace-Id": traceId } : {}),
                    },
                    body: JSON.stringify({
                      jsonrpc: "2.0",
                      id,
                      method,
                      params,
                    }),
                    signal: AbortSignal.any([signal, timeout]),
                  });
                  if (!res.ok)
                    throw new Error(`HTTP ${res.status} · 内核连接失败`);
                  data = await res.json();
                }
              } catch (error) {
                if (timeout.aborted && !signal.aborted) {
                  const message = method.startsWith("query.")
                    ? "读取超时，可重新读取"
                    : "内核响应超时，操作结果待确认";
                  throw new RpcResponseTimeout(
                    `${message} · ${method} · 请求 ${traceId || id}`,
                    {
                      cause: error,
                    },
                  );
                }
                throw error;
              }
              if (data.error)
                throw new Error(
                  `${data.error.data?.code || ""} · ${data.error.message}`,
                );
              if (
                method.startsWith("query.") &&
                configurationResponseOutdated(data.result, params)
              )
                throw new DOMException("Configuration changed", "AbortError");
              return data.result;
            },
            { ...options, signal: runtimeSignal },
          ),
        options,
        getConfigurationSession(),
      );
      const reset = observeConfigurationResponse(result, params);
      observeCatalogGeneration(
        result?.catalog_generation ??
          (["query.template_catalog", "configuration.templates.reset"].includes(
            method,
          )
            ? result?.generation
            : undefined),
        reset,
      );
      observeRuntimeCommand(method, params);
      if (configurationMutation(method)) invalidateConfigurationQueries();
      if (modelMutation(method)) invalidateModelCatalogs();
      return result;
    } finally {
      if (preparingRuntime)
        window.dispatchEvent(
          new CustomEvent("devdesk.runtime-preparation", {
            detail: { pending: false },
          }),
        );
      if (!options.prefetch) foregroundRequest(false);
    }
  };
}
export const standaloneRequest = createRequest();

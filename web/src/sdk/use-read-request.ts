import { useCallback, useEffect, useMemo } from "react";
import type { Request, RequestOptions, Row } from "./rpc-types";

export function useReadRequest(parent: Request): Request {
  const controller = useMemo(() => new AbortController(), []);
  useEffect(() => () => controller.abort(), [controller]);
  return useCallback(
    (method: string, params: Row = {}, options: RequestOptions = {}) => {
      if (
        options.shared ||
        !/^(query\.|models\.list|codex\.auth\.models)/.test(method)
      )
        return parent(method, params, options);
      const signal = options.signal
        ? AbortSignal.any([options.signal, controller.signal])
        : controller.signal;
      return parent(method, params, { ...options, signal });
    },
    [parent, controller],
  );
}

// A successful command may replace its read scope before its optional list refresh.
export function ignoreCancelledRefresh(error: unknown) {
  if (!(error instanceof Error) || error.name !== "AbortError") throw error;
}

import { useEffect, useRef } from "react";
import { subscribeRuntime } from "../runtime-state";

export function useRuntimeRefresh(
  load: () => Promise<void>,
  project: string,
  instance: string,
  options: {
    enabled?: boolean;
    root?: boolean;
    invalidate?: () => void;
    onError: (error: unknown) => void;
  },
) {
  const error = useRef(options.onError);
  error.current = options.onError;
  const { enabled = true, root = false, invalidate } = options;
  useEffect(() => {
    if (!enabled) return;
    let active = true;
    let running = false;
    let dirty = false;
    let timer: ReturnType<typeof setTimeout> | undefined;
    let due = 0;
    const schedule = (delay: number) => {
      const next = Date.now() + delay;
      // Continuous events must not postpone an already scheduled refresh.
      if (timer && due <= next) return;
      if (timer) clearTimeout(timer);
      due = next;
      timer = setTimeout(() => void refresh(), delay);
    };
    const refresh = async () => {
      timer = undefined;
      if (!active) return;
      if (running || document.visibilityState === "hidden") {
        dirty = true;
        return;
      }
      running = true;
      dirty = false;
      try {
        await load();
      } catch (value) {
        if (
          active &&
          !(value instanceof DOMException && value.name === "AbortError")
        )
          error.current(value);
      } finally {
        running = false;
        if (active) schedule(dirty ? 250 : 30000);
      }
    };
    const changed = () => {
      dirty = true;
      if (!running && document.visibilityState !== "hidden") schedule(250);
    };
    const unsubscribe = subscribeRuntime(project, instance, root, changed);
    document.addEventListener("visibilitychange", changed);
    void refresh();
    return () => {
      active = false;
      if (timer) clearTimeout(timer);
      unsubscribe();
      document.removeEventListener("visibilitychange", changed);
      invalidate?.();
    };
  }, [load, project, instance, enabled, root, invalidate]);
}

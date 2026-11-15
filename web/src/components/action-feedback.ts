import { useCallback, useId, useLayoutEffect, useRef, useState } from "react";
type Feedback = { text: string; kind: "status" | "alert" };
export type ActionTarget = {
  id: string;
  active: boolean;
  value: Feedback | null;
  listeners: Set<() => void>;
};
const targets = new Map<string, ActionTarget>();
let dispatchTarget: ActionTarget | null = null;

export const currentActionTarget = () => dispatchTarget;
export function publishActionFeedback(
  target: ActionTarget | null,
  text: string,
  error = false,
) {
  if (!target?.active) return false;
  target.value = text ? { text, kind: error ? "alert" : "status" } : null;
  target.listeners.forEach((listener) => listener());
  return true;
}
export function withActionTarget<T>(
  target: ActionTarget | null,
  work: () => T,
): T {
  const previous = dispatchTarget;
  dispatchTarget = target;
  try {
    return work();
  } finally {
    dispatchTarget = previous;
  }
}

export function useActionTarget() {
  const id = useId();
  const [target] = useState<ActionTarget>(() => ({
    id,
    active: true,
    value: null,
    listeners: new Set(),
  }));
  useLayoutEffect(() => {
    target.active = true;
    targets.set(id, target);
    return () => {
      target.active = false;
      targets.delete(id);
      target.listeners.clear();
    };
  }, [id, target]);
  return target;
}

export const actionTargetFor = (id: string) => targets.get(id) || null;
// Editors already clear their feedback before starting asynchronous work.
// Capture the originating control during that synchronous step, then retain
// that destination across awaits and clicks in unrelated editors.
export function useActionFeedback() {
  const [message, localMessage] = useState("");
  const [error, localError] = useState("");
  const anchor = useRef<ActionTarget | null>(null);
  const pending = useRef(0);
  const published = useRef(new Set<ActionTarget>());
  const capture = useCallback(() => {
    if (pending.current > 0) return;
    if (dispatchTarget) {
      if (anchor.current !== dispatchTarget) {
        published.current.forEach((target) =>
          publishActionFeedback(target, ""),
        );
        published.current.clear();
      }
      anchor.current = dispatchTarget;
    }
  }, []);
  const beginFeedback = useCallback(() => {
    capture();
    pending.current += 1;
  }, [capture]);
  const endFeedback = useCallback(() => {
    pending.current = Math.max(0, pending.current - 1);
  }, []);
  const write = useCallback(
    (text: string, failure: boolean) => {
      capture();
      if (!text) {
        published.current.forEach((target) =>
          publishActionFeedback(target, ""),
        );
        published.current.clear();
        localMessage("");
        localError("");
        return;
      }
      const target = dispatchTarget || anchor.current;
      if (publishActionFeedback(target, text, failure)) {
        published.current.add(target!);
        localMessage("");
        localError("");
      } else if (failure) localError(text);
      else localMessage(text);
    },
    [capture],
  );
  const setMessage = useCallback((text: string) => write(text, false), [write]);
  const setError = useCallback((text: string) => write(text, true), [write]);
  const setBackgroundError = useCallback((text: string) => {
    localError(text);
  }, []);
  return {
    message,
    error,
    setMessage,
    setError,
    setBackgroundError,
    capture,
    beginFeedback,
    endFeedback,
  };
}

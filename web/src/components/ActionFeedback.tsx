import {
  ButtonHTMLAttributes,
  FormHTMLAttributes,
  InputHTMLAttributes,
  SelectHTMLAttributes,
  forwardRef,
  useCallback,
  useId,
  useLayoutEffect,
  useRef,
  useState,
  useSyncExternalStore,
} from "react";

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

function useTarget() {
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
function Feedback({ target }: { target: ActionTarget }) {
  const subscribe = useCallback(
    (listener: () => void) => {
      target.listeners.add(listener);
      return () => {
        target.listeners.delete(listener);
      };
    },
    [target],
  );
  const value = useSyncExternalStore(subscribe, () => target.value);
  return value ? (
    <span
      className={`action-feedback ${value.kind === "alert" ? "error" : "notice"}`}
      role={value.kind}
    >
      {value.text}
    </span>
  ) : null;
}

// Native controls keep their HTML semantics and layout. Only an operation's
// own control receives its result; other editors cannot move that result.
export const ActionButton = forwardRef<
  HTMLButtonElement,
  ButtonHTMLAttributes<HTMLButtonElement>
>(function ActionButton({ onClick, ...props }, ref) {
  const target = useTarget();
  return (
    <span className="action-control">
      <button
        {...props}
        ref={ref}
        data-feedback-id={target.id}
        onClick={(event) => withActionTarget(target, () => onClick?.(event))}
      />
      <Feedback target={target} />
    </span>
  );
});
export const ActionForm = forwardRef<
  HTMLFormElement,
  FormHTMLAttributes<HTMLFormElement>
>(function ActionForm({ onSubmit, ...props }, ref) {
  return (
    <form
      {...props}
      ref={ref}
      onSubmit={(event) => {
        const submitter = (event.nativeEvent as SubmitEvent)
          .submitter as HTMLElement | null;
        const button =
          submitter ||
          event.currentTarget.querySelector<HTMLElement>(
            "button:not([type]),button[type=submit]",
          );
        withActionTarget(
          targets.get(button?.dataset.feedbackId || "") || null,
          () => onSubmit?.(event),
        );
      }}
    />
  );
});

function useControlName<T extends HTMLInputElement | HTMLSelectElement>(
  explicit?: string,
) {
  const ref = useRef<T>(null);
  const [name, setName] = useState(explicit);
  useLayoutEffect(() => {
    if (explicit || !ref.current?.labels?.length) return;
    const label = ref.current.labels[0].cloneNode(true) as HTMLElement;
    label
      .querySelectorAll("input,select,textarea,button,.action-feedback")
      .forEach((node) => node.remove());
    const text = label.textContent?.trim();
    if (text) setName(text);
  }, [explicit]);
  return { ref, name: explicit || name };
}
export function ActionSelect({
  onChange,
  ...props
}: SelectHTMLAttributes<HTMLSelectElement>) {
  const target = useTarget();
  const { ref, name } = useControlName<HTMLSelectElement>(props["aria-label"]);
  return (
    <>
      <select
        {...props}
        aria-label={name}
        ref={ref}
        data-feedback-id={target.id}
        onChange={(event) => withActionTarget(target, () => onChange?.(event))}
      />
      <Feedback target={target} />
    </>
  );
}
export function ActionInput({
  onChange,
  ...props
}: InputHTMLAttributes<HTMLInputElement>) {
  const target = useTarget();
  const { ref, name } = useControlName<HTMLInputElement>(props["aria-label"]);
  return (
    <>
      <input
        {...props}
        aria-label={name}
        ref={ref}
        data-feedback-id={target.id}
        onChange={(event) => withActionTarget(target, () => onChange?.(event))}
      />
      <Feedback target={target} />
    </>
  );
}

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

import {
  actionTargetFor,
  useActionTarget,
  withActionTarget,
  type ActionTarget,
} from "./action-feedback";
import {
  ButtonHTMLAttributes,
  FormHTMLAttributes,
  InputHTMLAttributes,
  SelectHTMLAttributes,
  forwardRef,
  useCallback,
  useLayoutEffect,
  useRef,
  useState,
  useSyncExternalStore,
} from "react";

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
  const target = useActionTarget();
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
          actionTargetFor(button?.dataset.feedbackId || ""),
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
  const target = useActionTarget();
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
  const target = useActionTarget();
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

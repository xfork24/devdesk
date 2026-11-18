import { displayText } from "../display-text";
import { useEffect, useState } from "react";
type Preparation = { provider: string; status: string; message: string };
export function RuntimePreparationStatus({
  request,
}: {
  request: (method: string) => Promise<{ preparations: Preparation[] }>;
}) {
  const [preparations, setPreparations] = useState<Preparation[]>([]);
  useEffect(() => {
    let active = true;
    let running = false;
    let dirty = false;
    let pending = 0;
    let timer: ReturnType<typeof setTimeout>;
    async function poll() {
      if (!active) return;
      if (running) {
        dirty = true;
        return;
      }
      if (document.visibilityState === "hidden") return;
      running = true;
      dirty = false;
      let delay = 30000;
      try {
        const value = await request("runtime.preparation_status");
        if (active) setPreparations(value.preparations);
        if (
          pending > 0 ||
          value.preparations.some(
            (p) => !["ready", "failed"].includes(p.status),
          )
        )
          delay = 1500;
      } catch {
        // Connection status is reported by the workspace indicator.
      } finally {
        running = false;
        if (active) timer = setTimeout(poll, dirty ? 100 : delay);
      }
    }
    const changed = (event: Event) => {
      const change = (event as CustomEvent<{ pending?: boolean }>).detail
        ?.pending;
      if (change !== undefined)
        pending = Math.max(0, pending + (change ? 1 : -1));
      clearTimeout(timer);
      void poll();
    };
    document.addEventListener("visibilitychange", changed);
    window.addEventListener("devdesk.runtime-preparation", changed);
    void poll();
    return () => {
      active = false;
      clearTimeout(timer);
      document.removeEventListener("visibilitychange", changed);
      window.removeEventListener("devdesk.runtime-preparation", changed);
    };
  }, [request]);
  return (
    <>
      {preparations
        .filter((p) => p.status !== "ready")
        .map((p) => (
          <div
            key={p.provider}
            className={p.status === "failed" ? "error" : "notice"}
            role="status"
          >
            {displayText(p.provider === "dsh" ? "DeepSeek Harness" : "Codex")} ·{" "}
            {displayText(p.message)}
          </div>
        ))}
    </>
  );
}

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
    let timer: ReturnType<typeof setTimeout>;
    async function poll() {
      try {
        const value = await request("runtime.preparation_status");
        if (active) setPreparations(value.preparations);
      } catch {
        /* Connection failures are shown by the workspace connection indicator. */
      } finally {
        if (active) timer = setTimeout(poll, 1500);
      }
    }
    void poll();
    return () => {
      active = false;
      clearTimeout(timer);
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
            {p.provider === "dsh" ? "DeepSeek Harness" : "Codex"} · {p.message}
          </div>
        ))}
    </>
  );
}

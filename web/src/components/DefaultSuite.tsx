import { useCallback, useEffect, useState } from "react";
type Row = Record<string, any>;
export function DefaultSuite({
  request,
}: {
  request: (method: string, params?: Row) => Promise<any>;
}) {
  const [data, setData] = useState<Row>(),
    [error, setError] = useState(""),
    [busy, setBusy] = useState(false);
  const load = useCallback(
    () => request("query.default_configuration").then(setData),
    [request],
  );
  useEffect(() => {
    let active = true;
    request("query.default_configuration")
      .then((value) => {
        if (active) setData(value);
      })
      .catch((error) => {
        if (active) setError(String(error));
      });
    return () => {
      active = false;
    };
  }, [request]);
  if (!data?.bundle)
    return error ? (
      <p role="alert" className="error">
        {error}
      </p>
    ) : null;
  const refs = data.bundle.data,
    records: Row[] = data.records;
  const model = records.find((row) => row.id === refs.model_ref),
    harness = records.find((row) => row.id === refs.harness_ref);
  if (!model || !harness)
    return (
      <p role="alert" className="error">
        默认套件依赖缺失，请重新读取配置。
      </p>
    );
  return (
    <section className="panel default-suite" aria-label="内建默认套件">
      <div className="panel-heading">
        <div>
          <h2>内建默认套件</h2>
          <p>
            {model.name} · Codex {harness.data.runtime_version} · v
            {data.bundle.version}
          </p>
        </div>
        <span className="badge">Global</span>
      </div>
      <p>
        五种角色已创建。新 Project 自动采用这套配置，Feature
        创建时冻结完整版本；之后可显式调整 Project 默认。
      </p>
      <p>
        模型推理强度：{model.data.parameters.reasoning_effort} · 默认
        AgentID：跨项目积累各自经验
      </p>
      <details>
        <summary>查看 Workflow、规范、角色与运行配置</summary>
        <div className="document-grid">
          {records.map((row) => (
            <article key={row.id}>
              <strong>{row.name}</strong>
              <p>
                {row.kind} · v{row.version}
              </p>
              <code>{row.id}</code>
            </article>
          ))}
        </div>
        <p>
          Permission：{data.permissions.map((row: Row) => row.name).join("、")}
        </p>
        <p>
          Sandbox：
          {data.sandboxes
            .map(
              (row: Row) =>
                `${row.provider} / ${row.platform} · ${row.version}`,
            )
            .join("、")}
        </p>
        <p className="mono">构建配方 SHA256：{harness.data.recipe_digest}</p>
      </details>
      <div className="actions">
        <button
          disabled={busy}
          onClick={async () => {
            setBusy(true);
            setError("");
            try {
              await request("runtime.prepare_defaults");
              await load();
            } catch (error) {
              setError(String(error));
            } finally {
              setBusy(false);
            }
          }}
        >
          准备默认 Codex 环境
        </button>
        <span>
          {data.materializations.length
            ? "已核验并固定本机 Docker 镜像"
            : "配置已就绪；首次执行时按固定配方准备 Docker 环境"}
        </span>
      </div>
      {error && (
        <p role="alert" className="error">
          {error}
        </p>
      )}
    </section>
  );
}

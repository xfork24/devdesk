import {
  ActionButton,
  ActionForm,
  ActionSelect,
  useActionFeedback,
} from "./ActionFeedback";
import { FormEvent, useEffect, useState } from "react";
import {
  latest,
  loadCatalog,
  type Row,
  type Request,
} from "./configuration-ui";

type Props = {
  project: Row;
  request: Request;
  refresh: () => Promise<void>;
};

function supportsModel(harness: Row, model: Row) {
  return (
    (harness.data.provider === "codex"
      ? model.data.provider === "openai"
      : model.data.provider !== "openai") &&
    (!harness.data.model_providers?.length ||
      harness.data.model_providers.includes(model.data.provider))
  );
}

export function RuntimePreferences({ project, request, refresh }: Props) {
  const { message, error, setMessage, setError, beginFeedback, endFeedback } =
    useActionFeedback();
  const [scope, setScope] = useState("project");
  const [harnessRef, setHarnessRef] = useState("");
  const [modelRef, setModelRef] = useState("");
  const [records, setRecords] = useState<Row[]>([]);
  const [loadError, setLoadError] = useState("");
  const [loading, setLoading] = useState(true);
  const [busy, setBusy] = useState(false);
  const [revision, setRevision] = useState(0);
  const preferencesJson = JSON.stringify(project.runtime_preferences || null);
  const preferences = project.runtime_preferences;
  const preference =
    scope === "project"
      ? preferences?.default
      : preferences?.roles?.find((r: Row) => r.role === scope)?.preference;

  useEffect(() => {
    const preferences = JSON.parse(preferencesJson);
    const preference =
      scope === "project"
        ? preferences?.default
        : preferences?.roles?.find((r: Row) => r.role === scope)?.preference;
    setHarnessRef(preference?.harness_ref || "");
    setModelRef(preference?.model_ref || "");
  }, [project.id, preferencesJson, scope]);

  useEffect(() => {
    let active = true;
    setLoading(true);
    loadCatalog(request, { id: project.id }, true)
      .then((records) => {
        if (active) {
          const preferences = JSON.parse(preferencesJson);
          const selected = new Set(
            [
              preferences?.default,
              ...(preferences?.roles ?? []).map((r: Row) => r.preference),
            ]
              .flatMap((p: Row | undefined) => [p?.model_ref, p?.harness_ref])
              .filter(Boolean),
          );
          setRecords(
            records.filter(
              (r) =>
                ["model", "harness"].includes(r.kind) &&
                (r.template_source?.origin !== "archived" ||
                  selected.has(r.id)),
            ),
          );
          setLoadError("");
        }
      })
      .catch((error) => {
        if (active) setLoadError(String(error));
      })
      .finally(() => {
        if (active) setLoading(false);
      });
    return () => {
      active = false;
    };
  }, [request, project.id, revision, preferencesJson]);

  useEffect(() => {
    const changed = () => setRevision((value) => value + 1);
    window.addEventListener("devdesk:runtime-options-changed", changed);
    return () =>
      window.removeEventListener("devdesk:runtime-options-changed", changed);
  }, []);

  const harness = records.find(
    (r) => r.kind === "harness" && r.id === harnessRef,
  );
  const model = records.find((r) => r.kind === "model" && r.id === modelRef);
  const canSave =
    !loading &&
    !loadError &&
    !!harness &&
    !!model &&
    supportsModel(harness, model);
  const currentRecords = latest(
    records.filter((r) => r.template_source?.origin !== "archived"),
  );
  const choices = (kind: string, selected: string) =>
    records.filter(
      (r) =>
        r.kind === kind &&
        (r.id === selected ||
          currentRecords.some((current) => current.id === r.id)),
    );

  async function save(event: FormEvent) {
    event.preventDefault();
    beginFeedback();
    setBusy(true);
    setError("");
    setMessage("");
    try {
      if (!canSave) throw new Error("请选择兼容的 Model 和 Harness。");
      const preference = {
        provider: harness!.data.provider,
        harness_ref: harnessRef,
        model_ref: modelRef,
      };
      const selection = (value: Row | null | undefined) => value ? {
        provider: value.provider, harness_ref: value.harness_ref, model_ref: value.model_ref,
      } : null;
      const previous = project.runtime_preferences;
      const old = {
        default: selection(previous?.default),
        roles: (previous?.roles || []).map((row: Row) => ({ role: row.role, preference: selection(row.preference) })),
      };
      const next =
        scope === "project"
          ? { ...old, default: preference }
          : {
              ...old,
              roles: [
                ...(old.roles || []).filter((r: Row) => r.role !== scope),
                { role: scope, preference },
              ],
            };
      await request("project.update", {
        project_id: project.id,
        command_id: crypto.randomUUID(),
        target: project.id,
        expected_revision: project.revision,
        payload: { runtime_preferences: next },
      });
      await refresh();
      setMessage("偏好已保存；已有会话继续使用冻结配置。");
    } catch (error) {
      setError(String(error));
    } finally {
      endFeedback();
      setBusy(false);
    }
  }

  return (
    <section className="setting-row">
      <div>
        <h3>项目与角色运行时偏好</h3>
        <p className="muted">
          {project.title} · 直接选择“基础组件”中已定义的 Model 和
          Harness，用于未显式指定完整 Agent 的新会话。完整 Agent
          与已有会话继续使用冻结配置。
        </p>
        {loadError && <p role="alert">{loadError}</p>}
        {preference && (!preference.model_ref || !preference.harness_ref) && (
          <p className="muted">
            当前旧偏好：{preference.provider} /{" "}
            {preference.model || "未指定模型"}。选择已定义的组件后保存即可更新。
          </p>
        )}
        <ActionForm onSubmit={save}>
          <label>
            配置范围
            <ActionSelect
              aria-label="运行时偏好范围"
              value={scope}
              onChange={(event) => setScope(event.target.value)}
              disabled={busy}
            >
              <option value="project">项目默认</option>
              {Object.entries({
                analyst: "需求分析师",
                architect: "架构师 / 项目规划",
                developer: "开发",
                tester: "测试",
                reviewer: "评审",
              }).map(([key, label]) => (
                <option key={key} value={key}>
                  {label}
                </option>
              ))}
            </ActionSelect>
          </label>
          <label>
            Harness
            <ActionSelect
              aria-label="项目角色 Harness"
              value={harnessRef}
              required
              disabled={busy || loading}
              onChange={(event) => {
                setHarnessRef(event.target.value);
                const next = records.find((r) => r.id === event.target.value);
                if (!next || !model || !supportsModel(next, model)) {
                  setModelRef("");
                }
              }}
            >
              <option value="" disabled>
                {loading ? "正在读取基础组件…" : "请选择已定义的 Harness"}
              </option>
              {harnessRef && !harness && (
                <option value={harnessRef} disabled>
                  原 Harness 配置不可用，请重新选择
                </option>
              )}
              {choices("harness", harnessRef).map((r) => (
                <option key={r.id} value={r.id}>
                  {r.name} · v{r.version}
                  {r.template_source?.origin === "archived"
                    ? " · 重置前固定版本"
                    : ""}{" "}
                  · {r.data.provider} {r.data.runtime_version}
                </option>
              ))}
            </ActionSelect>
          </label>
          <label>
            Model
            <ActionSelect
              aria-label="项目角色模型"
              value={modelRef}
              required
              disabled={busy || loading || !harness}
              onChange={(event) => {
                setModelRef(event.target.value);
              }}
            >
              <option value="" disabled>
                请选择已定义的 Model
              </option>
              {modelRef && !model && (
                <option value={modelRef} disabled>
                  原 Model 配置不可用，请重新选择
                </option>
              )}
              {choices("model", modelRef)
                .filter(
                  (r) =>
                    r.id === modelRef || (harness && supportsModel(harness, r)),
                )
                .map((r) => (
                  <option
                    key={r.id}
                    value={r.id}
                    disabled={!!harness && !supportsModel(harness, r)}
                  >
                    {r.name} · v{r.version}
                    {r.template_source?.origin === "archived"
                      ? " · 重置前固定版本"
                      : ""}{" "}
                    · {r.data.model}
                  </option>
                ))}
            </ActionSelect>
          </label>
          {model && (
            <p className="muted">
              模型 ID：{model.data.model} · 服务商：{model.data.provider}
              。使用所选 Model 的参数与容量配置。
            </p>
          )}
          <p className="muted">
            需要新增或修改组件时，前往<a href="?tab=components">基础组件</a>
            。新会话按所选 Harness 的固定版本准备运行时。
          </p>
          <div className="actions">
            <ActionButton disabled={busy || !canSave}>保存偏好</ActionButton>
            <ActionButton
              type="button"
              disabled={busy || loading}
              onClick={() => setRevision((value) => value + 1)}
            >
              刷新组件
            </ActionButton>
          </div>
        </ActionForm>
        {error && <p role="alert">{error}</p>}
        {message && <p role="status">{message}</p>}
      </div>
    </section>
  );
}

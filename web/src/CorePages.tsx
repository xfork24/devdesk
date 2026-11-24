import { useCallback, useEffect, useState } from "react";
import { ActionButton, ActionForm } from "./components/ActionFeedback";
import { ConfigurationPage } from "./components/ConfigurationPages";
import { ConfigurationScope } from "./components/configuration-scope";
import { deferPage } from "./components/deferred-page";
import { KernelConnection } from "./components/KernelConnection";
import { RuntimePreparationStatus } from "./components/RuntimePreparationStatus";
import { RuntimeSettings } from "./components/RuntimeSettings";
import { TemplateSettings } from "./components/TemplateSettings";
import type { ResourceVersion, Row } from "./components/rpc-types";
import { standaloneRequest as request } from "./workbench-request";

const { Page: CallHistory } = deferPage(() =>
  import("./components/Integrations").then((module) => module.CallHistory),
);

const coreConfigurationScope = { request, storageNamespace: "core" };

export function CorePages({ page }: { page: "core" | "history" | "settings" }) {
  if (page === "history")
    return (
      <>
        <header className="page-heading">
          <h1>历史记录</h1>
        </header>
        <CallHistory request={request} historyScope="global" />
      </>
    );
  return (
    <ConfigurationScope.Provider value={coreConfigurationScope}>
      <CoreConfiguration page={page} />
    </ConfigurationScope.Provider>
  );
}

function CoreConfiguration({ page }: { page: "core" | "settings" }) {
  const [data, setData] = useState<Row>();
  const [error, setError] = useState("");
  const refresh = useCallback(async () => {
    setData(await request("query.ui_bootstrap", {}, { fresh: true }));
  }, []);
  useEffect(() => {
    const controller = new AbortController();
    void request("query.ui_bootstrap", {}, { signal: controller.signal }).then(
      setData,
      (failure: unknown) => {
        if (!controller.signal.aborted) setError(String(failure));
      },
    );
    return () => controller.abort();
  }, []);

  if (error) return <p role="alert">{error}</p>;
  if (!data) return <p role="status">正在读取工作台…</p>;
  return (
    <>
      <RuntimePreparationStatus request={request} />
      {page === "core" ? (
        <ConfigurationPage
          group="foundations"
          request={request}
          installations={data.runtime_installations}
          refresh={refresh}
        />
      ) : (
        <>
          <header className="page-heading">
            <h1>设置</h1>
          </header>
          <section className="panel">
            <div className="panel-heading">
              <h2>数据与运行</h2>
            </div>
            <KernelConnection />
            <div className="setting-row">
              <div>
                <h3>数据目录</h3>
                <code>{data.data_root}</code>
              </div>
            </div>
          </section>
          <RuntimeSettings
            request={request}
            provider={data.default_harness}
            installations={data.runtime_installations}
            settingsVersions={data.settings_versions}
            refresh={refresh}
          />
          <section className="panel" aria-label="模板与额度设置">
            <TemplateSettings request={request} refresh={refresh} />
            <ConcurrencySettings data={data} refresh={refresh} />
          </section>
        </>
      )}
    </>
  );
}

function ConcurrencySettings({
  data,
  refresh,
}: {
  data: Row;
  refresh: () => Promise<void>;
}) {
  const [busy, setBusy] = useState(false);
  const [feedback, setFeedback] = useState<{ saved: boolean; text: string }>();
  const limits = [
    ["global_concurrency", "全局并发额度"],
    ["default_project_concurrency", "单项目默认并发额度"],
  ] as const;
  return (
    <ActionForm
      className="setting-row"
      key={String(
        data.settings_versions.map(
          (version: ResourceVersion) => version.revision,
        ),
      )}
      onSubmit={async (event) => {
        event.preventDefault();
        setBusy(true);
        setFeedback(undefined);
        const form = new FormData(event.currentTarget);
        try {
          await request("settings.update", {
            command_id: crypto.randomUUID(),
            preconditions: data.settings_versions.filter(
              (version: ResourceVersion) =>
                version.resource.kind === "setting" &&
                limits.some(([id]) => id === version.resource.id),
            ),
            payload: Object.fromEntries(
              limits.map(([id]) => [id, Number(form.get(id))]),
            ),
          });
          await refresh();
          setFeedback({ saved: true, text: "并发额度已保存" });
        } catch (failure) {
          setFeedback({ saved: false, text: String(failure) });
        } finally {
          setBusy(false);
        }
      }}
    >
      <div>
        <h3>并发额度设置</h3>
        {limits.map(([id, label]) => (
          <p key={id}>
            <label>
              {label}
              <input
                name={id}
                type="number"
                min={1}
                max={32}
                required
                defaultValue={data[id]}
              />
            </label>
          </p>
        ))}
        {feedback && (
          <p role={feedback.saved ? "status" : "alert"}>{feedback.text}</p>
        )}
      </div>
      <ActionButton disabled={busy}>保存额度</ActionButton>
    </ActionForm>
  );
}

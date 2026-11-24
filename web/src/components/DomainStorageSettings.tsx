import { useEffect, useState } from "react";
import { ActionButton, ActionForm } from "./ActionFeedback";
import { standaloneRequest as request } from "../workbench-request";

type StorageSettings = {
  package_id: string;
  revision: number;
  configured_root: string | null;
  data_root: string;
  project_roots: { data_root: string; projects: number }[];
};

export function DomainStorageSettings({ epoch }: { epoch: number }) {
  const [settings, setSettings] = useState<StorageSettings>();
  const [busy, setBusy] = useState(false);
  const [error, setError] = useState("");
  const [saved, setSaved] = useState(false);
  useEffect(() => {
    const controller = new AbortController();
    void request(
      "query.domain.storage",
      { epoch },
      { signal: controller.signal, fresh: true },
    ).then(
      (data) => setSettings(data as StorageSettings),
      (failure: unknown) => {
        if (!controller.signal.aborted) setError(String(failure));
      },
    );
    return () => controller.abort();
  }, [epoch]);

  return (
    <section className="panel" aria-label="领域数据目录设置">
      <div className="panel-heading">
        <h2>领域数据目录</h2>
        {settings && <code>{settings.package_id}</code>}
      </div>
      {settings ? (
        <ActionForm
          className="setting-row"
          key={settings.revision}
          onSubmit={async (event) => {
            event.preventDefault();
            const path = String(
              new FormData(event.currentTarget).get("data_root") ?? "",
            ).trim();
            setBusy(true);
            setError("");
            setSaved(false);
            try {
              const data = await request("domain.storage.configure", {
                epoch,
                expected_revision: settings.revision,
                data_root: path || null,
              });
              setSettings(data as StorageSettings);
              setSaved(true);
            } catch (failure) {
              setError(String(failure));
            } finally {
              setBusy(false);
            }
          }}
        >
          <div>
            <label>
              新项目数据目录
              <input
                name="data_root"
                defaultValue={settings.configured_root ?? ""}
                placeholder={settings.data_root}
                spellCheck={false}
              />
            </label>
            <p className="muted">
              输入 Host 所在机器上的绝对路径，留空使用 Host 数据目录。
              新项目的数据库、仓库和产物写入此处。已有项目继续使用各自的路径。
            </p>
            <p>
              当前生效目录 <code>{settings.data_root}</code>
            </p>
            {settings.project_roots.length > 0 && (
              <ul aria-label="已有项目的数据目录">
                {settings.project_roots.map((item) => (
                  <li key={item.data_root}>
                    <code>{item.data_root}</code> · {item.projects} 个项目
                  </li>
                ))}
              </ul>
            )}
          </div>
          <ActionButton type="submit" disabled={busy}>
            保存领域数据目录
          </ActionButton>
        </ActionForm>
      ) : (
        !error && <p role="status">正在读取领域数据目录…</p>
      )}
      {error && <p role="alert">{error}</p>}
      {saved && <p role="status">领域数据目录已保存</p>}
    </section>
  );
}

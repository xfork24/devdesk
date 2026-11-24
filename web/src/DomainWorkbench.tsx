import {
  Component,
  Suspense,
  lazy,
  useCallback,
  useEffect,
  useMemo,
  useLayoutEffect,
  useRef,
  useState,
  type ComponentType,
  type FormEvent,
  type ReactNode,
} from "react";
import { DomainProvider } from "./domain-sdk";
import {
  FolderMinus,
  History,
  LayoutDashboard,
  Library,
  PackageOpen,
  Settings,
} from "lucide-react";
import { ActionButton, ActionForm } from "./components/ActionFeedback";
import { WorkbenchLayout } from "./workbench-layout";
import { KernelAuthentication } from "./components/KernelConnection";
import type { Data, DomainContext } from "./domain-sdk";

type Package = { id: string; version: string; digest: string };
type Contribution = {
  id: string;
  title: string;
  query: string;
  projection?: boolean;
  fields: { key: string; label: string }[];
  inputs: { key: string; label: string }[];
};
type Binding = { contribution: string; entry: string; export: string };
type Directory = {
  epoch: number;
  active: Package | null;
  contributions: Contribution[] | null;
  targets:
    | {
        renderer: string;
        api: number;
        bindings: Binding[];
        styles?: string[];
      }[]
    | null;
  sources: CoreRecord[] | null;
  agent_workflows?: string[] | null;
  project_connection?: string;
  entity_types?: Record<string, { workflows?: Record<string, unknown> }> | null;
};
type CoreRecord = { id: string; name: string; type: string };
const CorePages = lazy(() =>
  import("./CorePages").then(({ CorePages }) => ({ default: CorePages })),
);
const DomainStorageSettings = lazy(() =>
  import("./components/DomainStorageSettings").then(
    ({ DomainStorageSettings }) => ({ default: DomainStorageSettings }),
  ),
);

async function rpc<T>(
  method: string,
  params: Data = {},
  signal?: AbortSignal,
): Promise<T> {
  const response = await fetch("/rpc", {
    method: "POST",
    signal,
    headers: { "content-type": "application/json" },
    body: JSON.stringify({
      jsonrpc: "2.0",
      id: crypto.randomUUID(),
      method,
      params,
    }),
  });
  if (!response.ok) throw new Error(`HTTP ${response.status}`);
  const body = await response.json();
  if (body.error)
    throw new Error(`${body.error.data?.code ?? ""} · ${body.error.message}`);
  return body.result as T;
}

function message(error: unknown) {
  return error instanceof Error ? error.message : String(error);
}

function DomainProjectRemoval({
  epoch,
  project,
}: {
  epoch: number;
  project: string;
}) {
  const [busy, setBusy] = useState(false);
  const [error, setError] = useState("");
  return (
    <span className="domain-project-removal">
      <ActionButton
        type="button"
        disabled={busy}
        title="再次打开同一仓库和主分支即可恢复"
        onClick={async () => {
          setBusy(true);
          setError("");
          try {
            await rpc("domain.project.remove", {
              epoch,
              project_id: project,
              command_id: crypto.randomUUID(),
            });
            const url = new URL(location.href);
            if (url.searchParams.get("project_id") === project) {
              const next = new URLSearchParams();
              for (const key of ["view", "tab"]) {
                const value = url.searchParams.get(key);
                if (value) next.set(key, value);
              }
              url.search = next.toString();
              history.replaceState(null, "", url);
              window.dispatchEvent(new PopStateEvent("popstate"));
            }
          } catch (failure) {
            setError(message(failure));
          } finally {
            setBusy(false);
          }
        }}
      >
        <FolderMinus size={15} aria-hidden="true" /> 移除项目
      </ActionButton>
      {error && <span className="error" role="alert">{error}</span>}
    </span>
  );
}

class PageBoundary extends Component<
  { children: ReactNode },
  { failed: boolean }
> {
  state = { failed: false };
  static getDerivedStateFromError() {
    return { failed: true };
  }
  render() {
    return this.state.failed ? (
      <p role="alert">领域页面无法载入。</p>
    ) : (
      this.props.children
    );
  }
}

function DomainPage({
  directory,
  contribution,
}: {
  directory: Directory;
  contribution: Contribution;
}) {
  const target = directory.targets?.find(
    ({ renderer, api }) => renderer === "web.react" && (api === 1 || api === 2),
  );
  const binding = target?.bindings.find(
    ({ contribution: id }) => id === contribution.id,
  );
  const epoch = directory.epoch;
  const base = `/domain-assets/${epoch}/${directory.active?.digest}/`;
  const entry = binding && base + binding.entry;
  const styles = binding ? target?.styles : undefined;
  useLayoutEffect(() => {
    const links = (styles ?? []).map((style) => {
      const link = document.createElement("link");
      link.rel = "stylesheet";
      link.href = base + style;
      document.head.append(link);
      return link;
    });
    return () => links.forEach((link) => link.remove());
  }, [base, styles]);
  const exportName = binding?.export;
  const Page = useMemo(
    () =>
      entry && exportName
        ? lazy(async () => {
            const module = await import(/* @vite-ignore */ entry);
            const page: unknown = module[exportName];
            if (typeof page !== "function") throw new Error("领域页面入口无效");
            return { default: page as ComponentType };
          })
        : null,
    [entry, exportName],
  );
  const lifetime = useRef<AbortController | null>(null);
  useLayoutEffect(() => {
    const controller = new AbortController();
    lifetime.current = controller;
    return () => controller.abort();
  }, []);
  const context = useMemo<DomainContext>(
    () => ({
      epoch,
      view: contribution.id,
      projectConnection: directory.project_connection,
      query: (handler, input) =>
        rpc(
          "query.domain.invoke",
          { epoch, handler, input },
          lifetime.current?.signal,
        ),
      request: (method, params, options) => {
        const signals = [lifetime.current?.signal, options?.signal].filter(
          (signal): signal is AbortSignal => !!signal,
        );
        const signal = AbortSignal.any(signals);
        const projection = directory.contributions?.find(
          (item) => item.projection && item.query === method,
        );
        return projection
          ? rpc(
              "query.domain.contribution",
              {
                epoch,
                contribution: projection.id,
                input: params,
              },
              signal,
            )
          : rpc(method, { ...params, domain_epoch: epoch }, signal);
      },
      events: (projectId) => {
        const params = new URLSearchParams({
          tail: "true",
          configuration: "true",
          domain_epoch: String(epoch),
        });
        if (projectId) params.set("project_id", projectId);
        return new EventSource(`/events?${params}`);
      },
    }),
    [
      epoch,
      contribution.id,
      directory.project_connection,
      directory.contributions,
    ],
  );
  if (target?.api === 2 && Page)
    return (
      <DomainProvider.Provider value={context}>
        <PageBoundary>
          <Suspense fallback={<p role="status">正在载入领域页面…</p>}>
            <Page />
          </Suspense>
        </PageBoundary>
      </DomainProvider.Provider>
    );
  return (
    <main>
      <section className="panel">
        <h2>{contribution.title}</h2>
        <DomainProvider.Provider value={context}>
          <PageBoundary>
            <Suspense fallback={<p role="status">正在载入领域页面…</p>}>
              {Page ? (
                <Page />
              ) : (
                <CoreQuery
                  handler={contribution.query}
                  run={(_, input) =>
                    contribution.projection
                      ? rpc("query.domain.contribution", {
                          epoch,
                          contribution: contribution.id,
                          input,
                        })
                      : context.query(contribution.query, input)
                  }
                  fields={contribution.inputs}
                  resultFields={contribution.fields}
                />
              )}
            </Suspense>
          </PageBoundary>
        </DomainProvider.Provider>
      </section>
    </main>
  );
}

type Field = { key: string; label: string };

function CoreQuery({
  handler,
  run,
  fields = [{ key: "path", label: "本地仓库路径" }],
  resultFields,
}: {
  handler: string;
  run: (handler: string, input: Data) => Promise<unknown>;
  fields?: Field[];
  resultFields?: Field[];
}) {
  const [result, setResult] = useState<unknown>();
  const [error, setError] = useState("");
  const [busy, setBusy] = useState(false);
  async function submit(event: FormEvent<HTMLFormElement>) {
    event.preventDefault();
    const data = Object.fromEntries(new FormData(event.currentTarget));
    setBusy(true);
    setError("");
    try {
      setResult(await run(handler, data));
    } catch (failure) {
      setError(message(failure));
    } finally {
      setBusy(false);
    }
  }
  const displayed =
    resultFields && result && typeof result === "object"
      ? resultFields.filter(({ key }) => Object.hasOwn(result, key))
      : [];
  return (
    <form onSubmit={submit}>
      {fields.map(({ key, label }) => (
        <p key={key}>
          <label>
            {label}
            <input name={key} required />
          </label>
        </p>
      ))}
      {handler === "query.core.git.diff" && (
        <>
          <p>
            <label>
              起始提交
              <input name="from" required />
            </label>
          </p>
          <p>
            <label>
              结束提交
              <input name="to" defaultValue="HEAD" required />
            </label>
          </p>
        </>
      )}
      <button type="submit" disabled={busy}>
        查询
      </button>
      {error && <p role="alert">{error}</p>}
      {displayed.length > 0 ? (
        <dl>
          {displayed.map(({ key, label }) => (
            <div key={key}>
              <dt>{label}</dt>
              <dd>
                {typeof (result as Data)[key] === "object" ? (
                  <pre>{JSON.stringify((result as Data)[key], null, 2)}</pre>
                ) : (
                  String((result as Data)[key])
                )}
              </dd>
            </div>
          ))}
        </dl>
      ) : (
        result !== undefined && <pre>{JSON.stringify(result, null, 2)}</pre>
      )}
    </form>
  );
}

function DatasetSetup({ opened }: { opened: () => Promise<void> }) {
  const [busy, setBusy] = useState(false);
  const [error, setError] = useState("");
  return (
    <section className="panel">
      <div className="panel-heading">
        <h2>打开数据集</h2>
      </div>
      <ActionForm
        onSubmit={async (event) => {
          event.preventDefault();
          const path = new FormData(event.currentTarget).get("path");
          setBusy(true);
          setError("");
          try {
            const capabilities = await fetch("/capabilities").then((response) =>
              response.json(),
            );
            if (capabilities.setup_required) {
              const response = await fetch("/setup", {
                method: "POST",
                headers: { "content-type": "application/json" },
                body: JSON.stringify({
                  path,
                  command_id: crypto.randomUUID(),
                  instance_id: capabilities.lifecycle.instance_id,
                  expected_revision: capabilities.lifecycle.revision,
                }),
              });
              const result = await response.json();
              if (result.error) throw new Error(result.error);
            }
            await opened();
          } catch (failure) {
            setError(message(failure));
          } finally {
            setBusy(false);
          }
        }}
      >
        <label>
          数据目录
          <input name="path" defaultValue="./data" disabled={busy} required />
        </label>
        <ActionButton className="primary" disabled={busy}>
          初始化 / 打开数据集
        </ActionButton>
      </ActionForm>
      {error && <p role="alert">{error}</p>}
    </section>
  );
}

function WorkflowLookup({ directory }: { directory: Directory }) {
  const [run, setRun] = useState({ project_id: "", execution_id: "" });
  const [selection, setSelection] = useState({ workflow: "", mode: "rust" });
  const [value, setValue] = useState<unknown>();
  const [error, setError] = useState("");
  const [busy, setBusy] = useState(false);

  async function submit(action: () => Promise<void>) {
    setError("");
    setBusy(true);
    try {
      await action();
    } catch (reason) {
      setError(message(reason));
    } finally {
      setBusy(false);
    }
  }

  const workflows =
    directory.sources?.filter((row) => row.type === "workflow") ?? [];
  const workflow = selection.workflow || workflows[0]?.id || "";
  const needsOwner = Object.values(directory.entity_types ?? {}).some(
    (type) => type.workflows?.[workflow],
  );
  const canUseAgent = directory.agent_workflows?.includes(workflow) ?? false;
  const mode =
    canUseAgent || selection.mode !== "agent" ? selection.mode : "rust";
  return (
    <section className="panel">
      <h2>领域工作流</h2>
      {workflows.length > 0 && (
        <form
          onSubmit={(event) => {
            event.preventDefault();
            const owner = new FormData(event.currentTarget);
            void submit(async () => {
              const receipt = await rpc<typeof run>("domain.workflow.start", {
                epoch: directory.epoch,
                workflow_ref: workflow,
                mode,
                command_id: crypto.randomUUID(),
                ...(needsOwner && {
                  project_id: owner.get("owner_project"),
                  entity_id: owner.get("owner_entity"),
                  expected_revision: Number(owner.get("owner_revision")),
                }),
              });
              setRun(receipt);
              setValue(undefined);
            });
          }}
        >
          <label>
            工作流
            <select
              name="workflow_ref"
              value={workflow}
              onChange={(event) =>
                setSelection({ ...selection, workflow: event.target.value })
              }
            >
              {workflows.map((row) => (
                <option key={row.id} value={row.id}>
                  {row.name}
                </option>
              ))}
            </select>
          </label>
          <label>
            调度方式
            <select
              value={mode}
              onChange={(event) =>
                setSelection({ ...selection, mode: event.target.value })
              }
            >
              <option value="rust">Rust</option>
              <option value="manual">人工</option>
              {canUseAgent && <option value="agent">Agent</option>}
            </select>
          </label>
          {needsOwner && (
            <>
              <label>
                归属项目 ID
                <input name="owner_project" required />
              </label>
              <label>
                归属对象 ID
                <input name="owner_entity" required />
              </label>
              <label>
                归属对象修订
                <input name="owner_revision" min="1" type="number" required />
              </label>
            </>
          )}
          <button disabled={busy}>启动工作流</button>
        </form>
      )}
      <form
        onSubmit={(event) => {
          event.preventDefault();
          void submit(async () =>
            setValue(
              await rpc("query.domain.workflow", {
                epoch: directory.epoch,
                ...run,
              }),
            ),
          );
        }}
      >
        <label>
          项目 ID
          <input
            name="project_id"
            required
            value={run.project_id}
            onChange={(event) =>
              setRun({ ...run, project_id: event.target.value })
            }
          />
        </label>
        <label>
          执行 ID
          <input
            name="execution_id"
            required
            value={run.execution_id}
            onChange={(event) =>
              setRun({ ...run, execution_id: event.target.value })
            }
          />
        </label>
        <button disabled={busy}>读取执行</button>
      </form>
      {error && <p role="alert">{error}</p>}
      {value !== undefined && (
        <pre aria-label="领域执行结果">{JSON.stringify(value, null, 2)}</pre>
      )}
    </section>
  );
}

async function archive(file: File) {
  if (file.size > 2 * 1024 * 1024) throw new Error("领域 ZIP 超出大小限制");
  return await new Promise<string>((resolve, reject) => {
    const reader = new FileReader();
    reader.onerror = () => reject(new Error("无法读取 ZIP"));
    reader.onload = () => resolve(String(reader.result).split(",")[1]);
    reader.readAsDataURL(file);
  });
}

function workbenchEntry(directory?: Directory) {
  const target = directory?.targets?.find(
    ({ renderer, api }) => renderer === "web.react" && api === 2,
  );
  return target?.bindings[0]?.contribution;
}

export function AppRoot() {
  const [directory, setDirectory] = useState<Directory>();
  const [packages, setPackages] = useState<Package[]>([]);
  const [selected, setSelected] = useState(() =>
    new URLSearchParams(location.search).get("view"),
  );
  const [error, setError] = useState("");
  const [auth, setAuth] = useState(false);
  const [needsDataset, setNeedsDataset] = useState(false);
  const [busy, setBusy] = useState(false);
  const projectAction = useCallback(
    (project: string) =>
      directory?.project_connection ? (
        <DomainProjectRemoval
          key={project}
          epoch={directory.epoch}
          project={project}
        />
      ) : null,
    [directory?.epoch, directory?.project_connection],
  );
  const refresh = useCallback(async () => {
    const [next, installed] = await Promise.all([
      rpc<Directory>("query.domain.active"),
      rpc<Package[]>("query.domain.installed"),
    ]);
    setDirectory((previous) =>
      previous && previous.epoch > next.epoch ? previous : next,
    );
    setPackages(installed);
    setNeedsDataset(false);
    return next;
  }, []);
  useEffect(() => {
    const events = new EventSource("/domain-events");
    const reload = () =>
      void refresh().catch((failure: unknown) => {
        const detail = message(failure);
        if (detail === "HTTP 401") setAuth(true);
        else if (detail.startsWith("setup_required ·")) setNeedsDataset(true);
        else setError(detail);
      });
    events.addEventListener("domain", reload);
    reload();
    const pop = () =>
      setSelected(new URLSearchParams(location.search).get("view"));
    window.addEventListener("popstate", pop);
    return () => {
      events.close();
      window.removeEventListener("popstate", pop);
    };
  }, [refresh]);
  function select(id: string) {
    const params = new URLSearchParams({ view: id });
    const activeContributions = directory?.contributions ?? [];
    if (activeContributions.some((item) => item.id === id)) {
      const current = new URLSearchParams(location.search);
      if (activeContributions.some((item) => item.id === current.get("view"))) {
        const project = current.get("project_id");
        if (project) params.set("project_id", project);
      }
    }
    if (location.search !== `?${params}`)
      history.pushState({}, "", `?${params}`);
    setSelected(id);
  }
  async function mutate(action: () => Promise<unknown>, enterDomain = false) {
    setError("");
    setBusy(true);
    try {
      await action();
      const next = await refresh();
      const home = enterDomain && workbenchEntry(next);
      if (home) select(home);
    } catch (failure) {
      setError(message(failure));
    } finally {
      setBusy(false);
    }
  }
  const home = workbenchEntry(directory);
  const corePage =
    selected === "core" || selected === "history" || selected === "settings"
      ? selected
      : selected === null && !directory?.active
        ? "core"
        : undefined;
  const platformView =
    selected === "packages" ||
    !!corePage ||
    (!home && (selected === "git" || selected === "workflow"));
  const contributions = directory?.contributions;
  const contribution =
    contributions?.find((item) => item.id === selected) ??
    (!platformView
      ? contributions?.find((item) => item.id === home)
      : undefined);
  const destination = contribution?.id;
  useLayoutEffect(() => {
    if (!destination || destination === selected) return;
    const params = new URLSearchParams(selected ? "" : location.search);
    params.set("view", destination);
    history.replaceState({}, "", `?${params}`);
    setSelected(destination);
  }, [destination, selected]);

  if (auth)
    return (
      <KernelAuthentication
        authenticated={async () => {
          try {
            await refresh();
          } catch (failure) {
            if (!message(failure).startsWith("setup_required ·")) throw failure;
            setNeedsDataset(true);
          }
          setAuth(false);
        }}
      />
    );
  return (
    <WorkbenchLayout
      domainProjectAction={
        contribution && directory?.project_connection
          ? projectAction
          : undefined
      }
      domainSettings={
        directory?.active && contribution ? (
          <Suspense fallback={null}>
            <DomainStorageSettings
              key={directory.epoch}
              epoch={directory.epoch}
            />
          </Suspense>
        ) : undefined
      }
      navigation={
        <nav aria-label="工作台管理">
          <ActionButton
            className={selected === "packages" ? "nav active" : "nav"}
            aria-current={selected === "packages" ? "page" : undefined}
            onClick={() => select("packages")}
          >
            <PackageOpen size={18} aria-hidden="true" /> 领域管理
          </ActionButton>
        </nav>
      }
      fallbackNavigation={
        <nav aria-label="工作台导航">
          {home && !contribution && (
            <ActionButton className="nav" onClick={() => select(home)}>
              <LayoutDashboard size={18} aria-hidden="true" /> 领域工作台
            </ActionButton>
          )}
          {(
            [
              ["core", "基础组件", Library],
              ["history", "历史记录", History],
              ["settings", "设置", Settings],
            ] as const
          ).map(([id, title, Icon]) => {
            const active = corePage === id;
            return (
              <ActionButton
                key={id}
                className={active ? "nav active" : "nav"}
                aria-current={active ? "page" : undefined}
                onClick={() => select(id)}
              >
                <Icon size={18} /> {title}
              </ActionButton>
            );
          })}
          {!home && directory?.active && (
            <ActionButton className="nav" onClick={() => select("workflow")}>
              领域工作流
            </ActionButton>
          )}
          {(!home || contribution) &&
            contributions?.map((item) => (
              <ActionButton
                className="nav"
                aria-current={selected === item.id ? "page" : undefined}
                key={item.id}
                onClick={() => select(item.id)}
              >
                {item.title}
              </ActionButton>
            ))}
        </nav>
      }
    >
      {error && <p role="alert">{error}</p>}
      {directory && contribution ? (
        <DomainPage
          key={`${directory.epoch}:${contribution.id}`}
          directory={directory}
          contribution={contribution}
        />
      ) : (
        <main>
          {needsDataset ? (
            <DatasetSetup
              opened={async () => {
                await refresh();
              }}
            />
          ) : !directory ? (
            <p role="status">正在连接工作台…</p>
          ) : corePage ? (
            <Suspense fallback={<p role="status">正在读取工作台…</p>}>
              <CorePages page={corePage} />
            </Suspense>
          ) : selected === "workflow" && directory?.active ? (
            <WorkflowLookup key={directory.epoch} directory={directory} />
          ) : selected === "git" ? (
            <section className="panel">
              <h2>Git</h2>
              <h3>仓库 HEAD</h3>
              <CoreQuery handler="query.core.git.head" run={rpc} />
              <h3>比较修订</h3>
              <CoreQuery handler="query.core.git.diff" run={rpc} />
            </section>
          ) : (
            <section className="panel">
              <h2>领域管理</h2>
              <p role="status">
                当前激活的领域：{directory.active?.id ?? "空领域"}
              </p>
              <form
                onSubmit={(event) => {
                  event.preventDefault();
                  const value = new FormData(event.currentTarget).get(
                    "archive",
                  );
                  if (value instanceof File && value.size)
                    void mutate(async () => {
                      await rpc("domain.install", {
                        archive: await archive(value),
                      });
                    });
                }}
              >
                <label>
                  领域 ZIP
                  <input type="file" name="archive" accept=".zip" required />
                </label>
                <button type="submit" disabled={busy || !directory}>
                  导入
                </button>
              </form>
              <ul>
                {packages.map((item) => (
                  <li key={item.digest}>
                    {item.id} {item.version}{" "}
                    <button
                      disabled={
                        busy ||
                        !directory ||
                        item.digest === directory.active?.digest
                      }
                      onClick={() =>
                        void mutate(
                          () =>
                            rpc("domain.activate", {
                              digest: item.digest,
                              epoch: directory?.epoch,
                            }),
                          true,
                        )
                      }
                    >
                      激活
                    </button>{" "}
                    <button
                      disabled={busy || !directory}
                      onClick={() =>
                        void mutate(() =>
                          rpc("domain.uninstall", {
                            digest: item.digest,
                            epoch: directory?.epoch,
                          }),
                        )
                      }
                    >
                      卸载
                    </button>
                  </li>
                ))}
              </ul>
            </section>
          )}
        </main>
      )}
    </WorkbenchLayout>
  );
}

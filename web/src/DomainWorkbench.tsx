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
import { ActionButton } from "./components/ActionFeedback";
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
type CoreRecord = { id: string; name: string; type: string; data: Data };

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

function CoreConfiguration() {
  const [records, setRecords] = useState<CoreRecord[]>([]);
  const [output, setOutput] = useState("");
  const [busy, setBusy] = useState(false);
  const refresh = useCallback(async () => {
    const summary = await rpc<{ records: { id: string }[] }>(
      "query.configuration_index",
      { limit: 64 },
    );
    const response = await rpc<{ records: CoreRecord[] }>(
      "query.configuration_records",
      {
        refs: summary.records.map((row) => row.id),
      },
    );
    setRecords(response.records);
  }, []);
  useEffect(() => {
    void refresh().catch((error: unknown) => setOutput(message(error)));
  }, [refresh]);
  async function apply(method: string, params: Data) {
    setBusy(true);
    setOutput("");
    try {
      const value = await rpc(method, params);
      setOutput(JSON.stringify(value, null, 2));
      await refresh();
    } catch (error) {
      setOutput(message(error));
    } finally {
      setBusy(false);
    }
  }
  return (
    <section className="panel">
      <h2>核心配置</h2>
      {[
        "model",
        "model_provider",
        "harness",
        "sandbox",
        "permission_profile",
        "tool",
      ].map((kind) => (
        <section key={kind}>
          <h3>{kind}</h3>
          <ul>
            {records
              .filter((row) => row.type === kind)
              .map((row) => (
                <li key={row.id}>
                  {row.name} <code>{row.id}</code>{" "}
                  {kind === "harness" && (
                    <button
                      disabled={busy}
                      onClick={() =>
                        void apply("runtime.prepare_harness", { id: row.id })
                      }
                    >
                      准备运行环境
                    </button>
                  )}
                </li>
              ))}
          </ul>
        </section>
      ))}
      <form
        onSubmit={(event) => {
          event.preventDefault();
          const fields = new FormData(event.currentTarget);
          void apply("model.create", {
            command_id: crypto.randomUUID(),
            payload: {
              name: fields.get("name"),
              scope: "global",
              data: {
                provider: fields.get("provider"),
                model: fields.get("model"),
                parameters: {},
                capabilities: ["structured-output", "tool-calling"],
              },
            },
          });
        }}
      >
        <h3>添加模型</h3>
        <label>
          显示名称
          <input name="name" required />
        </label>
        <label>
          供应商
          <select name="provider">
            {records
              .filter((row) => row.type === "model_provider")
              .map((row) => (
                <option key={row.id} value={String(row.data.id)}>
                  {row.name}
                </option>
              ))}
          </select>
        </label>
        <label>
          模型标识
          <input name="model" required />
        </label>
        <button type="submit" disabled={busy}>
          保存模型
        </button>
      </form>
      {output && <pre role="status">{output}</pre>}
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

export function AppRoot() {
  const [directory, setDirectory] = useState<Directory>();
  const [packages, setPackages] = useState<Package[]>([]);
  const [selected, setSelected] = useState(
    () => new URLSearchParams(location.search).get("view") ?? "packages",
  );
  const [error, setError] = useState("");
  const [auth, setAuth] = useState(false);
  const [busy, setBusy] = useState(false);
  const refresh = useCallback(async () => {
    const [next, installed] = await Promise.all([
      rpc<Directory>("query.domain.active"),
      rpc<Package[]>("query.domain.installed"),
    ]);
    setDirectory((previous) =>
      previous && previous.epoch > next.epoch ? previous : next,
    );
    setPackages(installed);
  }, []);
  useEffect(() => {
    const events = new EventSource("/domain-events");
    const reload = () =>
      void refresh().catch((failure: unknown) => {
        if (message(failure) === "HTTP 401") setAuth(true);
        else setError(message(failure));
      });
    events.addEventListener("domain", reload);
    reload();
    const pop = () =>
      setSelected(
        new URLSearchParams(location.search).get("view") ?? "packages",
      );
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
  async function mutate(action: () => Promise<unknown>) {
    setError("");
    setBusy(true);
    try {
      await action();
      await refresh();
    } catch (failure) {
      setError(message(failure));
    } finally {
      setBusy(false);
    }
  }
  if (auth)
    return (
      <KernelAuthentication
        authenticated={async () => {
          await refresh();
          setAuth(false);
        }}
      />
    );
  const contribution = directory?.contributions?.find(
    (item) => item.id === selected,
  );
  return (
    <WorkbenchLayout
      navigation={
        <nav aria-label="工作台导航">
          <ActionButton className="nav" onClick={() => select("packages")}>
            领域包
          </ActionButton>
          <ActionButton className="nav" onClick={() => select("core")}>
            核心配置
          </ActionButton>
          <ActionButton className="nav" onClick={() => select("git")}>
            Git
          </ActionButton>
          {directory?.active && (
            <ActionButton className="nav" onClick={() => select("workflow")}>
              领域工作流
            </ActionButton>
          )}
          {directory?.contributions?.map((item) => (
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
          {selected === "workflow" && directory?.active ? (
            <WorkflowLookup key={directory.epoch} directory={directory} />
          ) : selected === "core" ? (
            <CoreConfiguration />
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
              <h2>领域包</h2>
              <p role="status">
                {directory
                  ? (directory.active?.id ?? "空领域")
                  : "正在连接工作台…"}
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
                        void mutate(() =>
                          rpc("domain.activate", {
                            digest: item.digest,
                            epoch: directory?.epoch,
                          }),
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
              {directory?.sources?.length ? (
                <ul>
                  {directory.sources.map((source) => (
                    <li key={source.id}>
                      <details>
                        <summary>
                          {source.name} <code>{source.id}</code>
                        </summary>
                        <pre>{JSON.stringify(source.data, null, 2)}</pre>
                      </details>
                    </li>
                  ))}
                </ul>
              ) : null}
            </section>
          )}
        </main>
      )}
    </WorkbenchLayout>
  );
}

import {
  ActionButton,
  ActionForm,
  ActionSelect,
  ActionInput,
  useActionFeedback,
} from "./ActionFeedback";
import { useCallback, useEffect, useRef, useState } from "react";
import { ResourceEditor } from "./AgentCatalog";
import {
  latest,
  loadCatalog,
  MarkdownField,
  MarkdownPreview,
  RevisionSelect,
  useDraft,
  useGroupTab,
  type Request,
  type Row,
} from "./configuration-ui";
const tabs = ["http", "mcp", "skills", "native", "calls"];
const names: Record<string, string> = {
  http: "HTTP API",
  mcp: "MCP",
  skills: "Skills",
  native: "原生工具",
  calls: "调用记录",
};
const originOf = (value: string) => {
  try {
    return new URL(value).origin;
  } catch {
    return "";
  }
};
const lines = (value: string) =>
  value
    .split(/\r?\n/)
    .map((s) => s.trim())
    .filter(Boolean);
const command = (
  request: Request,
  method: string,
  project: Row | undefined,
  payload: Row,
  target?: Row,
) =>
  request(method, {
    local: !!project,
    project_id: project?.id,
    command_id: crypto.randomUUID(),
    target: target?.id,
    expected_revision: target?.revision,
    payload,
  });
async function records(request: Request, method: string, project?: Row) {
  const rows: Row[] = [];
  let offset = 0;
  for (;;) {
    const page = await request(method, {
      local: !!project,
      project_id: project?.id,
      offset,
      limit: 100,
      include_history: true,
    });
    rows.push(...page.records);
    offset += page.records.length;
    if (offset >= page.total || !page.records.length) return rows;
  }
}
function JsonField({
  label,
  value,
  onChange,
  optional = false,
}: {
  label: string;
  value: string;
  onChange: (s: string) => void;
  optional?: boolean;
}) {
  return (
    <label>
      {label}
      <textarea
        aria-label={label}
        required={!optional}
        rows={6}
        spellCheck={false}
        value={value}
        onChange={(e) => onChange(e.target.value)}
      />
    </label>
  );
}
export function Integrations({
  request,
  project,
  installations,
  kind,
  contentOnly = false,
}: {
  request: Request;
  project?: Row;
  installations: Row[];
  kind?: string;
  contentOnly?: boolean;
}) {
  const [savedTab, setTab] = useGroupTab("integrations", tabs);
  const tab = kind || savedTab;
  const [skillRevision, setSkillRevision] = useState(0);
  return (
    <>
      {!contentOnly && (
        <section className="panel">
          <div className="panel-heading">
            <h2>工具与技能接入</h2>
            <p>连接服务并发布具体动作，在 Capability 中选择工具与技能。</p>
          </div>
          <div
            className="configuration-tabs"
            role="tablist"
            aria-label="接入类别"
          >
            {tabs.map((id) => (
              <ActionButton
                key={id}
                role="tab"
                aria-selected={tab === id}
                onClick={() => setTab(id)}
              >
                {names[id]}
              </ActionButton>
            ))}
          </div>
        </section>
      )}
      {["http", "mcp"].includes(tab) ? (
        <SourceManager
          key={`${tab}:${project?.id}`}
          kind={tab}
          request={request}
          project={project}
        />
      ) : tab === "skills" ? (
        <>
          <SkillImport
            request={request}
            project={project}
            onImported={() => setSkillRevision((v) => v + 1)}
          />
          <ResourceEditor
            key={`skills:${project?.id}`}
            request={request}
            project={project}
            installations={installations}
            kind="skill"
            revision={skillRevision}
          />
        </>
      ) : tab === "native" ? (
        <NativeCatalog request={request} project={project} />
      ) : (
        <CallHistory
          key={`calls:${project?.id}`}
          request={request}
          project={project}
        />
      )}
    </>
  );
}
function NativeCatalog({
  request,
  project,
}: {
  request: Request;
  project?: Row;
}) {
  const { error, setError } = useActionFeedback();

  const [rows, setRows] = useState<Row[]>([]);
  useEffect(() => {
    let active = true;
    void loadCatalog(request, project)
      .then((rows) => {
        if (active) setRows(rows);
      })
      .catch((e) => {
        if (active) setError(String(e));
      });
    return () => {
      active = false;
    };
  }, [request, project?.id]);
  return (
    <section className="panel" aria-label="原生工具目录">
      <div className="panel-heading">
        <h2>Harness 原生工具</h2>
      </div>
      <div className="integration-content">
        <p role="note" aria-label="原生工具组说明">
          原生工具随 Harness 提供，无需手工配置输入或输出
          Schema。可访问范围以实际生效的 Permission 为准。
        </p>
        {latest(
          rows.filter(
            (row) =>
              row.kind === "tool" &&
              row.data.implementation === "harness.native",
          ),
        ).flatMap((row) => {
          const harnesses = latest(
            rows.filter(
              (harness) =>
                harness.kind === "harness" &&
                harness.data.provider ===
                  (row.data.harness_provider || "codex"),
            ),
          );
          return (harnesses.length ? harnesses : [undefined]).map((harness) => (
            <a
              className="native-tool-group"
              key={`${row.id}:${harness?.id || "unconfigured"}`}
              href={`?tab=components&config_tab=harness${harness ? `&config_object=${encodeURIComponent(harness.id)}` : ""}`}
            >
              <article>
                <h3>{row.name}</h3>
                <p>
                  v{row.version} · {row.scope.kind}
                  {harness ? ` · ${harness.name}` : ""}
                </p>
                <p>
                  适用 Harness 版本：
                  <code>{harness?.data.runtime_version || "尚未定义"}</code>
                </p>
              </article>
            </a>
          ));
        })}
        {error && <p role="alert">{error}</p>}
      </div>
    </section>
  );
}
function SourceManager(props: {
  kind: string;
  request: Request;
  project?: Row;
}) {
  const [revision, setRevision] = useState(0);
  const [selectId, setSelectId] = useState<string>();
  const changed = (id?: string) => {
    setSelectId(id);
    setRevision((v) => v + 1);
  };
  return (
    <>
      <SourceEditor
        {...props}
        mode="create"
        revision={revision}
        selectId={selectId}
        onChanged={changed}
      />
      <SourceEditor
        {...props}
        mode="manage"
        revision={revision}
        selectId={selectId}
        onChanged={changed}
      />
    </>
  );
}
function SourceEditor({
  kind,
  request,
  project,
  mode,
  revision,
  onChanged,
  selectId,
}: {
  mode: "create" | "manage";
  revision: number;
  onChanged: (id?: string) => void;
  selectId?: string;
  kind: string;
  request: Request;
  project?: Row;
}) {
  const {
    error,
    setError,
    message,
    setMessage,
    capture: captureFeedback,
    beginFeedback,
    endFeedback,
  } = useActionFeedback();

  const fresh = () => ({
    selected: "",
    name: "",
    scope: "global",
    description: "",
    base_url: "",
    transport: "streamable_http",
    url: "",
    protocol_version: "2026-07-28",
    command: '["node", "/work/server.mjs"]',
    oauth_enabled: false,
    client_id: "",
    authorization_url: "",
    token_url: "",
    revocation_url: "",
    oauth_scopes: "",
  });
  const [draft, setDraft] = useDraft(
    `devdesk:integrations:${project?.id || "global"}:${kind}:${mode}`,
    fresh,
  );
  const [sources, setSources] = useState<Row[]>([]),
    [catalog, setCatalog] = useState<Row[]>([]),
    [busy, setBusy] = useState(false);
  const load = useCallback(async () => {
    const [source, catalog] = await Promise.all([
      records(request, "query.integrations", project),
      loadCatalog(request, project),
    ]);
    setSources(source);
    setCatalog(catalog);
  }, [request, project?.id]);
  useEffect(() => {
    void load().catch((e) => setError(String(e)));
  }, [load, revision]);
  const [deletedSource, setDeletedSource] = useState<Row>();
  const selected =
    mode === "manage"
      ? sources.find((r) => r.id === draft.selected) || deletedSource
      : undefined;
  const choose = (r: Row) => {
    setDeletedSource(undefined);
    const s = r.data.source;
    setDraft({
      selected: r.id,
      name: r.name,
      scope: r.scope.kind,
      description: r.data.description || "",
      base_url: s.base_url || "",
      transport: s.transport || "streamable_http",
      url: s.url || "",
      protocol_version: s.protocol_version || "2026-07-28",
      command: JSON.stringify(s.command || [], null, 2),
      oauth_enabled: !!r.data.oauth,
      client_id: r.data.oauth?.client_id || "",
      authorization_url: r.data.oauth?.authorization_url || "",
      token_url: r.data.oauth?.token_url || "",
      revocation_url: r.data.oauth?.revocation_url || "",
      oauth_scopes: (r.data.oauth?.scopes || []).join("\n"),
    });
  };
  const consumedSelection = useRef<string | undefined>(undefined);
  useEffect(() => {
    if (
      mode !== "manage" ||
      !selectId ||
      consumedSelection.current === selectId
    )
      return;
    const row = sources.find((r) => r.id === selectId);
    if (row) {
      consumedSelection.current = selectId;
      choose(row);
    }
  }, [mode, sources, selectId]);
  const routeConsumed = useRef(false);
  useEffect(() => {
    if (mode !== "manage" || routeConsumed.current || !sources.length) return;
    const target = new URLSearchParams(location.search).get("config_object");
    const tool = catalog.find((r) => r.id === target && r.kind === "tool");
    const row = sources.find(
      (r) =>
        r.kind === "integration" &&
        (r.id === target || r.id === tool?.data.integration_ref),
    );
    if (row) {
      routeConsumed.current = true;
      choose(row);
    }
  }, [sources, catalog]);
  async function run(fn: () => Promise<void>) {
    captureFeedback();

    (beginFeedback(), setBusy(true));
    setError("");
    try {
      await fn();
    } catch (e) {
      setError(String(e));
    } finally {
      (endFeedback(), setBusy(false));
    }
  }
  const rows = latest(
    sources.filter(
      (r) =>
        r.kind === "integration" &&
        r.data.source.kind === (kind === "http" ? "http_api" : "mcp"),
    ),
  );
  useEffect(() => {
    if (mode === "manage" && !draft.selected && !deletedSource && rows[0])
      choose(rows[0]);
  }, [mode, draft.selected, sources]);
  return (
    <>
      <section
        className="panel"
        aria-label={`${names[kind]} ${mode === "create" ? "新建接入" : "接入管理"}`}
      >
        <div className="panel-heading">
          <h2>
            {mode === "create"
              ? `新建 ${names[kind]} 接入源`
              : `管理 ${names[kind]} 接入源`}
          </h2>
        </div>
        <div
          className={
            mode === "manage" ? "configuration-layout" : "integration-content"
          }
        >
          {mode === "manage" && (
            <div className="configuration-list">
              {rows.map((r) => (
                <ActionButton
                  key={r.id}
                  className={r.id === selected?.id ? "selected" : ""}
                  onClick={() => {
                    captureFeedback();
                    return choose(r);
                  }}
                >
                  <strong>{r.name}</strong>
                  <span>
                    v{r.version} · {r.scope.kind}
                  </span>
                </ActionButton>
              ))}
            </div>
          )}
          {mode === "create" || selected ? (
            <ActionForm
              onSubmit={(e) => {
                captureFeedback();

                e.preventDefault();
                void run(async () => {
                  const source =
                    kind === "http"
                      ? { kind: "http_api", base_url: draft.base_url }
                      : {
                          kind: "mcp",
                          transport: draft.transport,
                          protocol_version: draft.protocol_version,
                          url:
                            draft.transport === "streamable_http"
                              ? draft.url
                              : null,
                          command:
                            draft.transport === "stdio"
                              ? JSON.parse(draft.command)
                              : [],
                        };
                  const r = await command(
                    request,
                    `integration.${selected ? "revise" : "create"}`,
                    project,
                    {
                      name: draft.name,
                      scope:
                        draft.scope === "project"
                          ? { kind: "project", project_id: project!.id }
                          : { kind: "global" },
                      data: {
                        source,
                        description: draft.description,
                        ...(draft.oauth_enabled
                          ? {
                              oauth: {
                                client_id: draft.client_id,
                                authorization_url: draft.authorization_url,
                                token_url: draft.token_url,
                                revocation_url: draft.revocation_url || null,
                                scopes: lines(draft.oauth_scopes),
                              },
                            }
                          : {}),
                      },
                    },
                    selected,
                  );
                  await load();
                  if (mode === "manage") choose(r);
                  else setDraft(fresh());
                  onChanged(r.id);
                  setMessage("接入定义已保存；请配置绑定并发布具体工具。");
                });
              }}
            >
              <fieldset
                disabled={busy || !!deletedSource}
                className="integration-fields"
              >
                <label>
                  接入名称
                  <input
                    required
                    value={draft.name}
                    onChange={(e) => {
                      captureFeedback();
                      return setDraft({ ...draft, name: e.target.value });
                    }}
                  />
                </label>
                <label>
                  接入范围
                  <ActionSelect
                    disabled={!!selected}
                    value={draft.scope}
                    onChange={(e) => {
                      captureFeedback();
                      return setDraft({ ...draft, scope: e.target.value });
                    }}
                  >
                    <option value="global">Global</option>
                    {project && <option value="project">当前项目</option>}
                  </ActionSelect>
                </label>
                {kind === "http" ? (
                  <label>
                    API Base URL
                    <input
                      type="url"
                      required
                      value={draft.base_url}
                      onChange={(e) => {
                        captureFeedback();
                        return setDraft({ ...draft, base_url: e.target.value });
                      }}
                      placeholder="https://api.example.com/v1"
                    />
                  </label>
                ) : (
                  <>
                    <label>
                      MCP 传输
                      <ActionSelect
                        value={draft.transport}
                        onChange={(e) => {
                          captureFeedback();
                          return setDraft({
                            ...draft,
                            transport: e.target.value,
                          });
                        }}
                      >
                        <option value="streamable_http">Streamable HTTP</option>
                        <option value="stdio">stdio · Docker 中执行</option>
                      </ActionSelect>
                    </label>
                    <label>
                      MCP 协议版本
                      <ActionSelect
                        value={draft.protocol_version}
                        onChange={(e) => {
                          captureFeedback();
                          return setDraft({
                            ...draft,
                            protocol_version: e.target.value,
                          });
                        }}
                      >
                        <option>2026-07-28</option>
                        <option>2025-11-25</option>
                      </ActionSelect>
                    </label>
                    {draft.transport === "streamable_http" ? (
                      <label>
                        MCP URL
                        <input
                          type="url"
                          required
                          value={draft.url}
                          onChange={(e) => {
                            captureFeedback();
                            return setDraft({ ...draft, url: e.target.value });
                          }}
                        />
                      </label>
                    ) : (
                      <JsonField
                        label="MCP 启动参数（JSON 数组）"
                        value={draft.command}
                        onChange={(command) => {
                          captureFeedback();
                          return setDraft({ ...draft, command });
                        }}
                      />
                    )}
                  </>
                )}
                <label className="inline-control">
                  <input
                    type="checkbox"
                    checked={draft.oauth_enabled || false}
                    onChange={(e) => {
                      captureFeedback();
                      return setDraft({
                        ...draft,
                        oauth_enabled: e.target.checked,
                      });
                    }}
                  />
                  启用 OAuth 授权码 + PKCE
                </label>
                {draft.oauth_enabled && (
                  <fieldset>
                    <legend>OAuth 公共客户端</legend>
                    <label>
                      OAuth Client ID
                      <input
                        required
                        value={draft.client_id}
                        onChange={(e) => {
                          captureFeedback();
                          return setDraft({
                            ...draft,
                            client_id: e.target.value,
                          });
                        }}
                      />
                    </label>
                    <label>
                      授权端点
                      <input
                        type="url"
                        required
                        value={draft.authorization_url}
                        onChange={(e) => {
                          captureFeedback();
                          return setDraft({
                            ...draft,
                            authorization_url: e.target.value,
                          });
                        }}
                      />
                    </label>
                    <label>
                      Token 端点
                      <input
                        type="url"
                        required
                        value={draft.token_url}
                        onChange={(e) => {
                          captureFeedback();
                          return setDraft({
                            ...draft,
                            token_url: e.target.value,
                          });
                        }}
                      />
                    </label>
                    <label>
                      撤销端点（可选）
                      <input
                        type="url"
                        value={draft.revocation_url}
                        onChange={(e) => {
                          captureFeedback();
                          return setDraft({
                            ...draft,
                            revocation_url: e.target.value,
                          });
                        }}
                      />
                    </label>
                    <label>
                      可授权 scopes（每行一个）
                      <textarea
                        value={draft.oauth_scopes}
                        onChange={(e) => {
                          captureFeedback();
                          return setDraft({
                            ...draft,
                            oauth_scopes: e.target.value,
                          });
                        }}
                      />
                    </label>
                    <p>
                      在服务端登记回调地址：{window.location.origin}
                      /oauth/integrations/callback
                    </p>
                  </fieldset>
                )}
                <MarkdownField
                  key={draft.selected || "new"}
                  label="接入说明"
                  value={draft.description}
                  onChange={(description) => {
                    captureFeedback();
                    return setDraft({ ...draft, description });
                  }}
                />
                <div className="actions">
                  <ActionButton className="primary" disabled={busy}>
                    {selected ? "保存接入新修订" : "创建接入源"}
                  </ActionButton>
                  {selected && (
                    <>
                      <ActionButton
                        type="button"
                        disabled={busy}
                        onClick={() => {
                          captureFeedback();
                          return setDraft({
                            ...draft,
                            selected: "",
                            name: `${draft.name} · 副本`,
                          });
                        }}
                      >
                        复制接入源
                      </ActionButton>
                      <ActionButton
                        type="button"
                        disabled={busy}
                        onClick={() => {
                          captureFeedback();
                          return void run(async () => {
                            await command(
                              request,
                              "integration.delete",
                              project,
                              { reason: "用户删除接入源及所属动作和绑定" },
                              selected,
                            );
                            setDeletedSource(selected);
                            await load();
                            onChanged();
                            setMessage(
                              "接入源及所属动作、绑定已删除，历史记录保留。",
                            );
                          });
                        }}
                      >
                        删除接入源
                      </ActionButton>
                      <ActionButton
                        type="button"
                        disabled={busy}
                        onClick={() => {
                          captureFeedback();
                          return void run(async () => {
                            await command(
                              request,
                              "integration.disable",
                              project,
                              { reason: "用户紧急停用" },
                              selected,
                            );
                            setMessage("已紧急停用，将阻止后续工具派发。");
                          });
                        }}
                      >
                        紧急停用
                      </ActionButton>
                    </>
                  )}
                </div>
                {selected && !deletedSource && (
                  <label>
                    接入历史修订
                    <ActionSelect
                      value={selected.id}
                      onChange={(e) => {
                        captureFeedback();

                        const row = sources.find(
                          (r) => r.id === e.target.value,
                        );
                        if (row) choose(row);
                      }}
                    >
                      {sources
                        .filter(
                          (r) =>
                            r.kind === "integration" &&
                            r.definition_id === selected.definition_id,
                        )
                        .map((r) => (
                          <option key={r.id} value={r.id}>
                            v{r.version} · {r.name}
                          </option>
                        ))}
                    </ActionSelect>
                  </label>
                )}
              </fieldset>
            </ActionForm>
          ) : (
            <p className="muted">暂无接入源，请先在上方创建。</p>
          )}
        </div>
        {error && <p role="alert">{error}</p>}
        {message && <p role="status">{message}</p>}
      </section>
      {selected && !deletedSource && (
        <>
          <BindingManager
            key={selected.id}
            source={selected}
            request={request}
            project={project}
            catalog={catalog}
            sources={sources}
            reload={load}
          />
          <PublishedTools
            source={selected}
            request={request}
            project={project}
            catalog={catalog}
            reload={load}
          />
          {kind === "http" && (
            <HttpToolEditor
              key={`tools:${selected.id}`}
              source={selected}
              request={request}
              project={project}
              catalog={catalog}
              reload={load}
            />
          )}
        </>
      )}
    </>
  );
}
function PublishedTools({
  source,
  request,
  project,
  catalog,
  reload,
}: {
  source: Row;
  request: Request;
  project?: Row;
  catalog: Row[];
  reload: () => Promise<void>;
}) {
  const { error, setError, beginFeedback, endFeedback, setMessage } =
    useActionFeedback();
  const [removed, setRemoved] = useState<Row[]>([]);
  const [busy, setBusy] = useState(false);
  const tools = latest(
    catalog.filter(
      (r) => r.kind === "tool" && r.data.integration_ref === source.id,
    ),
  );
  return (
    <section className="panel" aria-label="已发布工具管理">
      <div className="panel-heading">
        <h2>已发布工具</h2>
      </div>
      <div className="integration-content">
        {!tools.length && !removed.length && (
          <p className="muted">尚未发布工具。</p>
        )}
        {[
          ...tools,
          ...removed.filter((r) => !tools.some((t) => t.id === r.id)),
        ].map((tool) => (
          <article key={tool.id}>
            <h3>
              {tool.name} · v{tool.version}
            </h3>
            <p>
              <code>{tool.data.qualified_name}</code>
            </p>
            <div className="actions">
              <ActionButton
                disabled={busy || removed.some((r) => r.id === tool.id)}
                onClick={async () => {
                  beginFeedback();
                  setBusy(true);
                  setError("");
                  try {
                    await command(
                      request,
                      "integration.delete",
                      project,
                      {},
                      tool,
                    );
                    setRemoved((rows) => [...rows, tool]);
                    await reload();
                    setMessage("工具已删除，历史调用和修订保留。");
                  } catch (e) {
                    setError(String(e));
                  } finally {
                    endFeedback();
                    setBusy(false);
                  }
                }}
              >
                删除工具 · {tool.name}
              </ActionButton>
            </div>
          </article>
        ))}
        {error && <p role="alert">{error}</p>}
      </div>
    </section>
  );
}
function BindingManager({
  source,
  request,
  project,
  catalog,
  sources,
  reload,
}: {
  source: Row;
  request: Request;
  project?: Row;
  catalog: Row[];
  sources: Row[];
  reload: () => Promise<void>;
}) {
  const {
    error,
    setError,
    message,
    setMessage,
    capture: captureFeedback,
    beginFeedback,
    endFeedback,
  } = useActionFeedback();

  const fresh = () => ({
    selected: "",
    name: "",
    scope: project ? "project" : "global",
    harness_ref: "builtin-harness-codex-managed-v1",
    sandbox_ref: "builtin-permission-sandbox-docker-v1",
    auth_kind: "none",
    credential_ref: "",
    header: "X-API-Key",
    account: "",
    scopes: "",
    origins:
      source.data.source.base_url || source.data.source.url
        ? originOf(source.data.source.base_url || source.data.source.url)
        : "",
    private_network: false,
    allowed_tools: [] as string[],
    timeout: 30000,
    limit: 1048576,
  });
  const [draft, setDraft] = useDraft(
    `devdesk:integration-binding:${project?.id || "global"}:${source.id}`,
    fresh,
  );
  const [secret, setSecret] = useState(""),
    [busy, setBusy] = useState(false),
    [permissions, setPermissions] = useState<Row>({ targets: [] }),
    [selectedNames, setSelectedNames] = useState<string[]>([]),
    [effects, setEffects] = useState<Record<string, string>>({}),
    [discovery, setDiscovery] = useState<Row>(),
    [oauthState, setOauthState] = useState(""),
    [oauthUrl, setOauthUrl] = useState("");
  useEffect(() => {
    void request("query.permission_resources")
      .then(setPermissions)
      .catch((e) => setError(String(e)));
  }, [request]);
  const bindings = latest(
    sources.filter(
      (r) =>
        r.kind === "integration_binding" &&
        r.data.integration_ref === source.id,
    ),
  );
  const [deletedBinding, setDeletedBinding] = useState<Row>();
  const selected =
    sources.find((r) => r.id === draft.selected) || deletedBinding;
  const tools = latest(
    catalog.filter(
      (r) => r.kind === "tool" && r.data.integration_ref === source.id,
    ),
  );
  const choose = (r: Row) => {
    setDeletedBinding(undefined);
    const b = r.data;
    setDraft({
      selected: r.id,
      name: r.name,
      scope: r.scope.kind,
      harness_ref: b.harness_ref,
      sandbox_ref: b.sandbox_ref,
      auth_kind:
        b.auth?.kind === "oauth_pending" || b.auth?.kind === "oauth"
          ? "oauth"
          : b.auth?.kind || "none",
      credential_ref: b.auth?.credential_ref || "",
      header: b.auth?.header || "X-API-Key",
      account: b.auth?.account || "",
      scopes: (b.auth?.scopes || []).join("\n"),
      origins: (b.allowed_origins || []).join("\n"),
      private_network: b.allow_private_network,
      allowed_tools: b.allowed_tools,
      timeout: b.timeout_ms,
      limit: b.max_result_bytes,
    });
    setDiscovery(undefined);
    setSelectedNames([]);
  };
  async function run(fn: () => Promise<void>) {
    captureFeedback();

    (beginFeedback(), setBusy(true));
    setError("");
    try {
      await fn();
    } catch (e) {
      setError(String(e));
    } finally {
      (endFeedback(), setBusy(false));
    }
  }
  return (
    <section className="panel" aria-label="接入绑定管理">
      <div className="panel-heading">
        <h2>账户、权限与允许工具</h2>
        <ActionButton
          onClick={() => {
            captureFeedback();

            setDraft(fresh());
            setSecret("");
            setMessage("");
          }}
        >
          新建接入绑定
        </ActionButton>
      </div>
      <div className="configuration-layout">
        <div className="configuration-list">
          {bindings.map((r) => (
            <ActionButton
              key={r.id}
              onClick={() => {
                captureFeedback();
                return choose(r);
              }}
            >
              {r.name} · v{r.version}
            </ActionButton>
          ))}
        </div>
        <ActionForm
          onSubmit={(e) => {
            captureFeedback();

            e.preventDefault();
            void run(async () => {
              const auth =
                draft.auth_kind === "none"
                  ? { kind: "none" }
                  : draft.auth_kind === "api_key"
                    ? {
                        kind: "api_key",
                        credential_ref: draft.credential_ref,
                        header: draft.header,
                      }
                    : draft.auth_kind === "oauth"
                      ? {
                          kind: draft.credential_ref
                            ? "oauth"
                            : "oauth_pending",
                          ...(draft.credential_ref
                            ? { credential_ref: draft.credential_ref }
                            : {}),
                          account: draft.account,
                          scopes: lines(draft.scopes),
                        }
                      : {
                          kind: "bearer",
                          credential_ref: draft.credential_ref,
                        };
              const row = await command(
                request,
                `integration.binding.${selected ? "revise" : "create"}`,
                project,
                {
                  name: draft.name,
                  scope:
                    draft.scope === "project"
                      ? { kind: "project", project_id: project!.id }
                      : { kind: "global" },
                  data: {
                    integration_ref: source.id,
                    auth,
                    harness_ref: draft.harness_ref,
                    sandbox_ref: draft.sandbox_ref,
                    allowed_origins: lines(draft.origins),
                    allow_private_network: draft.private_network,
                    allowed_tools: draft.allowed_tools,
                    timeout_ms: Number(draft.timeout),
                    max_result_bytes: Number(draft.limit),
                  },
                },
                selected,
              );
              await reload();
              choose(row);
              setMessage(
                "绑定已保存；需要在项目中明确采用，并在 Capability 选择工具。",
              );
            });
          }}
        >
          <fieldset disabled={!!deletedBinding} className="integration-fields">
            <label>
              绑定名称
              <input
                required
                value={draft.name}
                onChange={(e) => {
                  captureFeedback();
                  return setDraft({ ...draft, name: e.target.value });
                }}
              />
            </label>
            <label>
              绑定范围
              <ActionSelect
                value={draft.scope || "global"}
                disabled={!!selected}
                onChange={(e) => {
                  captureFeedback();
                  return setDraft({ ...draft, scope: e.target.value });
                }}
              >
                <option value="global">Global</option>
                {project && <option value="project">当前项目</option>}
              </ActionSelect>
            </label>
            <RevisionSelect
              label="接入 Harness"
              records={catalog.filter((r) => r.kind === "harness")}
              value={draft.harness_ref}
              onChange={(harness_ref) => {
                captureFeedback();
                return setDraft({ ...draft, harness_ref });
              }}
              required
            />
            <RevisionSelect
              label="接入 Sandbox"
              records={
                permissions.targets
                  ?.filter((r: Row) => r.target.kind === "sandbox")
                  .map((r: Row) => ({
                    id: r.target.revision_id,
                    name: r.name,
                    version: r.version,
                    definition_id: r.target.revision_id,
                    scope: { kind: "global" },
                  })) || []
              }
              value={draft.sandbox_ref}
              onChange={(sandbox_ref) => {
                captureFeedback();
                return setDraft({ ...draft, sandbox_ref });
              }}
              required
            />
            <label>
              认证方式
              <ActionSelect
                value={draft.auth_kind}
                onChange={(e) => {
                  captureFeedback();
                  return setDraft({ ...draft, auth_kind: e.target.value });
                }}
              >
                <option value="none">无需认证</option>
                <option value="bearer">Bearer token</option>
                <option value="api_key">API key · Header</option>
                <option value="oauth">OAuth 2.0</option>
              </ActionSelect>
            </label>
            {["bearer", "api_key"].includes(draft.auth_kind) && (
              <>
                <label>
                  系统凭据引用
                  <input
                    required
                    value={draft.credential_ref}
                    onChange={(e) => {
                      captureFeedback();
                      return setDraft({
                        ...draft,
                        credential_ref: e.target.value,
                      });
                    }}
                    placeholder="integration-secret:…"
                  />
                </label>
                <label>
                  新凭据
                  <input
                    type="password"
                    autoComplete="new-password"
                    value={secret}
                    onChange={(e) => {
                      captureFeedback();
                      return setSecret(e.target.value);
                    }}
                  />
                </label>
                <ActionButton
                  type="button"
                  disabled={busy || !secret}
                  onClick={() => {
                    captureFeedback();
                    return void run(async () => {
                      const result = await command(
                        request,
                        "credentials.integration.configure",
                        project,
                        { secret },
                      );
                      setDraft({ ...draft, credential_ref: result.reference });
                      setSecret("");
                      setMessage("凭据已写入系统存储，页面仅保留引用。");
                    });
                  }}
                >
                  保存接入凭据
                </ActionButton>
                {draft.credential_ref && (
                  <ActionButton
                    type="button"
                    disabled={busy || !secret}
                    onClick={() => {
                      captureFeedback();
                      return void run(async () => {
                        await command(
                          request,
                          "credentials.integration.configure",
                          project,
                          { reference: draft.credential_ref, secret },
                        );
                        setSecret("");
                        setMessage("同一账户凭据已轮换，原引用继续有效。");
                      });
                    }}
                  >
                    轮换同一账户凭据
                  </ActionButton>
                )}
                <ActionButton
                  type="button"
                  disabled={busy || !draft.credential_ref}
                  onClick={() => {
                    captureFeedback();
                    return void run(async () => {
                      const r = await command(
                        request,
                        "credentials.integration.status",
                        project,
                        { reference: draft.credential_ref },
                      );
                      setMessage(
                        r.available
                          ? "凭据可用。"
                          : "凭据不可用，请检查系统存储。",
                      );
                    });
                  }}
                >
                  检查凭据
                </ActionButton>
              </>
            )}
            {draft.auth_kind === "api_key" && (
              <label>
                API key Header
                <input
                  required
                  value={draft.header}
                  onChange={(e) => {
                    captureFeedback();
                    return setDraft({ ...draft, header: e.target.value });
                  }}
                />
              </label>
            )}
            {draft.auth_kind === "oauth" && (
              <>
                {!source.data.oauth && (
                  <p role="alert">请先在接入源配置 OAuth 公共客户端。</p>
                )}
                <p>
                  先保存待授权绑定，再发起授权。授权完成后保存新修订并由项目采用。
                </p>
                <p>
                  授权状态：
                  {draft.credential_ref ? "已取得凭据引用" : "等待授权"}
                </p>
                <label>
                  OAuth 账户
                  <input
                    required
                    value={draft.account}
                    onChange={(e) => {
                      captureFeedback();
                      return setDraft({ ...draft, account: e.target.value });
                    }}
                  />
                </label>
                <label>
                  OAuth scopes
                  <textarea
                    value={draft.scopes}
                    onChange={(e) => {
                      captureFeedback();
                      return setDraft({ ...draft, scopes: e.target.value });
                    }}
                  />
                </label>
                <div className="actions">
                  <ActionButton
                    type="button"
                    disabled={busy || !selected || !source.data.oauth}
                    onClick={() => {
                      captureFeedback();
                      return void run(async () => {
                        const flow = await command(
                          request,
                          "integration.oauth.start",
                          project,
                          {
                            binding_ref: selected!.id,
                            account: draft.account,
                            scopes: lines(draft.scopes),
                            redirect_uri: `${window.location.origin}/oauth/integrations/callback`,
                          },
                        );
                        setOauthState(flow.state);
                        setOauthUrl(flow.authorization_url);
                        setMessage("请打开授权页面，完成后点击检查授权结果。");
                      });
                    }}
                  >
                    发起 OAuth 授权
                  </ActionButton>
                  {oauthUrl && (
                    <a href={oauthUrl} target="_blank" rel="noreferrer">
                      打开服务授权页面
                    </a>
                  )}
                  <ActionButton
                    type="button"
                    disabled={busy || !oauthState}
                    onClick={() => {
                      captureFeedback();
                      return void run(async () => {
                        const flow = await command(
                          request,
                          "integration.oauth.status",
                          project,
                          { state: oauthState },
                        );
                        if (flow.status === "complete") {
                          setDraft({
                            ...draft,
                            credential_ref: flow.credential_ref,
                            account: flow.account,
                            scopes: flow.scopes.join("\n"),
                          });
                          setMessage("授权已完成，请保存绑定新修订。");
                        } else setMessage(`授权状态：${flow.status}`);
                      });
                    }}
                  >
                    检查授权结果
                  </ActionButton>
                  <ActionButton
                    type="button"
                    disabled={busy || !selected?.data.auth?.credential_ref}
                    onClick={() => {
                      captureFeedback();
                      return void run(async () => {
                        const result = await command(
                          request,
                          "integration.oauth.revoke",
                          project,
                          { binding_ref: selected!.id },
                        );
                        setMessage(
                          `本机授权已撤销；服务端：${result.remote_revocation}`,
                        );
                        setDraft({ ...draft, credential_ref: "" });
                      });
                    }}
                  >
                    撤销 OAuth 授权
                  </ActionButton>
                </div>
              </>
            )}
            <label>
              允许的服务 origin（每行一个）
              <textarea
                value={draft.origins}
                onChange={(e) => {
                  captureFeedback();
                  return setDraft({ ...draft, origins: e.target.value });
                }}
              />
            </label>
            <label className="inline-control">
              <input
                type="checkbox"
                checked={draft.private_network}
                onChange={(e) => {
                  captureFeedback();
                  return setDraft({
                    ...draft,
                    private_network: e.target.checked,
                  });
                }}
              />
              允许所列 origin 的本机/内网地址
            </label>
            <RevisionSelect
              label="绑定允许的确切工具修订"
              records={tools}
              value={draft.allowed_tools}
              multiple
              onChange={(allowed_tools) => {
                captureFeedback();
                return setDraft({ ...draft, allowed_tools });
              }}
            />
            <p>
              发现服务不会自动授权全部动作。请先发布具体工具，再选择允许的修订。
            </p>
            <label>
              超时（毫秒）
              <input
                type="number"
                min={100}
                max={120000}
                value={draft.timeout}
                onChange={(e) => {
                  captureFeedback();
                  return setDraft({
                    ...draft,
                    timeout: Number(e.target.value),
                  });
                }}
              />
            </label>
            <label>
              结果大小上限（字节）
              <input
                type="number"
                min={1024}
                max={8388608}
                value={draft.limit}
                onChange={(e) => {
                  captureFeedback();
                  return setDraft({ ...draft, limit: Number(e.target.value) });
                }}
              />
            </label>
            {selected && (
              <label>
                绑定历史修订
                <ActionSelect
                  value={selected.id}
                  onChange={(e) => {
                    captureFeedback();

                    const row = sources.find((r) => r.id === e.target.value);
                    if (row) choose(row);
                  }}
                >
                  {sources
                    .filter(
                      (r) =>
                        r.kind === "integration_binding" &&
                        r.definition_id === selected.definition_id,
                    )
                    .map((r) => (
                      <option key={r.id} value={r.id}>
                        v{r.version} · {r.name}
                      </option>
                    ))}
                </ActionSelect>
              </label>
            )}
            <div className="actions">
              <ActionButton className="primary" disabled={busy}>
                {selected ? "保存绑定新修订" : "创建接入绑定"}
              </ActionButton>
              {selected && (
                <>
                  <ActionButton
                    type="button"
                    onClick={() => {
                      captureFeedback();
                      return setDraft({
                        ...draft,
                        selected: "",
                        name: `${draft.name} · 副本`,
                      });
                    }}
                  >
                    复制接入绑定
                  </ActionButton>
                  <ActionButton
                    type="button"
                    disabled={busy}
                    onClick={() => {
                      captureFeedback();
                      return void run(async () => {
                        await command(
                          request,
                          "integration.delete",
                          project,
                          {},
                          selected,
                        );
                        setDeletedBinding(selected);
                        await reload();
                        setMessage("绑定已删除，历史记录保留。");
                      });
                    }}
                  >
                    删除接入绑定
                  </ActionButton>
                </>
              )}
              {selected && project && (
                <ActionButton
                  type="button"
                  disabled={busy}
                  onClick={() => {
                    captureFeedback();
                    return void run(async () => {
                      const rows = await records(
                        request,
                        "query.integration_bindings",
                        project,
                      );
                      const current =
                        latest(
                          rows.filter(
                            (r) =>
                              r.kind === "integration_project_configuration" &&
                              r.definition_id === project.id,
                          ),
                        )[0]?.data || {};
                      await command(
                        request,
                        "integration.project.bind",
                        project,
                        {
                          project_id: project.id,
                          bindings: { ...current, [source.id]: selected.id },
                        },
                      );
                      setMessage(
                        "项目已采用此绑定；已有冻结 Feature/Session 保留原版本。",
                      );
                    });
                  }}
                >
                  将绑定用于当前项目
                </ActionButton>
              )}
              {selected && source.data.source.kind === "mcp" && (
                <ActionButton
                  type="button"
                  disabled={busy}
                  onClick={() => {
                    captureFeedback();
                    return void run(async () => {
                      const r = await command(
                        request,
                        "integration.discover",
                        project,
                        { binding_ref: selected.id },
                      );
                      setDiscovery(r);
                      setSelectedNames([]);
                      await reload();
                      setMessage("发现完成，请选择要发布的具体动作。");
                    });
                  }}
                >
                  发现 MCP 工具
                </ActionButton>
              )}
            </div>
          </fieldset>
        </ActionForm>
      </div>
      {discovery && (
        <div className="integration-content">
          <h3>发现的 MCP 工具</h3>
          <p>
            新增：{discovery.data.diff?.added?.join(", ") || "无"} · 移除：
            {discovery.data.diff?.removed?.join(", ") || "无"} · 契约变化：
            {discovery.data.diff?.changed?.join(", ") || "无"}
          </p>
          {(discovery.data?.result?.tools || []).map((tool: Row) => (
            <article key={tool.name}>
              <label className="inline-control">
                <input
                  type="checkbox"
                  checked={selectedNames.includes(tool.name)}
                  onChange={(e) => {
                    captureFeedback();
                    return setSelectedNames(
                      e.target.checked
                        ? [...selectedNames, tool.name]
                        : selectedNames.filter((name) => name !== tool.name),
                    );
                  }}
                />
                {tool.name}
              </label>
              <MarkdownPreview text={tool.description || ""} />
              <label>
                动作影响 · {tool.name}
                <ActionSelect
                  value={effects[tool.name] || "unknown"}
                  onChange={(e) => {
                    captureFeedback();
                    return setEffects({
                      ...effects,
                      [tool.name]: e.target.value,
                    });
                  }}
                >
                  {["unknown", "read_only", "write", "destructive"].map((s) => (
                    <option key={s}>{s}</option>
                  ))}
                </ActionSelect>
              </label>
              <details>
                <summary>协议契约</summary>
                <pre>
                  {JSON.stringify(
                    { input: tool.inputSchema, output: tool.outputSchema },
                    null,
                    2,
                  )}
                </pre>
              </details>
            </article>
          ))}
          <ActionButton
            disabled={busy || !selectedNames.length}
            onClick={() => {
              captureFeedback();
              return void run(async () => {
                await command(request, "integration.publish_tools", project, {
                  discovery_ref: discovery.id,
                  names: selectedNames,
                  effects,
                  scope: discovery.scope,
                });
                await reload();
                setMessage("工具已发布，请在绑定中选择允许的修订并保存。");
              });
            }}
          >
            发布所选 MCP 工具
          </ActionButton>
        </div>
      )}
      {selected && (
        <div className="integration-content" aria-label="接入就绪状态">
          <h3>接入状态</h3>
          <p>
            定义：已保存 · 认证：
            {selected.data.auth.kind === "none"
              ? "无需凭据"
              : selected.data.auth.kind === "oauth_pending"
                ? "等待授权"
                : "请检查凭据"}{" "}
            · 执行环境：
            {permissions.targets?.find(
              (t: Row) => t.target.revision_id === selected.data.harness_ref,
            )?.materializations?.length
              ? "已物化"
              : "待准备"}
          </p>
          <ActionButton
            disabled={busy}
            onClick={() => {
              captureFeedback();
              return void run(async () => {
                await request("runtime.prepare_harness", {
                  id: selected.data.harness_ref,
                });
                setPermissions(
                  await request("query.permission_resources", {
                    project_id: project?.id,
                  }),
                );
                setMessage(
                  "执行环境已准备；测试调用与 Agent 授权仍需分别确认。",
                );
              });
            }}
          >
            准备工具执行环境
          </ActionButton>
          <p>
            实际调用结果可在“调用记录”查看；Capability 和项目选择决定 Agent
            是否拥有此工具。
          </p>
        </div>
      )}
      {selected && (
        <ToolTest
          request={request}
          project={project}
          tools={tools}
          binding={selected}
        />
      )}
      {error && <p role="alert">{error}</p>}
      {message && <p role="status">{message}</p>}
    </section>
  );
}
function HttpToolEditor({
  source,
  request,
  project,
  catalog,
  reload,
}: {
  source: Row;
  request: Request;
  project?: Row;
  catalog: Row[];
  reload: () => Promise<void>;
}) {
  const {
    error,
    setError,
    message,
    setMessage,
    capture: captureFeedback,
    beginFeedback,
    endFeedback,
  } = useActionFeedback();

  const initial = () => ({
    selected: "",
    name: "",
    description: "",
    method: "GET",
    path: "/",
    parameters: [] as Row[],
    body_pointer: "",
    input: '{"type":"object","properties":{},"additionalProperties":false}',
    output: "",
    effect: "read_only",
    idempotency_header: "",
    constraints: "{}",
    sensitive: "",
    required_auth: "",
  });
  const [draft, setDraft] = useDraft(
    `devdesk:http-tool:${project?.id || "global"}:${source.id}`,
    initial,
  );
  const [openapi, setOpenapi] = useState(""),
    [busy, setBusy] = useState(false);
  const [preview, setPreview] = useState<Row[]>([]),
    [operations, setOperations] = useState<string[]>([]),
    [authScheme, setAuthScheme] = useState(""),
    [serverIndex, setServerIndex] = useState(0),
    [aliases, setAliases] = useState<Record<string, string>>({});
  const tools = latest(
    catalog.filter(
      (r) => r.kind === "tool" && r.data.integration_ref === source.id,
    ),
  );
  const selected = catalog.find((r) => r.id === draft.selected);
  const choose = (r: Row) => {
    const t = r.data;
    const h = t.implementation;
    setDraft({
      selected: r.id,
      name: t.qualified_name,
      description: t.description,
      method: h.method,
      path: h.path,
      parameters: h.parameters || [],
      body_pointer: h.body_pointer || "",
      input: JSON.stringify(t.input_schema, null, 2),
      output:
        t.output_schema === undefined || t.output_schema === null
          ? ""
          : JSON.stringify(t.output_schema, null, 2),
      effect: t.effect,
      idempotency_header: h.idempotency_header || "",
      constraints: JSON.stringify(t.constraints || {}, null, 2),
      sensitive: (t.sensitive_parameters || []).join("\n"),
      required_auth: t.required_auth
        ? JSON.stringify(t.required_auth, null, 2)
        : "",
    });
  };
  async function run(fn: () => Promise<void>) {
    captureFeedback();

    (beginFeedback(), setBusy(true));
    setError("");
    try {
      await fn();
    } catch (e) {
      setError(String(e));
    } finally {
      (endFeedback(), setBusy(false));
    }
  }
  return (
    <section className="panel" aria-label="HTTP 动作管理">
      <div className="panel-heading">
        <h2>HTTP 可调用动作</h2>
        <ActionButton
          onClick={() => {
            captureFeedback();
            return setDraft(initial());
          }}
        >
          新建 HTTP 动作
        </ActionButton>
      </div>
      <div className="configuration-layout">
        <div className="configuration-list">
          {tools.map((t) => (
            <ActionButton
              key={t.id}
              onClick={() => {
                captureFeedback();
                return choose(t);
              }}
            >
              {t.name} · v{t.version}
            </ActionButton>
          ))}
        </div>
        <ActionForm
          onSubmit={(e) => {
            captureFeedback();

            e.preventDefault();
            void run(async () => {
              const tool = {
                schema_version: 2,
                qualified_name: draft.name,
                description: draft.description,
                integration_ref: source.id,
                effect: draft.effect,
                input_schema: JSON.parse(draft.input),
                ...(draft.output.trim()
                  ? { output_schema: JSON.parse(draft.output) }
                  : {}),
                constraints: JSON.parse(draft.constraints),
                sensitive_parameters: lines(draft.sensitive || ""),
                ...(draft.required_auth
                  ? { required_auth: JSON.parse(draft.required_auth) }
                  : {}),
                implementation: {
                  kind: "http_api",
                  method: draft.method,
                  path: draft.path,
                  parameters: draft.parameters,
                  body_pointer: draft.body_pointer || null,
                  idempotency_header: draft.idempotency_header || null,
                },
              };
              const r = await command(
                request,
                `integration.tool.${selected ? "revise" : "create"}`,
                project,
                { name: draft.name, scope: source.scope, data: tool },
                selected,
              );
              await reload();
              choose(r);
              setMessage("HTTP 工具已发布，可在绑定和 Capability 中选择。");
            });
          }}
        >
          <label>
            工具调用名称
            <input
              required
              maxLength={64}
              value={draft.name}
              onChange={(e) => {
                captureFeedback();
                return setDraft({ ...draft, name: e.target.value });
              }}
            />
          </label>
          <MarkdownField
            key={draft.selected || "new"}
            label="工具说明"
            value={draft.description}
            required
            onChange={(description) => {
              captureFeedback();
              return setDraft({ ...draft, description });
            }}
          />
          <label>
            HTTP method
            <ActionSelect
              value={draft.method}
              onChange={(e) => {
                captureFeedback();
                return setDraft({
                  ...draft,
                  method: e.target.value,
                  effect: ["GET", "HEAD"].includes(e.target.value)
                    ? "read_only"
                    : "unknown",
                });
              }}
            >
              {["GET", "POST", "PUT", "PATCH", "DELETE", "HEAD"].map((m) => (
                <option key={m}>{m}</option>
              ))}
            </ActionSelect>
          </label>
          <label>
            相对 path
            <input
              required
              value={draft.path}
              onChange={(e) => {
                captureFeedback();
                return setDraft({ ...draft, path: e.target.value });
              }}
            />
          </label>
          <label>
            动作影响
            <ActionSelect
              value={draft.effect}
              onChange={(e) => {
                captureFeedback();
                return setDraft({ ...draft, effect: e.target.value });
              }}
            >
              {["read_only", "unknown", "write", "destructive"].map((s) => (
                <option key={s}>{s}</option>
              ))}
            </ActionSelect>
          </label>
          <JsonField
            label="调用参数 JSON Schema"
            value={draft.input}
            onChange={(input) => {
              captureFeedback();
              return setDraft({ ...draft, input });
            }}
          />
          <JsonField
            label="结构化结果 JSON Schema（可选）"
            value={draft.output}
            optional
            onChange={(output) => {
              captureFeedback();
              return setDraft({ ...draft, output });
            }}
          />
          <fieldset>
            <legend>参数映射</legend>
            {draft.parameters.map((p, index) => (
              <div key={index} className="integration-parameter">
                <label>
                  参数名 {index + 1}
                  <input
                    required
                    value={p.name}
                    onChange={(e) => {
                      captureFeedback();
                      return setDraft({
                        ...draft,
                        parameters: draft.parameters.map((v, i) =>
                          i === index ? { ...v, name: e.target.value } : v,
                        ),
                      });
                    }}
                  />
                </label>
                <label>
                  输入 JSON Pointer {index + 1}
                  <input
                    required
                    value={p.pointer}
                    onChange={(e) => {
                      captureFeedback();
                      return setDraft({
                        ...draft,
                        parameters: draft.parameters.map((v, i) =>
                          i === index ? { ...v, pointer: e.target.value } : v,
                        ),
                      });
                    }}
                  />
                </label>
                <label>
                  参数位置 {index + 1}
                  <ActionSelect
                    value={p.location}
                    onChange={(e) => {
                      captureFeedback();
                      return setDraft({
                        ...draft,
                        parameters: draft.parameters.map((v, i) =>
                          i === index ? { ...v, location: e.target.value } : v,
                        ),
                      });
                    }}
                  >
                    {["path", "query", "header"].map((l) => (
                      <option key={l}>{l}</option>
                    ))}
                  </ActionSelect>
                </label>
                <label className="inline-control">
                  <input
                    type="checkbox"
                    checked={!!p.required}
                    onChange={(e) => {
                      captureFeedback();
                      return setDraft({
                        ...draft,
                        parameters: draft.parameters.map((v, i) =>
                          i === index
                            ? { ...v, required: e.target.checked }
                            : v,
                        ),
                      });
                    }}
                  />
                  必需
                </label>
                <ActionButton
                  type="button"
                  onClick={() => {
                    captureFeedback();
                    return setDraft({
                      ...draft,
                      parameters: draft.parameters.filter(
                        (_, i) => i !== index,
                      ),
                    });
                  }}
                >
                  删除参数 {index + 1}
                </ActionButton>
              </div>
            ))}
            <ActionButton
              type="button"
              onClick={() => {
                captureFeedback();
                return setDraft({
                  ...draft,
                  parameters: [
                    ...draft.parameters,
                    {
                      name: "",
                      pointer: "/",
                      location: "query",
                      required: false,
                    },
                  ],
                });
              }}
            >
              添加参数映射
            </ActionButton>
          </fieldset>
          <label>
            JSON body 来源 Pointer（可选）
            <input
              value={draft.body_pointer}
              onChange={(e) => {
                captureFeedback();
                return setDraft({ ...draft, body_pointer: e.target.value });
              }}
            />
          </label>
          <label>
            幂等键 Header（服务明确支持时）
            <input
              value={draft.idempotency_header}
              onChange={(e) => {
                captureFeedback();
                return setDraft({
                  ...draft,
                  idempotency_header: e.target.value,
                });
              }}
            />
          </label>
          <JsonField
            label="固定参数约束（Pointer 到值）"
            value={draft.constraints}
            onChange={(constraints) => {
              captureFeedback();
              return setDraft({ ...draft, constraints });
            }}
          />
          <label>
            敏感参数 Pointer（每行一个）
            <textarea
              value={draft.sensitive || ""}
              onChange={(e) => {
                captureFeedback();
                return setDraft({ ...draft, sensitive: e.target.value });
              }}
            />
          </label>
          <JsonField
            label="认证要求（可选 JSON）"
            value={draft.required_auth || ""}
            optional
            onChange={(required_auth) => {
              captureFeedback();
              return setDraft({ ...draft, required_auth });
            }}
          />
          {selected && (
            <label>
              工具历史修订
              <ActionSelect
                value={selected.id}
                onChange={(e) => {
                  captureFeedback();

                  const row = catalog.find((r) => r.id === e.target.value);
                  if (row) choose(row);
                }}
              >
                {catalog
                  .filter(
                    (r) =>
                      r.kind === "tool" &&
                      r.definition_id === selected.definition_id,
                  )
                  .map((r) => (
                    <option key={r.id} value={r.id}>
                      v{r.version} · {r.name}
                    </option>
                  ))}
              </ActionSelect>
            </label>
          )}
          <ActionButton
            className="primary"
            disabled={busy || !draft.description.trim()}
          >
            {selected ? "发布 HTTP 动作新修订" : "发布 HTTP 动作"}
          </ActionButton>
          {selected && (
            <div className="actions">
              <ActionButton
                type="button"
                onClick={() => {
                  captureFeedback();
                  return setDraft({
                    ...draft,
                    selected: "",
                    name: `${draft.name}_copy`,
                  });
                }}
              >
                复制 HTTP 动作
              </ActionButton>
            </div>
          )}
        </ActionForm>
      </div>
      <details className="integration-content">
        <summary>从 OpenAPI 3.1 导入</summary>
        <JsonField
          label="OpenAPI JSON / YAML"
          value={openapi}
          onChange={setOpenapi}
        />
        <label>
          OpenAPI server 索引
          <input
            type="number"
            min={0}
            value={serverIndex}
            onChange={(e) => {
              captureFeedback();
              return setServerIndex(Number(e.target.value));
            }}
          />
        </label>
        <label>
          OpenAPI 认证方案名称（存在 OR 时必选）
          <input
            value={authScheme}
            onChange={(e) => {
              captureFeedback();
              return setAuthScheme(e.target.value);
            }}
            placeholder="securitySchemes 中的名称；none 表示明确的匿名分支"
          />
        </label>
        <ActionButton
          disabled={busy || !openapi.trim()}
          onClick={() => {
            captureFeedback();
            return void run(async () => {
              const r = await command(
                request,
                "integration.openapi.preview",
                project,
                {
                  integration_ref: source.id,
                  document: openapi,
                  security_scheme: authScheme || undefined,
                  server_index: serverIndex,
                },
              );
              setPreview(r.tools);
              setOperations([]);
              setAliases({});
              setMessage("预览已生成，请选择具体 operation 并确认名称。");
            });
          }}
        >
          预览 OpenAPI 动作
        </ActionButton>
        {preview.map((t) => (
          <article key={t.qualified_name}>
            <label className="inline-control">
              <input
                type="checkbox"
                checked={operations.includes(t.qualified_name)}
                onChange={(e) => {
                  captureFeedback();
                  return setOperations(
                    e.target.checked
                      ? [...operations, t.qualified_name]
                      : operations.filter((n) => n !== t.qualified_name),
                  );
                }}
              />
              {t.qualified_name} · {t.implementation.method}{" "}
              {t.implementation.path}
            </label>
            <label>
              导入工具名称 · {t.qualified_name}
              <input
                value={aliases[t.qualified_name] || t.qualified_name}
                onChange={(e) => {
                  captureFeedback();
                  return setAliases({
                    ...aliases,
                    [t.qualified_name]: e.target.value,
                  });
                }}
              />
            </label>
            <details>
              <summary>生成的调用契约</summary>
              <pre>{JSON.stringify(t, null, 2)}</pre>
            </details>
          </article>
        ))}
        <ActionButton
          disabled={busy || !operations.length}
          onClick={() => {
            captureFeedback();
            return void run(async () => {
              const r = await command(
                request,
                "integration.openapi.import",
                project,
                {
                  integration_ref: source.id,
                  document: openapi,
                  operations,
                  aliases,
                  security_scheme: authScheme || undefined,
                  server_index: serverIndex,
                  scope: source.scope,
                },
              );
              await reload();
              setMessage(`已导入 ${r.records.length} 个 HTTP 动作。`);
            });
          }}
        >
          导入 OpenAPI 动作
        </ActionButton>
      </details>
      {error && <p role="alert">{error}</p>}
      {message && <p role="status">{message}</p>}
    </section>
  );
}
function ToolTest({
  request,
  project,
  tools,
  binding,
}: {
  request: Request;
  project?: Row;
  tools: Row[];
  binding: Row;
}) {
  const {
    message,
    setMessage,
    error,
    setError,
    capture: captureFeedback,
    beginFeedback,
    endFeedback,
  } = useActionFeedback();

  const [tool, setTool] = useState(""),
    [args, setArgs] = useState("{}"),
    [busy, setBusy] = useState(false);
  return (
    <div className="integration-content">
      <h3>实际测试调用</h3>
      <p>
        使用已保存的绑定和相同执行权限。写动作会产生真实副作用，结果在调用记录中查看。
      </p>
      <RevisionSelect
        label="测试工具修订"
        records={tools.filter((t) => binding.data.allowed_tools.includes(t.id))}
        value={tool}
        onChange={setTool}
      />
      <JsonField label="测试调用参数" value={args} onChange={setArgs} />
      <ActionButton
        disabled={!tool || busy}
        onClick={async () => {
          captureFeedback();

          (beginFeedback(), setBusy(true));
          setError("");
          try {
            const result = await command(request, "integration.test", project, {
              binding_ref: binding.id,
              tool_ref: tool,
              arguments: JSON.parse(args),
            });
            setMessage(`调用已登记：${result.id}，请在调用记录查看实际回执。`);
          } catch (e) {
            setError(String(e));
          } finally {
            (endFeedback(), setBusy(false));
          }
        }}
      >
        执行测试调用
      </ActionButton>
      {message && <p role="status">{message}</p>}
      {error && <p role="alert">{error}</p>}
    </div>
  );
}
function SkillImport({
  request,
  project,
  onImported,
}: {
  request: Request;
  project?: Row;
  onImported: () => void;
}) {
  const {
    error,
    setError,
    message,
    setMessage,
    capture: captureFeedback,
    beginFeedback,
    endFeedback,
  } = useActionFeedback();

  const [name, setName] = useState(""),
    [body, setBody] = useState(""),
    [zip, setZip] = useState(""),
    [gitUrl, setGitUrl] = useState(""),
    [commit, setCommit] = useState(""),
    [directory, setDirectory] = useState(""),
    [mode, setMode] = useState("markdown"),
    [sourceRevision, setSourceRevision] = useState(""),
    [busy, setBusy] = useState(false),
    [requires, setRequires] = useState<string[]>([]),
    [catalog, setCatalog] = useState<Row[]>([]);
  useEffect(() => {
    void loadCatalog(request, project)
      .then(setCatalog)
      .catch((e) => setError(String(e)));
  }, [request, project?.id]);
  return (
    <section className="panel" aria-label="Skill 包导入">
      <div className="panel-heading">
        <h2>导入 Skill 与静态材料</h2>
      </div>
      <ActionForm
        className="integration-content"
        onSubmit={async (e) => {
          captureFeedback();

          e.preventDefault();
          (beginFeedback(), setBusy(true));
          setError("");
          try {
            const r = await command(request, "skill.import_package", project, {
              name,
              scope: project
                ? { kind: "project", project_id: project.id }
                : { kind: "global" },
              requires_tools: requires,
              source_revision_id: sourceRevision || undefined,
              ...(sourceRevision
                ? { scope: catalog.find((r) => r.id === sourceRevision)?.scope }
                : {}),
              ...(mode === "zip"
                ? { zip_base64: zip }
                : mode === "git"
                  ? { git_url: gitUrl, commit, directory }
                  : { markdown: body }),
            });
            setMessage(
              `Skill 已导入：${r.name}。可在 Capability 的 Skills 中选择。`,
            );
            setBody("");
            setZip("");
            setSourceRevision("");
            setCatalog(await loadCatalog(request, project));
            onImported();
          } catch (e) {
            setError(String(e));
          } finally {
            (endFeedback(), setBusy(false));
          }
        }}
      >
        <label>
          导入 Skill 名称
          <input
            required
            value={name}
            onChange={(e) => {
              captureFeedback();
              return setName(e.target.value);
            }}
          />
        </label>
        <RevisionSelect
          label="更新已有 Skill 修订（可选）"
          records={catalog.filter((r) => r.kind === "skill")}
          value={sourceRevision}
          onChange={async (value) => {
            captureFeedback();

            setSourceRevision(value);
            const row = catalog.find((r) => r.id === value);
            if (row) {
              setName(row.name);
              try {
                const pkg = await request("skill.export", {
                  id: value,
                  project_id: project?.id,
                });
                setBody(pkg.body);
                setRequires(pkg.requires_tools);
              } catch (e) {
                setError(String(e));
              }
            }
          }}
        />
        <label>
          Skill 来源
          <ActionSelect
            value={mode}
            onChange={(e) => {
              captureFeedback();
              return setMode(e.target.value);
            }}
          >
            <option value="markdown">文档内容</option>
            <option value="zip">静态材料 ZIP</option>
            <option value="git">公开 Git · 固定 commit</option>
          </ActionSelect>
        </label>
        {mode === "markdown" ? (
          <MarkdownField
            label="Skill 内容"
            value={body}
            required
            onChange={setBody}
          />
        ) : mode === "zip" ? (
          <label>
            Skill ZIP 文件
            <ActionInput
              type="file"
              accept=".zip"
              required
              onChange={async (e) => {
                captureFeedback();

                const file = e.target.files?.[0];
                if (!file) return;
                if (file.size > 2097152) {
                  setError("Skill ZIP 超过 2 MiB");
                  return;
                }
                const reader = new FileReader();
                reader.onload = () =>
                  setZip(String(reader.result).split(",")[1]);
                reader.readAsDataURL(file);
              }}
            />
          </label>
        ) : (
          <>
            <label>
              Skill Git HTTPS URL
              <input
                type="url"
                required
                value={gitUrl}
                onChange={(e) => {
                  captureFeedback();
                  return setGitUrl(e.target.value);
                }}
              />
            </label>
            <label>
              Skill commit
              <input
                required
                pattern="[0-9a-fA-F]{40}"
                value={commit}
                onChange={(e) => {
                  captureFeedback();
                  return setCommit(e.target.value);
                }}
              />
            </label>
            <label>
              Skill 子目录
              <input
                value={directory}
                onChange={(e) => {
                  captureFeedback();
                  return setDirectory(e.target.value);
                }}
              />
            </label>
          </>
        )}
        <RevisionSelect
          label="Skill 必需工具"
          records={catalog.filter((r) => r.kind === "tool")}
          value={requires}
          multiple
          onChange={setRequires}
        />
        <p>仅导入静态指令和参考文件；包内脚本不会自动安装或执行。</p>
        <ActionButton
          className="primary"
          disabled={
            busy ||
            (mode === "markdown" && !body.trim()) ||
            (mode === "zip" && !zip)
          }
        >
          导入 Skill 包
        </ActionButton>
        {message && <p role="status">{message}</p>}
        {error && <p role="alert">{error}</p>}
      </ActionForm>
    </section>
  );
}
export function CallHistory({
  request,
  project,
}: {
  request: Request;
  project?: Row;
}) {
  const {
    error,
    setError,
    capture: captureFeedback,
    beginFeedback,
    endFeedback,
  } = useActionFeedback();

  const [rows, setRows] = useState<Row[]>([]),
    [selected, setSelected] = useState<Row>(),
    [offset, setOffset] = useState(0),
    [total, setTotal] = useState(0),
    [evidence, setEvidence] = useState(""),
    [effect, setEffect] = useState("unknown"),
    [filters, setFilters] = useState({
      feature_id: "",
      task_id: "",
      agent_instance_id: "",
      session_id: "",
      run_id: "",
      integration_ref: "",
      tool_ref: "",
    });
  const load = useCallback(async () => {
    const query: Row = {
      local: !!project,
      project_id: project?.id,
      offset,
      limit: 25,
    };
    for (const [key, value] of Object.entries(filters))
      if (value) query[key] = value;
    const r = await request("query.tool_calls", query);
    setRows(r.records);
    setTotal(r.total);
    setSelected((old) => r.records.find((v: Row) => v.id === old?.id));
  }, [request, project?.id, offset, JSON.stringify(filters)]);
  useEffect(() => {
    let active = true;
    const run = () => {
      if (active)
        void load().catch((e) => {
          if (active) setError(String(e));
        });
    };
    run();
    const timer = setInterval(run, 1500);
    return () => {
      active = false;
      clearInterval(timer);
    };
  }, [load]);
  const act = async (method: string, row: Row, payload: Row) => {
    captureFeedback();

    try {
      await command(request, method, project, payload, row);
      await load();
    } catch (e) {
      setError(String(e));
    }
  };
  return (
    <section className="panel" aria-label="工具调用记录">
      <div className="panel-heading">
        <h2>Tools调用记录</h2>
        <ActionButton
          onClick={async () => {
            captureFeedback();
            beginFeedback();
            setError("");
            try {
              await load();
            } catch (e) {
              setError(String(e));
            } finally {
              endFeedback();
            }
          }}
        >
          刷新调用记录
        </ActionButton>
      </div>
      <div className="integration-content">
        <div className="form-grid">
          {Object.entries(filters).map(([key, value]) => (
            <label key={key}>
              {
                (
                  {
                    feature_id: "Feature ID",
                    task_id: "Task ID",
                    agent_instance_id: "Agent",
                    session_id: "Session ID",
                    run_id: "Run ID",
                    integration_ref: "接入源修订 ID",
                    tool_ref: "工具修订 ID",
                  } as Row
                )[key]
              }
              <input
                value={value}
                onChange={(e) => {
                  captureFeedback();

                  setOffset(0);
                  setFilters({ ...filters, [key]: e.target.value });
                }}
              />
            </label>
          ))}
        </div>
        <div className="tool-call-history">
          {rows.length === 0 && <p className="muted">尚无工具调用记录。</p>}
          {rows.map((r, index) => (
            <div className="tool-call-history-item" key={r.id}>
              <div className="history-row">
                <span className="event-id">#{offset + index + 1}</span>
                <div>
                  <ActionButton
                    className="history-action"
                    aria-expanded={selected?.id === r.id}
                    aria-controls={`tool-call-detail-${r.id}`}
                    onClick={() => {
                      captureFeedback();
                      setSelected((current) =>
                        current?.id === r.id ? undefined : r,
                      );
                    }}
                  >
                    <strong>{r.name}</strong>
                  </ActionButton>
                  <p>
                    {r.data.status} ·{" "}
                    {r.data.invocation?.agent_instance_id || "手工测试"}
                  </p>
                  <code>{r.id}</code>
                </div>
                <time dateTime={new Date(r.created_at).toISOString()}>
                  {new Date(r.created_at).toLocaleString()}
                </time>
              </div>
              {selected && selected.id === r.id && (
                <article
                  className="tool-call-details"
                  id={`tool-call-detail-${r.id}`}
                  aria-label="调用详情"
                >
                  <h3>{selected.name}</h3>
                  <p>
                    状态：{selected.data.status} · 远端副作用：
                    {selected.data.remote_effect}
                  </p>
                  <p>{selected.id}</p>
                  <dl>
                    <dt>工具修订</dt>
                    <dd>{selected.data.invocation.tool.revision_id}</dd>
                    <dt>接入绑定</dt>
                    <dd>{selected.data.invocation.tool.binding_ref}</dd>
                    <dt>Agent</dt>
                    <dd>
                      {selected.data.invocation.agent_instance_id || "手工测试"}
                    </dd>
                  </dl>
                  <details>
                    <summary>参数与来源</summary>
                    <pre>
                      {JSON.stringify(selected.data.invocation, null, 2)}
                    </pre>
                  </details>
                  <h4>执行回执</h4>
                  {selected.data.receipt?.content?.some(
                    (c: Row) =>
                      c.type === "resource_link" &&
                      c.uri?.startsWith("devdesk-tool-artifact:"),
                  ) && (
                    <ActionButton
                      onClick={async () => {
                        captureFeedback();

                        try {
                          const result = await command(
                            request,
                            "tool.call.artifact",
                            project,
                            {},
                            selected,
                          );
                          const bytes = Uint8Array.from(
                            atob(result.base64),
                            (c) => c.charCodeAt(0),
                          );
                          const url = URL.createObjectURL(
                            new Blob([bytes], { type: result.mime }),
                          );
                          const link = document.createElement("a");
                          link.href = url;
                          link.download = `${selected.id}.bin`;
                          link.click();
                          setTimeout(() => URL.revokeObjectURL(url), 1000);
                        } catch (e) {
                          setError(String(e));
                        }
                      }}
                    >
                      下载调用文件结果
                    </ActionButton>
                  )}
                  <pre>
                    {JSON.stringify(
                      selected.data.receipt || { status: selected.data.status },
                      null,
                      2,
                    )}
                  </pre>
                  {selected.data.status === "awaiting_approval" && (
                    <div className="actions">
                      <ActionButton
                        onClick={() => {
                          captureFeedback();
                          return void act("tool.call.approve", selected, {
                            approve: true,
                          });
                        }}
                      >
                        批准此调用
                      </ActionButton>
                      <ActionButton
                        onClick={() => {
                          captureFeedback();
                          return void act("tool.call.approve", selected, {
                            approve: false,
                          });
                        }}
                      >
                        拒绝此调用
                      </ActionButton>
                    </div>
                  )}
                  {["prepared", "dispatched", "awaiting_approval"].includes(
                    selected.data.status,
                  ) && (
                    <ActionButton
                      onClick={() => {
                        captureFeedback();
                        return void act("tool.call.cancel", selected, {});
                      }}
                    >
                      取消工具调用
                    </ActionButton>
                  )}
                  {selected.data.status === "needs_reconciliation" && (
                    <fieldset>
                      <legend>核对未确认调用</legend>
                      <p>
                        系统不会自动重发。先检查执行器实际回执；没有回执时记录人工核对的来源。
                      </p>
                      <label>
                        远端执行事实
                        <ActionSelect
                          value={effect}
                          onChange={(e) => {
                            captureFeedback();
                            return setEffect(e.target.value);
                          }}
                        >
                          <option value="unknown">仍不确定</option>
                          <option value="not_applied">确认未执行</option>
                          <option value="applied">确认已执行</option>
                        </ActionSelect>
                      </label>
                      <label>
                        核对证据
                        <textarea
                          value={evidence}
                          onChange={(e) => {
                            captureFeedback();
                            return setEvidence(e.target.value);
                          }}
                        />
                      </label>
                      <ActionButton
                        disabled={!evidence.trim()}
                        onClick={() => {
                          captureFeedback();
                          return void act("tool.call.reconcile", selected, {
                            remote_effect: effect,
                            evidence,
                          });
                        }}
                      >
                        核对调用事实
                      </ActionButton>
                    </fieldset>
                  )}
                </article>
              )}
            </div>
          ))}
        </div>
        <div className="actions">
          <ActionButton
            disabled={!offset}
            onClick={() => {
              captureFeedback();
              return setOffset(Math.max(0, offset - 25));
            }}
          >
            上一页
          </ActionButton>
          <span>
            {total ? offset + 1 : 0}–{Math.min(offset + 25, total)} / {total}
          </span>
          <ActionButton
            disabled={offset + 25 >= total}
            onClick={() => {
              captureFeedback();
              return setOffset(offset + 25);
            }}
          >
            下一页
          </ActionButton>
        </div>
        {error && <p role="alert">{error}</p>}
      </div>
    </section>
  );
}

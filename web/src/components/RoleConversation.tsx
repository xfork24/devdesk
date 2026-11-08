import { FormEvent, useCallback, useEffect, useRef, useState } from "react";

type Row = Record<string, any>;
type Request = (method: string, params?: Row) => Promise<any>;
type Props = {
  project: Row;
  feature?: Row;
  tasks: Row[];
  agents: Row[];
  installations: Row[];
  initialOwner?: string;
  initialSession?: string;
  request: Request;
  refresh: () => Promise<void>;
};
const labels: Record<string, string> = {
  analyst: "需求分析师",
  architect: "架构师",
  developer: "开发",
  tester: "测试",
  reviewer: "评审",
};

export function RoleConversation({
  project,
  feature,
  tasks,
  agents,
  installations,
  initialOwner,
  initialSession,
  request,
  refresh,
}: Props) {
  const [owner, setOwner] = useState(initialOwner || project.id);
  const [agent, setAgent] = useState("builtin-agent-analyst-v1");
  const [provider, setProvider] = useState("");
  const [installation,setInstallation]=useState("");
  const [model,setModel]=useState("");
  const [models,setModels]=useState<Row[]>([]);
  const [editing,setEditing]=useState<Row>();
  const [sessions, setSessions] = useState<Row[]>([]);
  const [sessionId, setSessionId] = useState("");
  const [detail, setDetail] = useState<Row>();
  const [events, setEvents] = useState<Row[]>([]);
  const [input, setInput] = useState("");
  const [busy, setBusy] = useState(false);
  const [error, setError] = useState("");
  const [proposalJson, setProposalJson] = useState("");
  const [answers, setAnswers] = useState<Record<string, string>>({});
  const [selections,setSelections]=useState<Record<string,string[]>>({});
  const generation = useRef(0);
  const cursor = useRef(0);
  const listGeneration = useRef(0);
  const selectedOwner =
    owner === project.id
      ? project
      : owner === feature?.id
        ? feature
        : tasks.find((t) => t.id === owner);
  const roles = agents.filter(
    (a) => a.agent_profile && !agents.some((d) => d.supersedes === a.id),
  );
  const active = detail?.turns.find(
    (t: Row) => !["completed", "failed", "cancelled"].includes(t.status),
  );
  const lastCompleted = detail?.turns.find(
    (t: Row) => t.status === "completed",
  );
  const scopedRequest = useCallback(
    (method: string, params: Row = {}) =>
      request(method, { ...params, project_id: project.id }),
    [request, project.id],
  );
  const command = (
    method: string,
    target: string | undefined,
    expected_revision: number | undefined,
    payload: Row = {},
  ) =>
    scopedRequest(method, {
      command_id: crypto.randomUUID(),
      target,
      expected_revision,
      payload,
    });
  const loadSessions = useCallback(async () => {
    const current = ++listGeneration.current;
    const result = await scopedRequest("query.sessions", { owner_id: owner });
    if (current === listGeneration.current) setSessions(result.sessions);
  }, [scopedRequest, owner]);
  useEffect(() => {
    ++generation.current;
    setSessionId(owner === initialOwner ? initialSession || "" : "");
    setDetail(undefined);
    setEvents([]);
    cursor.current = 0;
    loadSessions().catch((e) => setError(String(e)));
  }, [loadSessions,owner,initialOwner,initialSession]);
  useEffect(() => {
    const current = ++generation.current;
    cursor.current = 0;
    setEvents([]);
    setDetail(undefined);
    if (!sessionId) return;
    let stopped = false;
    let inFlight = false;
    async function poll() {
      if (inFlight) return;
      inFlight = true;
      try {
        let more = true;
        while (more && !stopped) {
          const value = await scopedRequest("query.session", {
            id: sessionId,
            cursor: cursor.current,
            limit: 100,
          });
          if (stopped || current !== generation.current) return;
          setDetail(value);
          setEvents((old) => [
            ...old,
            ...value.events.filter(
              (e: Row) => !old.some((row) => row.id === e.id),
            ),
          ]);
          cursor.current = value.next_cursor;
          more = value.events.length === 100;
        }
      } catch (e) {
        if (!stopped && current === generation.current) setError(String(e));
      } finally {
        inFlight = false;
      }
    }
    void poll();
    const timer = window.setInterval(poll, 1000);
    return () => {
      stopped = true;
      window.clearInterval(timer);
    };
  }, [sessionId, scopedRequest]);
  async function action(work: () => Promise<void>) {
    setBusy(true);
    setError("");
    try {
      await work();
    } catch (e) {
      setError(String(e));
    } finally {
      setBusy(false);
    }
  }
  async function open() {
    if (!selectedOwner) return;
    await action(async () => {
      const result = await command(
        "session.open",
        owner,
        selectedOwner.revision,
        { agent, ...(provider ? { provider } : {}), ...(installation ? {installation_id:installation, provider:installations.find(r=>r.id===installation)?.key.provider}:{}), ...(model.trim()?{model:model.trim()}:{} ) },
      );
      await loadSessions();
      setSessionId(result.id);
    });
  }
  async function send(event: FormEvent) {
    event.preventDefault();
    if (!detail || !input.trim()) return;
    await action(async () => {
      await command("conversation.send", sessionId, detail.session.revision, {
        client_message_id: crypto.randomUUID(),
        input,
      });
      setInput("");
    });
  }
  const projected: Row[] = [];
  for (const event of events) {
    const previous = event.provider_item
      ? projected.find(
          (e) =>
            e.turn_id === event.turn_id &&
            e.provider_item === event.provider_item,
        )
      : undefined;
    if (previous)
      previous.content =
        event.mode === "delta"
          ? previous.content + event.content
          : event.content;
    else projected.push({ ...event });
  }
  return (
    <div className="panel conversation-panel">
      <div className="panel-heading">
        <div>
          <h2>角色讨论</h2>
          <p className="muted">
            连续讨论保存为独立 Session。结构化内容经采纳后进入工程，正式交付和
            Review 另行完成。
          </p>
        </div>
      </div>
      <div className="actions">
        <label>
          讨论范围
          <select
            aria-label="讨论范围"
            value={owner}
            onChange={(e) => setOwner(e.target.value)}
          >
            <option value={project.id}>项目 · {project.title}</option>
            {feature && (
              <option value={feature.id}>Feature · {feature.title}</option>
            )}
            {tasks.map((t) => (
              <option key={t.id} value={t.id}>
                Task · {t.title}
              </option>
            ))}
          </select>
        </label>
        <label>
          角色
          <select
            aria-label="讨论角色"
            value={agent}
            onChange={(e) => setAgent(e.target.value)}
          >
            {roles.map((a) => (
              <option key={a.id} value={a.id}>
                {labels[a.agent_profile.role]} · {a.name} v{a.version}
              </option>
            ))}
          </select>
        </label>
        <label>
          Harness
          <select
            aria-label="讨论 Harness"
            value={provider}
            onChange={(e) => {setProvider(e.target.value);setInstallation("");setModel("");}}
          >
            <option value="">继承项目 / 全局偏好</option>
            <option value="codex">Codex</option>
            <option value="dsh">DeepSeek Harness</option>
          </select>
        </label>
        <label>共享安装<select aria-label="讨论安装" value={installation} onChange={e=>setInstallation(e.target.value)}><option value="">自动准备 / 继承固定安装偏好</option>{installations.filter(r=>r.key.os==="linux"&&r.artifact.image_id&&(!provider||r.key.provider===provider)).map(r=><option key={r.id} value={r.id}>{r.key.provider} · {r.key.version} · {r.key.arch}</option>)}</select></label>
        <label>模型<input aria-label="讨论模型" value={model} maxLength={160} onChange={e=>setModel(e.target.value)} list="discussion-models" placeholder="继承模型偏好"/><datalist id="discussion-models">{models.map(m=><option key={m.model} value={m.model}>{m.displayName||m.model}</option>)}</datalist></label>
        <button disabled={busy} onClick={()=>action(async()=>{const value=await scopedRequest("codex.auth.models");setModels(value.models);})}>读取 Codex 模型目录</button>
        <button disabled={busy || !selectedOwner} onClick={open}>
          开始 / 续用讨论
        </button>
      </div>
      <label>
        已有会话
        <select
          aria-label="讨论会话"
          value={sessionId}
          onChange={(e) => setSessionId(e.target.value)}
        >
          <option value="">选择会话</option>
          {sessions.map((s) => (
            <option key={s.id} value={s.id}>
              {s.runtime_binding.installation.key.provider} · {s.agent} ·{" "}
              {s.lifecycle} · {s.id.slice(0, 8)}
            </option>
          ))}
        </select>
      </label>
      {error && (
        <div className="error" role="alert">
          {error}
        </div>
      )}
      {detail && (
        <>
          <p className="muted">
            {detail.session.runtime_binding.installation.key.provider} ·{" "}
            {detail.session.runtime_binding.model} · {detail.session.lifecycle}{" "}
            · 原生会话 {detail.session.external_session || "尚未启动"}
          </p>
          <details><summary>本次会话的冻结权限</summary>{detail.session.runtime_binding.permissions ? <pre>{JSON.stringify(detail.session.runtime_binding.permissions, null, 2)}</pre> : <p>旧版执行未保存标准 Permission 快照；继续使用原运行锁，执行保证以历史回执为准。</p>}</details>
          {detail.executions.filter((e:Row)=>e.status==="cleanup_failed").map((e:Row)=><p key={e.id} className="notice">执行实例 {e.id} 已停止，清理失败。<button disabled={busy} onClick={()=>action(async()=>{await command("execution.retry_cleanup",e.id,undefined);})}>重试清理</button></p>)}
          {detail.session.predecessor && (
            <p>
              继承自 {detail.session.predecessor}，上下文快照{" "}
              {detail.session.snapshot_id}
            </p>
          )}
          <div role="log" aria-label="讨论记录" className="conversation-log">
            {projected.map((e) => (
              <article key={e.id} className={`conversation-message ${e.kind}`}>
                <strong>{e.kind}</strong>
                <pre>{e.content}</pre>
              </article>
            ))}
          </div>
          {active && (
            <p role="status">
              当前轮次：{active.status} · 输入
              {active.consumed ? "已消费" : "尚未确认消费"}{" "}
              <button
                disabled={busy}
                onClick={() =>
                  action(async () => {
                    await command(
                      "conversation.cancel",
                      active.id,
                      active.revision,
                    );
                  })
                }
              >
                停止当前轮次
              </button>
            </p>
          )}
          {detail.interactions
            .filter((i: Row) => i.status === "pending")
            .map((i: Row) => (
              <div className="notice" key={i.id}>
                <h3>
                  {i.kind === "tool_approval"
                    ? "运行时工具审批"
                    : "Agent 澄清问题"}
                </h3>
                <p>{i.reason}</p>
                {i.tool && <pre>{i.tool}</pre>}
                {i.questions.map((q: Row) => (
                  <label key={q.id}>
                    {q.question}
                    {q.detail&&<pre>{q.detail}</pre>}
                    {q.options.length>0&&<select aria-label={q.question+"选项"} multiple={q.multi_select} value={q.multi_select?(selections[q.id]||[]):(selections[q.id]?.[0]||"")} onChange={e=>setSelections(old=>({...old,[q.id]:Array.from(e.target.selectedOptions).map(o=>o.value).filter(Boolean)}))}>{!q.multi_select&&<option value="">请选择或填写回答</option>}{q.options.map((option:string)=><option key={option} value={option}>{option}</option>)}</select>}
                    <input
                      aria-label={q.question}
                      value={answers[q.id] || ""}
                      onChange={(e) =>
                        setAnswers((old) => ({
                          ...old,
                          [q.id]: e.target.value,
                        }))
                      }
                    />
                  </label>
                ))}
                <div className="actions">
                  {(i.kind === "tool_approval"
                    ? ["allowed_once", "rejected", "cancelled"]
                    : ["answer", "cancelled"]
                  ).map((decision) => (
                    <button
                      key={decision}
                      disabled={busy}
                      onClick={() =>
                        action(async () => {
                          const answer =
                            i.kind === "tool_approval"
                              ? { kind: "tool_approval", outcome: decision }
                              : {
                                  kind: "user_questions",
                                  cancelled: decision === "cancelled",
                                  answers:
                                    decision === "cancelled"
                                      ? []
                                      : i.questions.map((q: Row) => ({
                                          id: q.id,
                                          selected: selections[q.id] || [],
                                          custom: answers[q.id]?.trim() || null,
                                        })),
                                };
                          await command(
                            "interaction.answer",
                            i.id,
                            i.revision,
                            {
                              session_id: sessionId,
                              turn_id: i.turn_id,
                              answer,
                            },
                          );
                        })
                      }
                    >
                      {
                        (
                          {
                            allowed_once: "仅允许本次",
                            rejected: "拒绝",
                            cancelled: "取消",
                            answer: "提交回答",
                          } as Row
                        )[decision]
                      }
                    </button>
                  ))}
                </div>
              </div>
            ))}
          <form onSubmit={send}>
            <label>
              发送给角色
              <textarea
                aria-label="讨论输入"
                value={input}
                onChange={(e) => setInput(e.target.value)}
                required
                maxLength={65536}
              />
            </label>
            <button
              className="primary"
              disabled={
                busy ||
                !!active ||
                ["implementation","testing","review"].includes(detail.session.runtime_binding.work_type) ||
                [
                  "needs_reconciliation",
                  "requires_successor",
                  "closed",
                ].includes(detail.session.lifecycle)
              }
            >
              发送
            </button>
          </form>
          <details>
            <summary>结构化提案与采纳</summary>
            <p className="muted">
              请让 Agent 输出 ProposalContent
              JSON，或在此编辑内容。采纳会校验来源轮次、目标版本和输入修订。TaskPlan
              在规划 Review 通过后实例化。
            </p>
            <textarea
              aria-label="结构化提案 JSON"
              value={proposalJson}
              onChange={(e) => setProposalJson(e.target.value)}
              placeholder={
                '{"kind":"requirement","title":"...","body":"...","acceptance":[{"key":"AC-1","description":"..."}]}'
              }
            />
            <div className="actions">
              <button
                disabled={busy || !lastCompleted}
                onClick={() =>
                  action(async () => {
                    await command("proposal.extract", undefined, undefined, {
                      turn_id: lastCompleted.id,
                      target: owner,
                    });
                  })
                }
              >
                从完成的回答提取
              </button>
              <button
                disabled={busy || !lastCompleted || !proposalJson.trim()}
                onClick={() =>
                  action(async () => {
                    await command(editing?"proposal.revise":"proposal.create",editing?.id,editing?.revision,{
                      turn_id:lastCompleted.id,target:owner,content:JSON.parse(proposalJson),
                    });setEditing(undefined);setProposalJson("");
                  })
                }
              >
                {editing?"保存草稿的新修订":"保存编辑后的提案"}
              </button>
            </div>
            {detail.proposals.map((p: Row) => (
              <article key={p.id}>
                <h3>
                  {p.content.kind} · {p.status}
                </h3>
                <p className="muted">目标版本 {p.expected_revision} · 输入修订 {p.dependencies.join(", ") || "无"} · 来源 Turn {p.turn_id}</p>
                {p.supersedes&&<details><summary>查看修订前内容</summary><pre>{JSON.stringify(detail.proposals.find((v:Row)=>v.id===p.supersedes)?.content||{id:p.supersedes},null,2)}</pre></details>}
                <pre>{JSON.stringify(p.content, null, 2)}</pre>
                {["requirement","design","test_plan"].includes(p.content.kind)&&<p className="muted">采纳会生成文档新修订；工程输入变化会使相关执行与证据失效。</p>}
                <div className="actions">
                  <button disabled={busy||p.status!=="draft"} onClick={()=>{setEditing({id:p.id,revision:p.revision});setProposalJson(JSON.stringify(p.content,null,2));}}>编辑为新修订</button>
                  <button
                    disabled={busy || p.status !== "draft"}
                    onClick={() =>
                      action(async () => {
                        await command("proposal.adopt", p.id, p.revision);
                        await refresh();
                      })
                    }
                  >
                    采纳
                  </button>
                  <button
                    disabled={busy || p.status !== "draft"}
                    onClick={() =>
                      action(async () => {
                        await command("proposal.reject", p.id, p.revision);
                      })
                    }
                  >
                    拒绝
                  </button>
                  {p.content.kind === "task_plan" && (
                    <button
                      disabled={
                        busy ||
                        p.status !== "accepted" ||
                        Object.keys(p.task_instances).length > 0
                      }
                      onClick={() =>
                        action(async () => {
                          await command(
                            "proposal.instantiate",
                            p.id,
                            p.revision,
                          );
                          await refresh();
                        })
                      }
                    >
                      实例化 Tasks
                    </button>
                  )}
                </div>
              </article>
            ))}
          </details>
        </>
      )}
    </div>
  );
}

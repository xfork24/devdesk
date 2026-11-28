//! ACP v1 client. A native process and its bridge serve exactly one DevDesk Turn.
use super::*;
use std::time::Duration;

const STARTUP: Duration = Duration::from_secs(90);
const MAX_ASSISTANT: usize = 1024 * 1024;

pub(super) struct Acp {
    pub bridge: bridge::Bridge,
    pub config: PathBuf,
    assistant: String,
    allow_once: HashMap<String, String>,
}

impl Acp {
    pub(super) fn new(bridge: bridge::Bridge, config: PathBuf) -> Self {
        Self {
            bridge,
            config,
            assistant: String::new(),
            allow_once: HashMap::new(),
        }
    }
}

impl Drop for Acp {
    fn drop(&mut self) {
        self.bridge.revoke();
        let _ = fs::remove_dir_all(&self.config);
    }
}

pub(crate) fn handshake(wire: &mut JsonlProcess) -> Result<Value> {
    wire.rpc2();
    let response = wire.request_at_startup(
        "initialize",
        json!({"protocolVersion":1,"clientCapabilities":{},
            "clientInfo":{"name":"devdesk","version":env!("CARGO_PKG_VERSION")}}),
        STARTUP,
    )?;
    require(response["protocolVersion"] == 1, "ACP 版本不兼容")?;
    Ok(response["agentCapabilities"].clone())
}

pub(super) fn attach(
    wire: &mut JsonlProcess,
    external: Option<&str>,
    servers: Vec<Value>,
    model: &str,
) -> Result<String> {
    let capabilities = handshake(wire)?;
    require(
        servers.is_empty() || capabilities["mcpCapabilities"]["http"] == true,
        "ACP 未声明 HTTP MCP 支持",
    )?;
    let mut params = json!({"cwd":"/work","mcpServers":servers});
    let method = if let Some(session) = external {
        params["sessionId"] = json!(session);
        if capabilities["sessionCapabilities"].get("resume").is_some() {
            "session/resume"
        } else if capabilities["loadSession"] == true {
            "session/load"
        } else {
            return Err(err("provider_protocol", "ACP 不支持恢复已登记会话"));
        }
    } else {
        "session/new"
    };
    let response = wire.request_at_startup(method, params, STARTUP)?;
    let session = match external {
        Some(expected) => {
            require(
                response
                    .get("sessionId")
                    .is_none_or(|reported| reported == expected),
                "ACP 恢复返回了其他会话",
            )?;
            expected.to_owned()
        }
        None => string(&response, "sessionId")?,
    };
    let current = response["configOptions"]
        .as_array()
        .into_iter()
        .flatten()
        .find(|option| option["category"] == "model" || option["id"] == "model")
        .and_then(|option| option["currentValue"].as_str())
        .or_else(|| response["models"]["currentModelId"].as_str());
    require(current == Some(model), "ACP 会话未使用冻结模型")?;
    Ok(session)
}

fn observe_admission(activity: &mut Activity, output: &mut Vec<Obs>) {
    if activity.external.is_none() {
        activity.external = Some(activity.turn.clone());
        activity.consumed = true;
        output.push(Obs::Admitted {
            turn: activity.turn.clone(),
            message: None,
            external_turn: activity.external.clone(),
        });
        output.push(Obs::Consumed {
            turn: activity.turn.clone(),
            message: None,
        });
    }
}

fn assistant(activity: &Activity, mode: MessageMode, text: String) -> Obs {
    let item = format!(
        "{}:acp-assistant:{}",
        activity.turn, activity.assistant_item
    );
    Obs::Message {
        turn: activity.turn.clone(),
        key: format!("{item}:{}:{mode:?}", activity.serial),
        content: text,
        kind: ConversationEventKind::Assistant,
        mode,
        item,
    }
}

fn flush(acp: &mut Acp, activity: &mut Activity, output: &mut Vec<Obs>) {
    if !acp.assistant.is_empty() {
        output.push(assistant(
            activity,
            MessageMode::Full,
            std::mem::take(&mut acp.assistant),
        ));
        activity.assistant_item += 1;
    }
}

pub(super) fn project(
    worker: &mut Worker,
    activity: &mut Activity,
    frames: Vec<Value>,
) -> Result<Vec<Obs>> {
    let mut output = Vec::new();
    if activity.done {
        return Ok(output);
    }
    let acp = worker.acp.as_mut().expect("ACP worker");
    let mut terminal = None;
    for frame in frames {
        activity.serial += 1;
        if frame["jsonrpc"] != "2.0" {
            return Err(err("provider_protocol", "ACP 帧缺少 JSON-RPC 版本"));
        }
        if frame.get("method").is_none() {
            if frame["id"] == activity.request {
                let reason = frame["result"]["stopReason"].as_str();
                if frame.get("error").is_some() && activity.external.is_none() {
                    acp.bridge.revoke();
                    return Err(
                        err("provider_rejected", "ACP 拒绝原轮输入").technical_not_applied()
                    );
                }
                let (outcome, reason) = stop_reason(reason, frame.get("error").is_some())?;
                terminal = Some((outcome, reason));
                break;
            }
            continue;
        }
        let params = &frame["params"];
        let session_matches = params["sessionId"] == worker.session;
        match frame["method"].as_str().unwrap_or_default() {
            "session/update" if session_matches => {
                let update = &params["update"];
                match update["sessionUpdate"].as_str() {
                    Some("agent_message_chunk") => {
                        let Some(text) = update["content"]["text"].as_str() else {
                            continue;
                        };
                        observe_admission(activity, &mut output);
                        require(
                            acp.assistant.len() + text.len() <= MAX_ASSISTANT,
                            "ACP 助手文本超过观察预算",
                        )?;
                        acp.assistant.push_str(text);
                        output.push(assistant(activity, MessageMode::Delta, text.into()));
                    }
                    Some("tool_call" | "tool_call_update") => {
                        let call = string(update, "toolCallId")?;
                        observe_admission(activity, &mut output);
                        flush(acp, activity, &mut output);
                        output.push(Obs::Message {
                            turn: activity.turn.clone(),
                            key: format!("{}:acp-tool:{}", activity.turn, activity.serial),
                            item: call,
                            content: update.to_string(),
                            mode: MessageMode::Full,
                            kind: ConversationEventKind::Tool,
                        });
                    }
                    _ => {}
                }
            }
            "session/request_permission" if session_matches => {
                let request_id = &frame["id"];
                if !request_id.is_string() && !request_id.is_i64() {
                    continue;
                }
                observe_admission(activity, &mut output);
                let once = params["options"]
                    .as_array()
                    .into_iter()
                    .flatten()
                    .find(|option| option["kind"] == "allow_once")
                    .and_then(|option| option["optionId"].as_str());
                if let Some(once) = once
                    && worker
                        .permissions
                        .as_ref()
                        .is_some_and(|p| p.approval == ApprovalPolicy::OnRequest)
                {
                    let key = format!("acp:{request_id}");
                    if worker.requests.contains_key(&key) {
                        continue;
                    }
                    require(worker.requests.len() < 256, "ACP 权限请求超出观察预算")?;
                    worker.requests.insert(
                        key.clone(),
                        Request {
                            id: request_id.clone(),
                            method: "session/request_permission".into(),
                            context_digest: None,
                            answered: false,
                        },
                    );
                    acp.allow_once.insert(key.clone(), once.into());
                    output.push(Obs::Interaction {
                        turn: activity.turn.clone(),
                        request: key,
                        kind: InteractionKind::ToolApproval,
                        questions: vec![],
                        tool: params["toolCall"]["title"].as_str().map(str::to_owned),
                        reason: "ACP runtime requests a one-time tool approval".into(),
                    });
                } else {
                    worker.wire.respond(request_id, cancelled())?;
                }
            }
            "elicitation/create" if frame.get("id").is_some() => {
                worker
                    .wire
                    .respond(&frame["id"], json!({"action":"cancel"}))?;
            }
            _ if frame.get("id").is_some() => {
                worker.wire.respond_error(
                    &frame["id"],
                    -32601,
                    "Unsupported or out-of-scope ACP client request",
                )?;
            }
            _ => {}
        }
    }
    if let Some((outcome, reason)) = terminal {
        acp.bridge.revoke();
        observe_admission(activity, &mut output);
        flush(acp, activity, &mut output);
        activity.done = true;
        output.push(Obs::Terminal {
            turn: activity.turn.clone(),
            outcome,
            reason: reason.into(),
            quiescent: true,
        });
        return Ok(output);
    }
    for call in acp.bridge.drain(&activity.turn)? {
        observe_admission(activity, &mut output);
        flush(acp, activity, &mut output);
        output.push(Obs::ToolCall {
            turn:call.turn, request:json!({"kind":"bridge","operation":worker.operation,"turn":activity.turn,"call_id":call.id}),
            thread:worker.session.clone(), external_turn:activity.turn.clone(), call:call.id,
            tool:call.tool, arguments:call.arguments, worktree:worker.worktree.to_string_lossy().into_owned(),
        });
    }
    Ok(output)
}

fn stop_reason(reason: Option<&str>, error: bool) -> Result<(ProviderTerminal, &'static str)> {
    match reason {
        Some("end_turn") => Ok((ProviderTerminal::Completed, "acp_end_turn")),
        Some("cancelled") => Ok((ProviderTerminal::Cancelled, "acp_cancelled")),
        Some("max_tokens") => Ok((ProviderTerminal::Failed, "acp_max_tokens")),
        Some("max_turn_requests") => Ok((ProviderTerminal::Failed, "acp_max_turn_requests")),
        Some("refusal") => Ok((ProviderTerminal::Failed, "acp_refusal")),
        None if error => Ok((ProviderTerminal::Failed, "acp_provider_failure")),
        _ => Err(err(
            "needs_reconciliation",
            "ACP 终态不属于已核验的 stopReason",
        )),
    }
}

fn cancelled() -> Value {
    json!({"outcome":{"outcome":"cancelled"}})
}

pub(super) fn answer(worker: &mut Worker, interaction: &PendingInteraction) -> Result<()> {
    let request = worker
        .requests
        .get_mut(&interaction.external_request)
        .ok_or_else(|| err("needs_reconciliation", "ACP 交互请求已经失效"))?;
    if request.answered {
        return Ok(());
    }
    let once = worker
        .acp
        .as_ref()
        .and_then(|acp| acp.allow_once.get(&interaction.external_request))
        .ok_or_else(|| err("permission_denied", "ACP 原请求缺少一次性选项"))?;
    let answer = match interaction.answer.as_ref() {
        Some(InteractionAnswer::ToolApproval {
            outcome: ApprovalDecision::AllowedOnce,
        }) => {
            json!({"outcome":{"outcome":"selected","optionId":once}})
        }
        Some(InteractionAnswer::ToolApproval { .. }) => cancelled(),
        _ => return Err(err("invalid_params", "ACP 等待确切的权限决定")),
    };
    worker.wire.respond(&request.id, answer)?;
    request.answered = true;
    if let Some(activity) = &mut worker.activity {
        activity.deadline = std::time::Instant::now() + activity.budget;
    }
    Ok(())
}

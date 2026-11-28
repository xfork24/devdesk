//! Pi's RPC frames and its transport for frozen DevDesk step tools.
use super::*;
use std::collections::{BTreeSet, VecDeque};

const POLICY_NOTIFICATION: &str = "devdesk.policy.loaded.v1";
const TOOL_PREFIX: &str = "devdesk.tool_call.v1 ";
const APPROVAL_PREFIX: &str = "devdesk.approval.v1 ";

pub(super) fn command(session: &str, connection: &ModelConnection, model: &str) -> Vec<String> {
    [
        "pi",
        "--mode",
        "rpc",
        "-ne",
        "-e",
        "/run/devdesk/tool-policy.js",
        "--no-skills",
        "--no-prompt-templates",
        "--no-themes",
        "--no-context-files",
        "--no-approve",
        "--no-mcp",
        "--session-id",
        session,
        "--provider",
        &connection.route,
        "--model",
        model,
        "--append-system-prompt",
        "/run/devdesk/profile",
    ]
    .into_iter()
    .map(str::to_owned)
    .collect()
}

pub(super) fn initialize(
    wire: &mut JsonlProcess,
    session: &str,
    connection: &ModelConnection,
    model: &str,
) -> Result<String> {
    wire.pi_rpc();
    let state = wire.request("get_state", json!({}))?;
    let loaded = wire.drain()?.iter().any(|frame| {
        frame["type"] == "extension_ui_request"
            && frame["method"] == "notify"
            && frame["message"] == POLICY_NOTIFICATION
    });
    if !loaded {
        return Err(err("policy_not_enforced", "pi 未确认已装载冻结权限"));
    }
    require(
        state["sessionId"] == session,
        "pi 恢复的会话身份与原绑定不符",
    )?;
    if state["model"]["provider"] != connection.route || state["model"]["id"] != model {
        wire.request(
            "set_model",
            json!({"provider":connection.route,"modelId":model}),
        )?;
        let checked = wire.request("get_state", json!({}))?;
        require(
            checked["model"]["provider"] == connection.route && checked["model"]["id"] == model,
            "pi 未使用冻结模型",
        )?;
    }
    Ok(session.to_owned())
}

pub(super) fn tool_definitions(binding: &RuntimeBinding) -> Result<Vec<Value>> {
    let integration = binding
        .agent_configuration
        .as_ref()
        .and_then(|configuration| configuration.integration_lock.as_ref());
    let unbound: BTreeSet<&str> = integration
        .into_iter()
        .flat_map(|lock| lock.unbound_tools.iter())
        .filter_map(|tool| tool.data["qualified_name"].as_str())
        .collect();
    let discussion_platform: BTreeSet<String> = if binding.work_type == WorkType::Discussion {
        integration
            .into_iter()
            .flat_map(|lock| lock.platform_tools.iter())
            .map(|tool| {
                tool.decode::<ToolDefinition>()
                    .map(|definition| definition.call_name())
            })
            .collect::<Result<_>>()?
    } else {
        BTreeSet::new()
    };
    let mut names = BTreeSet::new();
    let mut tools = Vec::new();
    for tool in dynamic_tools(binding)? {
        let Some(name) = tool["name"].as_str() else {
            continue;
        };
        if unbound.contains(name)
            || discussion_platform.contains(name)
            || ["read", "bash", "edit", "write"].contains(&name)
        {
            continue;
        }
        require(
            names.insert(name.to_owned()),
            "pi 宿主工具名在冻结工具集中冲突",
        )?;
        tools.push(tool);
    }
    Ok(tools)
}

pub(super) fn project(
    worker: &mut Worker,
    activity: &mut Activity,
    frames: Vec<Value>,
) -> Result<Vec<Obs>> {
    let mut output = Vec::new();
    let mut pending: VecDeque<_> = activity.buffered.drain(..).chain(frames).collect();
    while let Some(frame) = pending.pop_front() {
        if activity.done {
            break;
        }
        activity.serial += 1;
        let event = frame["type"].as_str().unwrap_or_default();
        if event == "response" {
            if frame["id"] == activity.request
                && frame["command"] == "prompt"
                && activity.external.is_none()
            {
                if frame["success"] != true || frame["data"]["disposition"] != "started" {
                    return Err(
                        err("provider_rejected", "pi 拒绝开始此轮输入").technical_not_applied()
                    );
                }
                let external = activity.turn.clone();
                activity.external = Some(external.clone());
                output.push(Obs::Admitted {
                    turn: activity.turn.clone(),
                    message: None,
                    external_turn: Some(external),
                });
                for buffered in activity.buffered.drain(..).rev() {
                    pending.push_front(buffered);
                }
            }
            continue;
        }
        if activity.external.is_none() {
            require(activity.buffered.len() < 256, "pi 输入回执前事件过多")?;
            activity.buffered.push(frame);
            continue;
        }
        match event {
            "message_end" if frame["message"]["role"] == "user" && !activity.consumed => {
                activity.consumed = true;
                output.push(Obs::Consumed {
                    turn: activity.turn.clone(),
                    message: None,
                });
            }
            "message_update" if frame["assistantMessageEvent"]["type"] == "text_delta" => {
                if let Some(delta) = frame["assistantMessageEvent"]["delta"]
                    .as_str()
                    .filter(|s| !s.is_empty())
                {
                    output.push(assistant(activity, delta.to_owned(), MessageMode::Delta));
                }
            }
            "message_end" if frame["message"]["role"] == "assistant" => {
                let message = &frame["message"];
                let text: String = message["content"]
                    .as_array()
                    .into_iter()
                    .flatten()
                    .filter(|item| item["type"] == "text")
                    .filter_map(|item| item["text"].as_str())
                    .collect();
                if !text.is_empty() {
                    output.push(assistant(activity, text, MessageMode::Full));
                }
                activity.assistant_item += 1;
                activity.terminal = Some(match message["stopReason"].as_str() {
                    Some("error") => (ProviderTerminal::Failed, "provider_error".into()),
                    Some("length") => (ProviderTerminal::Failed, "max_tokens".into()),
                    Some("aborted") => (ProviderTerminal::Cancelled, "aborted".into()),
                    _ => (ProviderTerminal::Completed, "completed".into()),
                });
            }
            "tool_execution_start" | "tool_execution_end" => {
                let item = format!(
                    "{}:pi-tool:{}:{event}",
                    activity.turn,
                    string(&frame, "toolCallId")?
                );
                output.push(Obs::Message {
                    turn: activity.turn.clone(),
                    key: item.clone(),
                    content: serde_json::to_string(&frame).map_err(storage)?,
                    kind: ConversationEventKind::Tool,
                    mode: MessageMode::Full,
                    item,
                });
            }
            "extension_ui_request" => dialogue(worker, activity, &frame, &mut output)?,
            "agent_settled" => {
                require(activity.consumed, "pi 终态缺少输入消费证据")?;
                activity.done = true;
                let (outcome, reason) = if frame["aborted"] == true {
                    (ProviderTerminal::Cancelled, "aborted".into())
                } else {
                    activity
                        .terminal
                        .take()
                        .ok_or_else(|| err("needs_reconciliation", "pi 停止时缺少权威消息终稿"))?
                };
                output.push(Obs::Terminal {
                    turn: activity.turn.clone(),
                    outcome,
                    reason,
                    quiescent: true,
                });
            }
            _ => {}
        }
    }
    Ok(output)
}

fn assistant(activity: &Activity, content: String, mode: MessageMode) -> Obs {
    let item = format!("{}:pi-assistant:{}", activity.turn, activity.assistant_item);
    Obs::Message {
        turn: activity.turn.clone(),
        key: format!("{item}:{}", activity.serial),
        content,
        kind: ConversationEventKind::Assistant,
        mode,
        item,
    }
}

#[derive(serde::Deserialize)]
struct ToolDialog {
    tool: String,
    call: String,
    arguments: Value,
}

fn dialogue(
    worker: &mut Worker,
    activity: &Activity,
    frame: &Value,
    output: &mut Vec<Obs>,
) -> Result<()> {
    let method = frame["method"].as_str().unwrap_or_default();
    if !["select", "confirm", "input", "editor"].contains(&method) {
        return Ok(());
    }
    let id = string(frame, "id")?;
    let title = frame["title"].as_str().unwrap_or_default();
    if method == "input"
        && let Some(data) = title.strip_prefix(TOOL_PREFIX)
        && let Ok(dialog) = serde_json::from_str::<ToolDialog>(data)
        && !dialog.call.is_empty()
        && !dialog.tool.is_empty()
        && dialog.arguments.is_object()
    {
        if !register_dialog(worker, activity, &id, "pi-tool", frame)? {
            return Ok(());
        }
        output.push(Obs::ToolCall {
            turn: activity.turn.clone(),
            request: json!({"kind":"dialog","id":id}),
            thread: worker.session.clone(),
            external_turn: activity.external.clone().unwrap(),
            call: dialog.call,
            tool: dialog.tool,
            arguments: dialog.arguments,
            worktree: worker.worktree.to_string_lossy().into_owned(),
        });
        return Ok(());
    }
    if method == "select"
        && let Some(tool) = title.strip_prefix(APPROVAL_PREFIX)
        && worker
            .permissions
            .as_ref()
            .is_some_and(|p| p.approval == ApprovalPolicy::OnRequest)
        && ["bash", "edit", "write"].contains(&tool)
        && frame["options"] == json!(["Allow once", "Reject"])
    {
        if !register_dialog(worker, activity, &id, "pi-approval", frame)? {
            return Ok(());
        }
        output.push(Obs::Interaction {
            turn: activity.turn.clone(),
            request: id,
            kind: InteractionKind::ToolApproval,
            questions: vec![],
            tool: Some(tool.to_owned()),
            reason: "pi 请求执行受冻结权限约束的工具".into(),
        });
        return Ok(());
    }
    worker
        .wire
        .send(&json!({"type":"extension_ui_response","id":id,"cancelled":true}))
}

fn register_dialog(
    worker: &mut Worker,
    activity: &Activity,
    id: &str,
    method: &str,
    frame: &Value,
) -> Result<bool> {
    let digest = configuration_hash(&(&activity.turn, method, frame));
    if let Some(previous) = worker.requests.get(id) {
        require(
            previous.context_digest.as_ref() == Some(&digest),
            "pi 重复对话请求改变了原轮次或内容",
        )?;
        return Ok(false);
    }
    worker.requests.insert(
        id.to_owned(),
        Request {
            id: json!(id),
            method: method.to_owned(),
            context_digest: Some(digest),
            answered: false,
        },
    );
    Ok(true)
}

pub(super) fn answer_tool(
    worker: &mut Worker,
    request: &Value,
    receipt: &ToolReceipt,
) -> Result<()> {
    require(request["kind"] == "dialog", "pi 工具回执缺少原始对话请求")?;
    reply(
        worker,
        &string(request, "id")?,
        "pi-tool",
        Some(serde_json::to_string(receipt).map_err(storage)?),
    )
}

pub(super) fn answer(worker: &mut Worker, interaction: &PendingInteraction) -> Result<()> {
    let value = match &interaction.answer {
        Some(InteractionAnswer::ToolApproval {
            outcome: ApprovalDecision::AllowedOnce,
        }) => Some("Allow once".into()),
        Some(InteractionAnswer::ToolApproval {
            outcome: ApprovalDecision::Rejected,
        }) => Some("Reject".into()),
        Some(InteractionAnswer::ToolApproval {
            outcome: ApprovalDecision::Cancelled,
        }) => None,
        _ => return Err(err("invalid_params", "pi 交互只接受原始工具审批裁决")),
    };
    reply(worker, &interaction.external_request, "pi-approval", value)
}

fn reply(worker: &mut Worker, id: &str, method: &str, value: Option<String>) -> Result<()> {
    let request = worker
        .requests
        .get_mut(id)
        .ok_or_else(|| err("needs_reconciliation", "pi 原始对话已丢失，不能伪造回执"))?;
    require(request.method == method, "pi 对话回执类型与原请求不符")?;
    if request.answered {
        return Ok(());
    }
    let frame = if let Some(value) = value {
        json!({"type":"extension_ui_response","id":id,"value":value})
    } else {
        json!({"type":"extension_ui_response","id":id,"cancelled":true})
    };
    worker.wire.send(&frame)?;
    request.answered = true;
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::time::Duration;

    #[test]
    fn project_pi_turn_preserves_tool_identity_and_full_message() {
        let mut command = std::process::Command::new("/bin/cat");
        command.arg("-u");
        let mut worker = Worker {
            worktree: PathBuf::new(),
            permissions: None,
            wire: JsonlProcess::spawn(command).unwrap(),
            provider: HarnessProvider::Pi,
            session: "fixture-session".into(),
            operation: id(),
            container: "fixture".into(),
            image: "fixture".into(),
            log: PathBuf::new(),
            activity: None,
            requests: HashMap::new(),
        };
        let mut activity = Activity {
            budget: Duration::from_secs(60),
            deadline: std::time::Instant::now() + Duration::from_secs(60),
            turn: "fixture-turn".into(),
            request: 2,
            external: None,
            consumed: false,
            terminal: None,
            wait: None,
            done: false,
            buffered: vec![],
            serial: 0,
            assistant_item: 0,
        };
        let fixture: Value = serde_json::from_str(include_str!(
            "../../../tests/fixtures/protocols/pi-turn.json"
        ))
        .unwrap();
        let mut frames = fixture["frames"].as_array().unwrap().clone();
        let tool = frames
            .iter()
            .find(|frame| frame["id"] == "fixture-dialog")
            .unwrap()
            .clone();
        let before_out = frames
            .iter()
            .position(|frame| frame["type"] == "tool_execution_end")
            .unwrap();
        frames.insert(before_out, tool);
        let facts = project(&mut worker, &mut activity, frames).unwrap();
        assert!(matches!(
            &facts[..2],
            [Obs::Admitted { .. }, Obs::Consumed { .. }]
        ));
        let tool_calls: Vec<_> = facts
            .iter()
            .filter(|fact| matches!(fact, Obs::ToolCall { .. }))
            .collect();
        assert!(matches!(
            &tool_calls[..],
            [Obs::ToolCall { request, external_turn, call, .. }]
                if *request == json!({"kind":"dialog","id":"fixture-dialog"})
                && external_turn == "fixture-turn" && call == "fixture-call"
        ));
        assert_eq!(worker.requests.len(), 1);
        let messages: Vec<_> = facts
            .iter()
            .filter_map(|fact| match fact {
                Obs::Message {
                    kind: ConversationEventKind::Assistant,
                    key,
                    content,
                    mode,
                    item,
                    ..
                } => Some((key, content.as_str(), mode, item)),
                _ => None,
            })
            .collect();
        assert_eq!(
            messages
                .iter()
                .map(|(_, content, mode, _)| (*content, **mode))
                .collect::<Vec<_>>(),
            [
                ("PI_", MessageMode::Delta),
                ("OK", MessageMode::Delta),
                ("PI_OK", MessageMode::Full)
            ]
        );
        assert_eq!(
            messages
                .iter()
                .map(|(_, _, _, item)| item)
                .collect::<BTreeSet<_>>()
                .len(),
            1
        );
        assert_eq!(
            messages
                .iter()
                .map(|(key, _, _, _)| key)
                .collect::<BTreeSet<_>>()
                .len(),
            3
        );
        assert!(matches!(
            facts.last(),
            Some(Obs::Terminal {
                outcome: ProviderTerminal::Completed,
                quiescent: true,
                ..
            })
        ));

        let deadline = std::time::Instant::now() + Duration::from_secs(3);
        let mut cancelled = BTreeSet::new();
        while cancelled.len() < 2 {
            for frame in worker.wire.drain().unwrap() {
                assert_eq!(frame["type"], "extension_ui_response");
                assert_eq!(frame["cancelled"], true);
                cancelled.insert(string(&frame, "id").unwrap());
            }
            assert!(
                std::time::Instant::now() < deadline,
                "unexpected dialogs were not cancelled"
            );
            std::thread::sleep(Duration::from_millis(10));
        }
        assert_eq!(
            cancelled,
            BTreeSet::from(["fixture-unmanaged".into(), "fixture-approval".into()])
        );
    }
}

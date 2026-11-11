//! Typed discussion material; adopting a document does not create engineering tasks or grant evidence.
use crate::*;
impl Engine {
    pub(crate) fn proposal_command(
        &self,
        method: &str,
        command: &Command,
        state: &mut EngineeringState,
    ) -> Result<Option<Value>> {
        if !method.starts_with("proposal.") {
            return Ok(None);
        }
        let target = command.target.as_deref().unwrap_or("");
        let p = &command.payload;
        match method {
            "proposal.create" | "proposal.extract" | "proposal.revise" => {
                let previous = if method == "proposal.revise" {
                    Some(
                        state
                            .proposals
                            .iter()
                            .find(|r| r.id == target)
                            .ok_or_else(|| err("not_found", "提案不存在"))?
                            .clone(),
                    )
                } else {
                    None
                };
                if let Some(previous) = &previous {
                    require(
                        previous.status == ProposalStatus::Draft
                            && command.expected_revision == Some(previous.revision),
                        "提案版本或状态已变化",
                    )?;
                    require(
                        !state
                            .proposals
                            .iter()
                            .any(|r| r.supersedes.as_ref() == Some(&previous.id)),
                        "提案已有后继版本",
                    )?;
                }
                let turn_id = previous
                    .as_ref()
                    .map(|r| r.turn_id.as_str())
                    .or(p["turn_id"].as_str())
                    .ok_or_else(|| err("invalid_params", "需要来源 turn_id"))?;
                let turn = state
                    .turns
                    .iter()
                    .find(|t| t.id == turn_id && t.status == TurnStatus::Completed)
                    .ok_or_else(|| err("blocked", "来源轮次尚未真实完成"))?;
                let session = state
                    .sessions
                    .iter()
                    .find(|s| s.id == turn.session_id)
                    .unwrap()
                    .clone();
                require(
                    !crate::agent_tasks::formal_session(state, &session.id),
                    "工作流产物由步骤输出合同接收，请在工作流中处理",
                )?;
                let content: ProposalContent = if method == "proposal.extract" {
                    let body = state
                        .conversation_events
                        .iter()
                        .filter(|e| {
                            e.turn_id == turn_id && e.kind == ConversationEventKind::Assistant
                        })
                        .fold(String::new(), |mut body, e| {
                            if e.mode == MessageMode::Full {
                                body = e.content.clone()
                            } else {
                                body.push_str(&e.content)
                            }
                            body
                        });
                    serde_json::from_str(body.trim())
                        .map_err(|e| err("invalid_proposal", e.to_string()))?
                } else {
                    serde_json::from_value(p["content"].clone())
                        .map_err(|e| err("invalid_proposal", e.to_string()))?
                };
                require(!content.title.trim().is_empty(), "材料标题不能为空")?;
                let scope = Scope::Project {
                    project_id: turn.project_id.clone(),
                };
                let delivery = crate::governance::reference(
                    state,
                    &content.delivery_ref,
                    "delivery_type",
                    &scope,
                )?;
                let definition: DeliveryTypeDefinition = delivery.decode()?;
                require(
                    !matches!(
                        definition.producer,
                        Some(DeliveryProducer::Platform { .. } | DeliveryProducer::Workflow)
                    ),
                    "提案不能冒充平台或子流程产出",
                )?;
                self.validate_configured_value(&definition, &content.value)?;
                for input in &content.input_refs {
                    let same_project =
                        state
                            .documents
                            .iter()
                            .find(|d| d.id == *input)
                            .is_some_and(|d| {
                                d.project_id.as_ref() == Some(&turn.project_id)
                                    || d.feature_id
                                        .as_deref()
                                        .and_then(|id| state.feature(id).ok())
                                        .is_some_and(|f| f.project_id == turn.project_id)
                            })
                            || state.configuration(input).is_ok_and(|r| {
                                r.kind == "workflow_output"
                                    && r.scope.project() == Some(turn.project_id.as_str())
                            });
                    require(same_project, "材料输入必须属于同一项目")?;
                }
                let expected = revision(state, &session.owner_id);
                require(expected.is_some(), "提案需要明确的项目或工作范围")?;
                let owner = material_owner(
                    session
                        .project_id
                        .as_deref()
                        .filter(|_| session.scope == Some(SessionScope::Project)),
                    session.feature_id.as_deref(),
                    session.task_id.as_deref(),
                )?;
                let artifact = self.artifacts.seal_scoped(
                    &owner,
                    "proposals",
                    &serde_json::to_string(&content)
                        .map_err(|e| err("serialization", e.to_string()))?,
                )?;
                let row = Proposal {
                    id: id(),
                    project_id: turn.project_id.clone(),
                    session_id: session.id,
                    turn_id: turn.id.clone(),
                    schema_version: 2,
                    dependencies: content.input_refs.clone(),
                    content,
                    target: Some(session.owner_id),
                    expected_revision: expected,
                    status: ProposalStatus::Draft,
                    artifact_id: Some(artifact.id.clone()),
                    adopted_objects: vec![],
                    supersedes: previous.map(|r| r.id),
                    revision: 1,
                };
                let result = json!({"id":row.id});
                state.artifacts.push(artifact);
                state.proposals.push(row);
                Ok(Some(result))
            }
            "proposal.reject" | "proposal.adopt" => {
                let row = state
                    .proposals
                    .iter()
                    .find(|r| r.id == target)
                    .ok_or_else(|| err("not_found", "提案不存在"))?
                    .clone();
                require(
                    row.status == ProposalStatus::Draft
                        && command.expected_revision == Some(row.revision),
                    "提案状态已变化",
                )?;
                require(
                    !state
                        .proposals
                        .iter()
                        .any(|r| r.supersedes.as_ref() == Some(&row.id)),
                    "提案已有后继版本",
                )?;
                if method == "proposal.reject" {
                    let current = state.proposals.iter_mut().find(|r| r.id == target).unwrap();
                    current.status = ProposalStatus::Rejected;
                    current.revision += 1;
                    return Ok(Some(json!({"id":target})));
                }
                let owner = row
                    .target
                    .as_deref()
                    .ok_or_else(|| err("scope_violation", "提案归属缺失"))?;
                require(
                    revision(state, owner) == row.expected_revision,
                    "目标范围版本已变化，请重建提案",
                )?;
                let definition: DeliveryTypeDefinition =
                    state.configuration(&row.content.delivery_ref)?.decode()?;
                self.validate_configured_value(&definition, &row.content.value)?;
                let body = row
                    .content
                    .value
                    .as_str()
                    .map(str::to_owned)
                    .unwrap_or_else(|| serde_json::to_string_pretty(&row.content.value).unwrap());
                let material=self.revise_material(state,owner,&json!({"kind":row.content.delivery_ref,"title":row.content.title,"body":body,"input_refs":row.content.input_refs}))?;
                let current = state.proposals.iter_mut().find(|r| r.id == target).unwrap();
                current.status = ProposalStatus::Accepted;
                current.adopted_objects = vec![text(&material, "id")?];
                current.revision += 1;
                Ok(Some(json!({"id":target,"objects":current.adopted_objects})))
            }
            _ => Err(err("method_not_found", "未知提案命令")),
        }
    }
}

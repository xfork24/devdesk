//! Structured discussion outcomes are adopted through the same formal domain commands as manual edits.
use crate::*;
use std::collections::{HashMap, HashSet};
fn dependencies(content: &ProposalContent) -> Vec<Id> {
    match content {
        ProposalContent::Requirement { .. } => vec![],
        ProposalContent::Feature { requirement, .. }
        | ProposalContent::Design { requirement, .. } => vec![requirement.clone()],
        ProposalContent::TaskPlan {
            requirement,
            design,
            ..
        }
        | ProposalContent::TestPlan {
            requirement,
            design,
            ..
        } => vec![requirement.clone(), design.clone()],
    }
}
fn validate(
    s: &EngineeringState,
    session: &Session,
    content: &ProposalContent,
    target: &str,
) -> Result<()> {
    let project = session.project_id.as_deref().unwrap();
    require(
        s.projects.iter().any(|p| p.id == target && p.id == project)
            || s.features
                .iter()
                .any(|f| f.id == target && f.project_id == project),
        "提案目标必须属于来源项目",
    )?;
    for id in dependencies(content) {
        let doc = s
            .documents
            .iter()
            .find(|d| d.id == id)
            .ok_or_else(|| err("not_found", "提案输入修订不存在"))?;
        let owner = material_owner(
            doc.project_id.as_deref(),
            doc.feature_id.as_deref(),
            doc.task_id.as_deref(),
        )?;
        require(s.material_project(&owner)? == project, "提案引用跨项目")?;
        if s.features.iter().any(|f| f.id == target) {
            require(
                s.document_in_feature(doc, target),
                "提案输入不属于目标 Feature",
            )?;
        }
        require(
            !s.documents.iter().any(|d| {
                d.supersedes.as_ref() == Some(&id)
                    && if s.features.iter().any(|f| f.id == target) {
                        s.document_in_feature(d, target)
                    } else {
                        d.project_id.as_deref() == Some(target)
                    }
            }),
            "提案输入已被修订，请基于新修订重新讨论",
        )?;
    }
    match content {
        ProposalContent::Requirement {
            title,
            body,
            acceptance,
        } => {
            require(
                !title.trim().is_empty() && !body.trim().is_empty(),
                "需求标题与正文不能为空",
            )?;
            criteria(&json!({"acceptance":acceptance}), body)?;
        }
        ProposalContent::Feature { title, requirement } => {
            require(
                !title.trim().is_empty() && target == project,
                "Feature 提案应以 Project 为目标",
            )?;
            require(
                s.documents.iter().any(|d| {
                    d.id == *requirement
                        && d.kind == "requirement"
                        && d.project_id.as_deref() == Some(project)
                }),
                "Feature 需要同项目的 Project Requirement",
            )?;
        }
        ProposalContent::Design {
            title,
            body,
            requirement,
        } => {
            require(
                !title.trim().is_empty()
                    && !body.trim().is_empty()
                    && s.features.iter().any(|f| f.id == target),
                "Design 需要 Feature 目标与完整正文",
            )?;
            require(
                s.documents
                    .iter()
                    .any(|d| d.id == *requirement && d.kind == "requirement"),
                "Design 缺少确定需求修订",
            )?;
        }
        ProposalContent::TaskPlan {
            requirement,
            design,
            tasks,
        } => {
            require(
                !tasks.is_empty() && tasks.len() <= 100,
                "TaskPlan 需包含 1—100 个条目",
            )?;
            require(
                s.features.iter().any(|f| f.id == target),
                "TaskPlan 必须属于 Feature",
            )?;
            let keys: HashSet<_> = tasks.iter().map(|t| t.key.as_str()).collect();
            require(
                keys.len() == tasks.len() && keys.iter().all(|k| !k.trim().is_empty()),
                "TaskPlan 条目标识必须非空且唯一",
            )?;
            let acceptance = s.acceptance_keys(requirement)?;
            for t in tasks {
                if let Some(recipe) = &t.test_recipe {
                    recipe.validate()?;
                }
                require(
                    !t.title.trim().is_empty()
                        && !t.acceptance.is_empty()
                        && t.acceptance.iter().all(|key| acceptance.contains(key)),
                    "Task 范围必须映射需求验收条目",
                )?;
                require(
                    t.dependencies
                        .iter()
                        .all(|k| keys.contains(k.as_str()) && k != &t.key),
                    "TaskPlan 依赖不存在或引用自身",
                )?;
            }
            let mut done = HashSet::new();
            loop {
                let old = done.len();
                for t in tasks {
                    if let Some(recipe) = &t.test_recipe {
                        recipe.validate()?;
                    }
                    if t.dependencies.iter().all(|k| done.contains(k)) {
                        done.insert(t.key.clone());
                    }
                }
                if done.len() == tasks.len() {
                    break;
                }
                require(done.len() > old, "TaskPlan 依赖存在环")?;
            }
            require(
                s.documents.iter().any(|d| {
                    d.id == *design
                        && d.kind == "design"
                        && d.requirement_id.as_ref() == Some(requirement)
                }),
                "TaskPlan 设计与需求修订不匹配",
            )?;
        }
        ProposalContent::TestPlan {
            title,
            body,
            requirement,
            design,
            verifies,
            test_recipe,
        } => {
            if let Some(recipe) = test_recipe {
                recipe.validate()?;
            }
            let keys = s.acceptance_keys(requirement)?;
            require(
                !title.trim().is_empty()
                    && !body.trim().is_empty()
                    && !verifies.is_empty()
                    && verifies.iter().all(|k| keys.contains(k)),
                "TestPlan 缺少有效验收映射",
            )?;
            require(
                s.documents.iter().any(|d| {
                    d.id == *design
                        && d.kind == "design"
                        && d.requirement_id.as_ref() == Some(requirement)
                }),
                "TestPlan 设计与需求修订不匹配",
            )?;
        }
    }
    Ok(())
}
impl Engine {
    pub(crate) fn proposal_command(
        &self,
        method: &str,
        c: &Command,
        s: &mut EngineeringState,
    ) -> Result<Option<Value>> {
        if !method.starts_with("proposal.") {
            return Ok(None);
        }
        let target = c.target.as_deref().unwrap_or("");
        let p = &c.payload;
        let result = match method {
            "proposal.create" | "proposal.extract" | "proposal.revise" => {
                let previous = if method == "proposal.revise" {
                    Some(
                        s.proposals
                            .iter()
                            .find(|p| p.id == target)
                            .cloned()
                            .ok_or_else(|| err("not_found", "Proposal 不存在"))?,
                    )
                } else {
                    None
                };
                if let Some(old) = &previous {
                    require(
                        c.expected_revision == Some(old.revision)
                            && old.status == ProposalStatus::Draft,
                        "只能修订当前草稿",
                    )?;
                    require(
                        !s.proposals
                            .iter()
                            .any(|p| p.supersedes.as_deref() == Some(target)),
                        "提案已有新修订",
                    )?;
                }
                let tid = previous
                    .as_ref()
                    .map(|p| p.turn_id.as_str())
                    .or_else(|| p["turn_id"].as_str())
                    .ok_or_else(|| err("invalid_params", "需要来源 turn_id"))?;
                let turn = s
                    .turns
                    .iter()
                    .find(|t| t.id == tid && t.status == TurnStatus::Completed)
                    .ok_or_else(|| err("blocked", "来源轮次必须真实完成"))?;
                let session = s
                    .sessions
                    .iter()
                    .find(|v| v.id == turn.session_id)
                    .unwrap()
                    .clone();
                let formal_session = crate::agent_tasks::formal_session(s, &session.id);
                require(
                    turn.run_id.is_none() && !formal_session,
                    "正式交付或评审结果不属于需求提案；请在工作流查看模型结论并执行人类 Approve",
                )?;
                let content: ProposalContent = if method == "proposal.extract" {
                    let text = s
                        .conversation_events
                        .iter()
                        .rev()
                        .find(|e| {
                            e.turn_id == tid
                                && e.kind == ConversationEventKind::Assistant
                                && e.mode == MessageMode::Full
                        })
                        .ok_or_else(|| err("blocked", "没有完整模型输出"))?
                        .content
                        .trim();
                    let clean = text
                        .strip_prefix("```json")
                        .and_then(|s| s.strip_suffix("```"))
                        .unwrap_or(text)
                        .trim();
                    serde_json::from_str(clean).map_err(|_| {
                        err(
                            "invalid_proposal",
                            "模型输出不符合 ProposalContent schema；请编辑结构化内容后重试",
                        )
                    })?
                } else {
                    serde_json::from_value(p["content"].clone())
                        .map_err(|_| err("invalid_proposal", "结构化提案内容无效"))?
                };
                let owner = p["target"]
                    .as_str()
                    .or_else(|| previous.as_ref().and_then(|p| p.target.as_deref()))
                    .unwrap_or(&session.owner_id)
                    .to_string();
                validate(s, &session, &content, &owner)?;
                let expected = p["expected_revision"]
                    .as_u64()
                    .or_else(|| {
                        if owner == session.owner_id {
                            turn.source_revision
                        } else {
                            None
                        }
                    })
                    .or_else(|| revision(s, &owner))
                    .ok_or_else(|| err("blocked", "目标缺少 revision"))?;
                require(revision(s, &owner) == Some(expected), "提案目标版本已变化")?;
                let material = if s.projects.iter().any(|v| v.id == owner) {
                    MaterialOwner::Project { id: owner.clone() }
                } else {
                    MaterialOwner::Feature { id: owner.clone() }
                };
                let body = serde_json::to_string_pretty(&content)
                    .map_err(|e| err("serialization", e.to_string()))?;
                let mut artifact = self.artifacts.seal_scoped(&material, "proposals", &body)?;
                artifact.origin = if method == "proposal.extract" {
                    Origin::External
                } else {
                    Origin::Human
                };
                let proposal = Proposal {
                    id: id(),
                    project_id: turn.project_id.clone(),
                    session_id: session.id,
                    turn_id: tid.into(),
                    schema_version: 1,
                    dependencies: dependencies(&content),
                    content,
                    target: Some(owner),
                    expected_revision: Some(expected),
                    status: ProposalStatus::Draft,
                    artifact_id: Some(artifact.id.clone()),
                    adopted_objects: vec![],
                    supersedes: previous.map(|p| p.id),
                    revision: 1,
                    task_instances: HashMap::new(),
                };
                let id = proposal.id.clone();
                s.artifacts.push(artifact);
                s.proposals.push(proposal);
                json!({"id":id})
            }
            "proposal.reject" => {
                let row = s
                    .proposals
                    .iter_mut()
                    .find(|p| p.id == target)
                    .ok_or_else(|| err("not_found", "Proposal 不存在"))?;
                require(
                    c.expected_revision == Some(row.revision)
                        && row.status == ProposalStatus::Draft,
                    "只能拒绝当前草稿",
                )?;
                row.status = ProposalStatus::Rejected;
                row.revision += 1;
                json!({"id":target})
            }
            "proposal.adopt" => {
                let proposal = s
                    .proposals
                    .iter()
                    .find(|p| p.id == target)
                    .cloned()
                    .ok_or_else(|| err("not_found", "Proposal 不存在"))?;
                require(
                    proposal.status == ProposalStatus::Draft
                        && c.expected_revision == Some(proposal.revision),
                    "提案已处理或 revision 冲突",
                )?;
                require(
                    !s.proposals
                        .iter()
                        .any(|p| p.supersedes.as_deref() == Some(target)),
                    "提案已有后继修订",
                )?;
                let owner = proposal.target.as_deref().unwrap();
                require(
                    revision(s, owner) == proposal.expected_revision,
                    "提案目标已变化，请重新确认内容",
                )?;
                let session = s
                    .sessions
                    .iter()
                    .find(|s| s.id == proposal.session_id)
                    .unwrap();
                validate(s, session, &proposal.content, owner)?;
                let (method, payload) = match &proposal.content {
                    ProposalContent::Requirement {
                        title,
                        body,
                        acceptance,
                    } => (
                        "document.revise",
                        json!({"kind":"requirement","title":title,"body":body,"acceptance":acceptance}),
                    ),
                    ProposalContent::Feature { title, requirement } => (
                        "feature.create",
                        json!({"project_id":proposal.project_id,"title":title,"source_requirement":requirement}),
                    ),
                    ProposalContent::Design { title, body, .. } => (
                        "document.revise",
                        json!({"kind":"design","title":title,"body":body}),
                    ),
                    ProposalContent::TaskPlan { .. } => (
                        "document.revise",
                        json!({"kind":"doc","title":"Structured Task Plan","body":serde_json::to_string_pretty(&proposal.content).unwrap()}),
                    ),
                    ProposalContent::TestPlan {
                        title,
                        body,
                        verifies,
                        test_recipe,
                        ..
                    } => (
                        "document.revise",
                        json!({"kind":"test","title":title,"body":body,"verifies":verifies,"test_recipe":test_recipe}),
                    ),
                };
                let result = self.apply_command(
                    method,
                    &Command {
                        command_id: format!("{}:adopt", c.command_id),
                        target: Some(owner.into()),
                        expected_revision: proposal.expected_revision,
                        payload,
                    },
                    s,
                )?;
                let object = string_result(&result)?;
                let row = s.proposals.iter_mut().find(|p| p.id == target).unwrap();
                row.status = ProposalStatus::Accepted;
                row.adopted_objects = vec![object];
                row.revision += 1;
                json!({"id":target,"objects":row.adopted_objects})
            }
            "proposal.instantiate" => {
                let proposal = s
                    .proposals
                    .iter()
                    .find(|p| p.id == target)
                    .cloned()
                    .ok_or_else(|| err("not_found", "Proposal 不存在"))?;
                require(
                    c.expected_revision == Some(proposal.revision)
                        && proposal.status == ProposalStatus::Accepted,
                    "需要已采纳的 TaskPlan",
                )?;
                let ProposalContent::TaskPlan {
                    requirement,
                    design,
                    tasks,
                } = &proposal.content
                else {
                    return Err(err("blocked", "只支持实例化 TaskPlan"));
                };
                if !proposal.task_instances.is_empty() {
                    return Ok(Some(json!({"id":target,"tasks":proposal.task_instances})));
                }
                let fid = proposal.target.as_deref().unwrap();
                let session = s
                    .sessions
                    .iter()
                    .find(|s| s.id == proposal.session_id)
                    .unwrap();
                validate(s, session, &proposal.content, fid)?;
                let mut instances: HashMap<String, Id> = HashMap::new();
                while instances.len() < tasks.len() {
                    for task in tasks {
                        if instances.contains_key(&task.key)
                            || !task.dependencies.iter().all(|k| instances.contains_key(k))
                        {
                            continue;
                        }
                        let deps: Vec<_> = task
                            .dependencies
                            .iter()
                            .map(|k| instances[k].clone())
                            .collect();
                        let result=self.apply_command("task.create",&Command{command_id:format!("{}:{}",c.command_id,task.key),target:Some(fid.into()),expected_revision:revision(s,fid),payload:json!({"title":task.title,"dependencies":deps,"scope":{"requirement":requirement,"design":design,"acceptance":task.acceptance},"test_recipe":task.test_recipe})},s)?;
                        instances.insert(task.key.clone(), string_result(&result)?);
                    }
                }
                let row = s.proposals.iter_mut().find(|p| p.id == target).unwrap();
                row.task_instances = instances;
                row.revision += 1;
                json!({"id":target,"tasks":row.task_instances})
            }
            _ => return Err(err("method_not_found", "未知 Proposal 命令")),
        };
        Ok(Some(result))
    }
}
fn string_result(v: &Value) -> Result<String> {
    v["id"]
        .as_str()
        .map(str::to_owned)
        .ok_or_else(|| err("internal", "领域命令没有返回对象 ID"))
}

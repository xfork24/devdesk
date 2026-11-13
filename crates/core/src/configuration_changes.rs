//! One configuration candidate/validation/commit pipeline for UI and managed callers.
use crate::*;
use std::collections::BTreeSet;
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(tag = "type", rename_all = "snake_case", deny_unknown_fields)]
pub enum ConfigurationOperation {
    Create {
        source: SourceDefinition,
    },
    Revise {
        base_ref: Id,
        expected_revision: u64,
        source: SourceDefinition,
    },
    Retire {
        reference: Id,
        expected_revision: u64,
        reason: String,
    },
    ManageInstance {
        action: String,
        #[serde(default)]
        reference: Option<Id>,
        #[serde(default)]
        expected_revision: Option<u64>,
        #[serde(default)]
        name: Option<String>,
        #[serde(default)]
        agent_ref: Option<Id>,
    },
    PermissionBindings {
        bindings: Vec<Value>,
    },
    ImportSkill {
        name: String,
        package: SkillPackage,
        #[serde(default)]
        source_revision_id: Option<Id>,
    },
    SelectProject {
        project_id: Id,
        expected_revision: u64,
        selection: Value,
    },
    SelectWork {
        owner_id: Id,
        expected_revision: u64,
        selection: Value,
    },
}
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ConfigurationChangeSet {
    pub operations: Vec<ConfigurationOperation>,
}
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ConfigurationDiagnostic {
    pub code: String,
    pub source_ref: Id,
    pub path: String,
    pub target_ref: Option<Id>,
    pub message: String,
}
fn diagnostic(
    r: &ConfigurationRecord,
    path: &str,
    target: Option<Id>,
    e: Error,
) -> ConfigurationDiagnostic {
    ConfigurationDiagnostic {
        code: e.code,
        source_ref: r.id.clone(),
        path: path.into(),
        target_ref: target,
        message: e.message,
    }
}
pub fn configuration_fingerprint(s: &EngineeringState) -> String {
    let mut records: Vec<_> = s
        .governance
        .iter()
        .filter(|r| {
            r.is_definition()
                || [
                    "project_configuration",
                    "work_configuration",
                    "integration_project_configuration",
                    "agent_instance",
                    "agent_instance_retirement",
                    "skill_package",
                    "configuration_retirement",
                    "role_retirement",
                    "template_catalog_state",
                    "permission_binding",
                    "permission_assignment",
                ]
                .contains(&r.r#type.as_str())
        })
        .collect();
    records.sort_by(|a, b| a.id.cmp(&b.id));
    configuration_hash(&records.iter().map(|r|json!({"id":r.id,"type":r.r#type,"identity":r.definition_id,"version":r.version,"scope":r.scope,"name":r.name,"refs":r.refs,"data":r.data,"immutable":r.immutable,"revision":r.revision,"supersedes":r.supersedes})).collect::<Vec<_>>())
}
impl Engine {
    pub fn configuration_diagnostics(
        &self,
        s: &EngineeringState,
        records: &[ConfigurationRecord],
    ) -> Vec<ConfigurationDiagnostic> {
        let mut errors = Vec::new();
        for r in records {
            match configuration_edges(r) {
                Err(e) => errors.push(diagnostic(r, "/", None, e)),
                Ok(edges) => {
                    for edge in edges {
                        let check = (|| {
                            let target = resolve_configuration_edge(s, &edge)?;
                            require(target.r#type == edge.expected_type, "引用目标类型不匹配")?;
                            require(
                                if edge.expected_type == "role" {
                                    configuration_visible(s, target, r.scope.project_id())
                                } else {
                                    target.scope.visible_from(&r.scope)
                                },
                                "引用目标在源范围不可见",
                            )?;
                            require(!configuration_retired(s, target), "引用目标已停用")
                        })();
                        if let Err(e) = check {
                            errors.push(diagnostic(r, &edge.path, Some(edge.to), e));
                        }
                    }
                }
            }
            let check = (|| {
                if r.r#type == "tool" && r.data["integration_ref"].is_string() {
                    r.decode::<ExternalTool>()?.validate()?;
                } else {
                    crate::governance::validate_source_payload(
                        s,
                        &r.r#type,
                        &r.refs,
                        &r.data,
                        &r.scope,
                        Some(r),
                    )?;
                }
                match r.r#type.as_str() {
                    "workflow" => {
                        compile_workflow_sources(
                            self.tool_executor.as_deref(),
                            s,
                            r,
                            &r.scope,
                            None,
                        )?;
                    }
                    "standard" => {
                        standard_snapshot(s, &r.id, &r.scope)?;
                    }
                    "spec" => r.decode::<ConfiguredSpecification>()?.validate()?,
                    "skill" => {
                        require(
                            r.data["body"]
                                .as_str()
                                .is_some_and(|b| !b.trim().is_empty()),
                            "Skill 正文不能为空",
                        )?;
                    }
                    "integration" => r.decode::<IntegrationDefinition>()?.validate()?,
                    "integration_binding" => {
                        r.decode::<IntegrationBinding>()?.validate_origins()?
                    }
                    "tool" if r.data["integration_ref"].is_string() => {
                        r.decode::<ExternalTool>()?.validate()?
                    }
                    "permission_profile" => {
                        let mut p = r.permission_profile()?;
                        p.normalize()?;
                    }
                    _ => {}
                }
                Ok(())
            })();
            if let Err(e) = check {
                errors.push(diagnostic(r, "/data", None, e));
            }
        }
        errors
    }
    pub(crate) fn configuration_candidate(
        &self,
        before: &EngineeringState,
        change: &ConfigurationChangeSet,
        actor: &WorkflowActor,
    ) -> Result<(EngineeringState, Vec<ConfigurationRecord>, Vec<Value>)> {
        require(
            !change.operations.is_empty() && change.operations.len() <= 256,
            "变更集需要 1—256 项操作",
        )?;
        require(
            serde_json::to_vec(change)
                .map_err(|e| err("invalid_params", e.to_string()))?
                .len()
                <= 4 * 1024 * 1024,
            "配置变更集过大",
        )?;
        let mut candidate = before.clone();
        let mut changed = Vec::new();
        let mut touched = BTreeSet::new();
        for operation in &change.operations {
            let (source, base) = match operation {
                ConfigurationOperation::Create { source } => {
                    require(
                        !before
                            .governance
                            .iter()
                            .any(|r| r.is_definition() && r.definition_id == source.identity),
                        "配置身份已存在，请创建后继修订",
                    )?;
                    (source, None)
                }
                ConfigurationOperation::Revise {
                    base_ref,
                    expected_revision,
                    source,
                } => {
                    let old = before.configuration(base_ref)?;
                    require(
                        old.immutable && old.is_definition() && old.revision == *expected_revision,
                        "基准修订已变化",
                    )?;
                    require(
                        !configuration_retired(before, old) && template_record_current(before, old),
                        "停用或归档的修订不能直接编辑",
                    )?;
                    require(
                        !before.governance.iter().any(|r| {
                            r.r#type == old.r#type
                                && r.definition_id == old.definition_id
                                && r.version > old.version
                        }),
                        "身份已有更新修订，请重新合并",
                    )?;
                    require(
                        source.identity == old.definition_id
                            && source.r#type == old.r#type
                            && source.scope == old.scope
                            && old.version.checked_add(1) == Some(source.revision),
                        "后继修订必须保持身份、类型、范围并递增一次",
                    )?;
                    (source, Some(old))
                }
                _ => continue,
            };
            require(
                touched.insert(source.identity.clone()),
                "同一变更集不能重复编辑一个身份",
            )?;
            let mut record = source.clone().into_record()?;
            require(
                record.is_definition()
                    && !["permission_binding", "permission_assignment"]
                        .contains(&record.r#type.as_str()),
                "只能发布配置定义；权限绑定必须走绑定用例",
            )?;
            let expected_scope = self
                .store
                .project_id()
                .map_or(Scope::global(), Scope::project);
            require(
                record.scope == expected_scope
                    || (expected_scope.is_global() && record.r#type == "role"),
                "写入范围与当前配置库不匹配",
            )?;
            record.supersedes = base.map(|r| r.id.clone());
            require(
                !candidate.governance.iter().any(|r| r.id == record.id),
                "源修订已存在",
            )?;
            candidate.governance.push(record.clone());
            changed.push(record);
        }
        candidate.refresh_configuration_views()?;
        let errors = self.configuration_diagnostics(&candidate, &changed);
        require(errors.is_empty(), serde_json::to_string(&errors).unwrap())?;
        let retiring: BTreeSet<_> = change
            .operations
            .iter()
            .filter_map(|op| {
                if let ConfigurationOperation::Retire { reference, .. } = op {
                    candidate
                        .configuration(reference)
                        .ok()
                        .map(|r| r.definition_id.clone())
                } else {
                    None
                }
            })
            .collect();
        let mut results = Vec::new();
        let mut ordered: Vec<_> = change.operations.iter().enumerate().collect();
        ordered.sort_by_key(|(_, op)| matches!(op, ConfigurationOperation::Retire { .. }));
        for (index, operation) in ordered {
            match operation {
                ConfigurationOperation::Retire {
                    reference,
                    expected_revision,
                    reason,
                } => {
                    let source = candidate.configuration(reference)?.clone();
                    require(
                        source.immutable
                            && source.is_definition()
                            && source.revision == *expected_revision,
                        "停用目标已变化",
                    )?;
                    require(!reason.trim().is_empty(), "停用需要理由")?;
                    require(
                        source.scope.project_id() == self.store.project_id().as_deref()
                            || (source.r#type == "role" && self.store.project_id().is_none()),
                        "停用范围与当前配置库不匹配",
                    )?;
                    let mut users = configuration_identity_usage(&candidate, reference)?;
                    users.retain(|v| {
                        v["reference"]
                            .as_str()
                            .and_then(|id| candidate.configuration(id).ok())
                            .is_none_or(|r| !retiring.contains(&r.definition_id))
                    });
                    require(users.is_empty(), format!("定义仍被引用: {}", json!(users)))?;
                    let mut retired = crate::governance::record(
                        if source.r#type == "role" {
                            "role_retirement"
                        } else {
                            "configuration_retirement"
                        },
                        if source.r#type == "role" {
                            source.definition_id.clone()
                        } else {
                            reference.clone()
                        },
                        source.scope.clone(),
                        source.name.clone(),
                        1,
                        None,
                        json!({"reason":reason}),
                        true,
                    );
                    retired.id = format!("retired:{}", reference);
                    if !candidate.governance.iter().any(|r| r.id == retired.id) {
                        candidate.governance.push(retired);
                    }
                    results.push(json!({"reference":reference,"retired":true}));
                }
                ConfigurationOperation::ManageInstance {
                    action,
                    reference,
                    expected_revision,
                    name,
                    agent_ref,
                } => {
                    require(
                        ["create", "rebind", "remove"].contains(&action.as_str()),
                        "未知实例操作",
                    )?;
                    let command = Command {
                        command_id: format!(
                            "configuration-instance:{}:{index}",
                            configuration_hash(change)
                        ),
                        target: reference.clone(),
                        expected_revision: *expected_revision,
                        payload: json!({"scope":self.store.project_id().map_or(Scope::global(),Scope::project),"name":name,"agent_revision_id":agent_ref}),
                    };
                    let value = self.apply_command(
                        &format!("agent.instance.{action}"),
                        &command,
                        &mut candidate,
                        actor,
                    )?;
                    results.push(value);
                }
                ConfigurationOperation::PermissionBindings { bindings } => {
                    let backend = self
                        .configuration_backend
                        .as_ref()
                        .ok_or_else(|| err("unsupported", "未装配权限后端校验"))?;
                    let expected = self
                        .store
                        .project_id()
                        .map_or(Scope::global(), Scope::project);
                    let bindings = bindings
                        .iter()
                        .map(|b| {
                            let id = b["target"]["revision_id"].as_str().unwrap_or("");
                            require(
                                candidate.configuration(id)?.scope == expected,
                                "权限绑定不能跨配置库",
                            )?;
                            backend.prepare_permission_binding(&candidate, b)
                        })
                        .collect::<Result<Vec<_>>>()?;
                    results.push(apply_permission_bindings(&mut candidate, &bindings)?);
                }
                ConfigurationOperation::ImportSkill {
                    name,
                    package,
                    source_revision_id,
                } => {
                    let command = Command {
                        command_id: format!(
                            "configuration-skill:{}:{index}",
                            configuration_hash(change)
                        ),
                        target: None,
                        expected_revision: None,
                        payload: json!({"scope":self.store.project_id().map_or(Scope::global(),Scope::project),"name":name,"package":package,"source_revision_id":source_revision_id}),
                    };
                    results.push(self.apply_command(
                        "skill.import",
                        &command,
                        &mut candidate,
                        actor,
                    )?);
                }
                ConfigurationOperation::SelectProject {
                    project_id,
                    expected_revision,
                    selection,
                }
                | ConfigurationOperation::SelectWork {
                    owner_id: project_id,
                    expected_revision,
                    selection,
                } => {
                    let method =
                        if matches!(operation, ConfigurationOperation::SelectProject { .. }) {
                            "project.configuration.update"
                        } else {
                            "work.configuration.update"
                        };
                    let command = Command {
                        command_id: format!("selection:{}:{index}", configuration_hash(change)),
                        target: Some(project_id.clone()),
                        expected_revision: Some(*expected_revision),
                        payload: selection.clone(),
                    };
                    let value = self.apply_command(method, &command, &mut candidate, actor)?;
                    self.governance_after_command(method, &command, &mut candidate, &value)?;
                    results.push(value);
                }
                _ => {}
            }
        }
        let generated: Vec<_> = candidate
            .governance
            .iter()
            .filter(|r| {
                !before.governance.iter().any(|old| old.id == r.id)
                    && !changed.iter().any(|source| source.id == r.id)
            })
            .cloned()
            .collect();
        let errors = self.configuration_diagnostics(&candidate, &generated);
        require(errors.is_empty(), serde_json::to_string(&errors).unwrap())?;
        register_template_members(&mut candidate, template_catalog_generation(before));
        candidate.validate_governance(before)?;
        Ok((candidate, changed, results))
    }
    pub fn preview_configuration_change(&self, change: &ConfigurationChangeSet) -> Result<Value> {
        self.preview_configuration_change_as(change, &WorkflowActor::Human)
    }
    pub(crate) fn preview_configuration_change_as(
        &self,
        change: &ConfigurationChangeSet,
        actor: &WorkflowActor,
    ) -> Result<Value> {
        let before = self.store.snapshot()?;
        self.configuration_preview_in(&before, change, actor)
    }
    fn configuration_preview_in(
        &self,
        before: &EngineeringState,
        change: &ConfigurationChangeSet,
        actor: &WorkflowActor,
    ) -> Result<Value> {
        let (candidate, changed, selections) =
            self.configuration_candidate(before, change, actor)?;
        let base = configuration_fingerprint(before);
        let affected: Vec<_> = changed
            .iter()
            .filter_map(|r| r.supersedes.as_deref())
            .map(|id| {
                configuration_usage(before, id).map(|users| json!({"reference":id,"users":users}))
            })
            .collect::<Result<_>>()?;
        // Preview authority survives another turn in the same frozen session. The
        // actual apply receipt still records its exact turn/tool invocation.
        let mut principal = json!(actor);
        if let Some(object) = principal.as_object_mut() {
            object.remove("turn_id");
        }
        let hash = configuration_hash(
            &json!({"base":base,"changeset":change,"result":configuration_fingerprint(&candidate),"actor":principal}),
        );
        Ok(
            json!({"valid":true,"structure_valid":true,"composition_valid":true,"runtime_readiness":"not_probed","base_fingerprint":base,"preview_hash":hash,"published_refs":changed.iter().map(|r|&r.id).collect::<Vec<_>>(),"affected":affected,"selections":selections,"changeset":change}),
        )
    }
    pub(crate) fn configuration_change_command(
        &self,
        method: &str,
        c: &Command,
        s: &mut EngineeringState,
        actor: &WorkflowActor,
    ) -> Result<Option<Value>> {
        if method != "configuration.change.apply" {
            return Ok(None);
        }
        let change: ConfigurationChangeSet = serde_json::from_value(c.payload["changeset"].clone())
            .map_err(|e| err("invalid_params", e.to_string()))?;
        let preview = self.configuration_preview_in(s, &change, actor)?;
        require(
            c.payload["preview_hash"] == preview["preview_hash"],
            "配置或授权已经变化，请重新预演",
        )?;
        let (candidate, _, _) = self.configuration_candidate(s, &change, actor)?;
        *s = candidate;
        Ok(Some(
            json!({"id":c.command_id,"accepted":true,"published_refs":preview["published_refs"],"selections":preview["selections"]}),
        ))
    }
    pub(crate) fn configuration_change_query(
        &self,
        method: &str,
        p: &Value,
    ) -> Option<Result<Value>> {
        match method {
            "configuration.change.preview" => Some((|| {
                let change =
                    serde_json::from_value(p.get("changeset").cloned().unwrap_or(Value::Null))
                        .map_err(|e| err("invalid_params", e.to_string()))?;
                self.preview_configuration_change(&change)
            })()),
            "query.configuration_graph" => Some((|| {
                let s = self.store.snapshot()?;
                let id = text(p, "id")?;
                let record = s.configuration(&id)?;
                Ok(
                    json!({"record":record,"edges":configuration_edges(record)?,"usage":configuration_usage(&s,&id)?,"diagnostics":self.configuration_diagnostics(&s,&[record.clone()])}),
                )
            })()),
            _ => None,
        }
    }
}
pub fn configuration_usage(state: &EngineeringState, reference: &str) -> Result<Vec<Value>> {
    let mut users = Vec::new();
    for record in &state.governance {
        if ![
            "project_configuration",
            "work_configuration",
            "integration_project_configuration",
            "agent_instance",
            "skill_package",
            "permission_binding",
        ]
        .contains(&record.r#type.as_str())
            && !record.is_definition()
        {
            continue;
        }
        if configuration_retired(state, record)
            || state.governance.iter().any(|r| {
                r.r#type == record.r#type
                    && r.definition_id == record.definition_id
                    && r.version > record.version
            })
        {
            continue;
        }
        for edge in configuration_edges(record)? {
            if resolve_configuration_edge(state, &edge).is_ok_and(|r| r.id == reference) {
                users.push(json!({"id":record.id,"reference":record.id,"name":record.name,"scope":record.scope,"path":edge.path,"type":record.r#type}));
            }
        }
    }
    Ok(users)
}

pub fn configuration_identity_usage(
    state: &EngineeringState,
    reference: &str,
) -> Result<Vec<Value>> {
    let source = state.configuration(reference)?;
    let mut users = Vec::new();
    for r in state
        .governance
        .iter()
        .filter(|r| r.r#type == source.r#type && r.definition_id == source.definition_id)
    {
        users.extend(configuration_usage(state, &r.id)?);
    }
    users.sort_by_key(|v| v.to_string());
    users.dedup();
    Ok(users)
}

/// Caller supplies authoritative read-only project snapshots before entering the write transaction.
pub fn check_configuration_retirements(
    change: &ConfigurationChangeSet,
    project_name: &str,
    state: &EngineeringState,
) -> Result<()> {
    for op in &change.operations {
        if let ConfigurationOperation::Retire { reference, .. } = op {
            let identity = parse_source_reference(reference)?.0;
            let targets: Vec<_> = state
                .governance
                .iter()
                .filter(|r| r.is_definition() && r.definition_id == identity)
                .map(|r| r.id.clone())
                .collect();
            for id in targets {
                require(
                    configuration_usage(state, &id)?.is_empty(),
                    format!("项目 {project_name} 仍使用 {id}"),
                )?;
            }
        }
    }
    Ok(())
}

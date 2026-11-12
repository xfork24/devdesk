//! Canonical source revisions and transient execution views.
use crate::*;

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct SourceDefinition {
    pub kind: String,
    pub identity: String,
    pub revision: u64,
    pub name: String,
    pub scope: Scope,
    pub data: Value,
}

pub fn source_reference(identity: &str, revision: u64) -> Result<String> {
    require(
        !identity.is_empty()
            && identity
                .chars()
                .all(|c| !c.is_whitespace() && !c.is_control())
            && revision > 0,
        "源定义需要明确的 identity 和正整数 revision",
    )?;
    Ok(format!("{identity}:{revision}"))
}

pub fn parse_source_reference(reference: &str) -> Result<(&str, u64)> {
    let (identity, revision) = reference
        .rsplit_once(':')
        .ok_or_else(|| err("invalid_reference", "引用必须为 identity:revision"))?;
    let revision = revision
        .parse::<u64>()
        .map_err(|_| err("invalid_reference", "引用修订必须为正整数"))?;
    require(
        source_reference(identity, revision)? == reference,
        "引用必须使用规范的 identity:revision",
    )?;
    Ok((identity, revision))
}

impl SourceDefinition {
    pub fn into_record(self) -> Result<ConfigurationRecord> {
        validate_source_data(&self.kind, &self.data)?;
        let id = source_reference(&self.identity, self.revision)?;
        Ok(ConfigurationRecord {
            id,
            kind: self.kind,
            definition_id: self.identity,
            scope: self.scope,
            name: self.name,
            version: self.revision,
            supersedes: None,
            content_hash: configuration_hash(&self.data),
            data: self.data,
            immutable: true,
            revision: 1,
            created_at: 0,
        })
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct PermissionContextDefinition {
    pub sandbox_ref: Id,
    pub harness: PermissionSelection,
    pub sandbox: PermissionSelection,
    #[serde(default)]
    pub step_grants: Vec<StepPermissionGrant>,
    pub control_policy: WorkflowControlPolicy,
}
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct StepPermissionGrant {
    pub workflow_ref: Id,
    pub step: String,
    #[serde(default)]
    pub writable_paths: Vec<String>,
    #[serde(default)]
    pub read_only_paths: Vec<String>,
    #[serde(default)]
    pub tool_refs: Vec<Id>,
}

pub fn validate_source_data(kind: &str, data: &Value) -> Result<()> {
    let invalid = |e: serde_json::Error| err("invalid_source", e.to_string());
    match kind {
        "agent" => {
            let a: AgentDefinition = serde_json::from_value(data.clone()).map_err(invalid)?;
            parse_source_reference(&a.role_ref)?;
            parse_source_reference(&a.capability_ref)?;
        }
        "capability" => {
            let cap: CapabilityDefinition =
                serde_json::from_value(data.clone()).map_err(invalid)?;
            for reference in [&cap.model_ref, &cap.harness_ref]
                .into_iter()
                .chain(&cap.skills)
                .chain(&cap.tools)
            {
                parse_source_reference(reference)?;
            }
            require(
                cap.skills
                    .iter()
                    .collect::<std::collections::BTreeSet<_>>()
                    .len()
                    == cap.skills.len()
                    && cap
                        .tools
                        .iter()
                        .collect::<std::collections::BTreeSet<_>>()
                        .len()
                        == cap.tools.len(),
                "Capability 引用不得重复",
            )?;
        }
        "permission_profile" => {
            require(
                data.as_object().is_some_and(|v| {
                    v.keys()
                        .all(|k| ["schema_version", "target_kind", "policy"].contains(&k.as_str()))
                }),
                "权限正文不能重复身份或修订元数据",
            )?;
        }
        "model_provider" => {
            let p: ProviderFacts = serde_json::from_value(data.clone()).map_err(invalid)?;
            require(
                data["models"].as_array().is_none_or(|m| m.is_empty()),
                "模型容量属于 Harness 目录，不在服务商源中声明",
            )?;
            p.validate()?;
        }
        "model_catalog" => {
            let catalog: ModelCatalogDefinition =
                serde_json::from_value(data.clone()).map_err(invalid)?;
            catalog.validate()?;
        }
        "harness" => {
            let h: HarnessDefinition = serde_json::from_value(data.clone()).map_err(invalid)?;
            let mut refs = std::collections::BTreeSet::new();
            for reference in &h.model_providers {
                parse_source_reference(reference)?;
                require(refs.insert(reference.clone()), "Harness 服务商引用重复")?;
            }
            if let Some(catalog_ref) = &h.model_catalog_ref {
                parse_source_reference(catalog_ref)?;
            }
        }
        "permission_context" => {
            let context: PermissionContextDefinition =
                serde_json::from_value(data.clone()).map_err(invalid)?;
            parse_source_reference(&context.sandbox_ref)?;
            context.control_policy.validate()?;
            let mut grants = std::collections::BTreeSet::new();
            for grant in &context.step_grants {
                parse_source_reference(&grant.workflow_ref)?;
                require(
                    !grant.step.is_empty() && grants.insert((&grant.workflow_ref, &grant.step)),
                    "步骤授权需要唯一的 Workflow 修订和步骤",
                )?;
            }
        }
        "standard" => {
            let standard: ConfiguredStandard =
                serde_json::from_value(data.clone()).map_err(invalid)?;
            standard.validate()?;
        }
        "workflow" => {
            for step in data["steps"].as_array().into_iter().flatten() {
                require(
                    step["access"].as_object().is_none_or(|access| {
                        access
                            .keys()
                            .all(|k| ["workspace_input", "working_directory"].contains(&k.as_str()))
                    }),
                    "Workflow 只描述任务上下文；读写及工具授权必须放在 Permission",
                )?;
            }
        }
        _ => {}
    }
    Ok(())
}

impl ConfigurationRecord {
    pub fn source(&self) -> SourceDefinition {
        SourceDefinition {
            kind: self.kind.clone(),
            identity: self.definition_id.clone(),
            revision: self.version,
            name: self.name.clone(),
            scope: self.scope.clone(),
            data: self.data.clone(),
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct AgentDefinition {
    pub role_ref: Id,
    pub capability_ref: Id,
}

impl EngineeringState {
    pub fn resolve_agent(&self, reference: &str) -> Result<AgentComposition> {
        let record = self.configuration(reference)?;
        require(record.kind == "agent", "需要 Agent 源修订")?;
        let agent: AgentDefinition = record.decode()?;
        let role = self.configuration(&agent.role_ref)?;
        let capability = self.configuration(&agent.capability_ref)?;
        require(
            role.kind == "role" && capability.kind == "capability",
            "Agent 引用类型不匹配",
        )?;
        let role_data: RoleDefinition = role.decode()?;
        let cap: CapabilityDefinition = capability.decode()?;
        Ok(AgentComposition {
            role_ref: Some(agent.role_ref),
            capability_ref: Some(agent.capability_ref),
            role: role.definition_id.clone(),
            instructions: role_data.instructions,
            model_ref: cap.model_ref,
            harness_ref: cap.harness_ref,
            skills: cap.skills,
            tools: cap.tools,
        })
    }

    /// These vectors are request-local indexes, never independent definitions.
    pub fn refresh_configuration_views(&mut self) -> Result<()> {
        self.definitions.clear();
        self.harnesses.clear();
        self.sandboxes.clear();
        for r in &self.governance {
            match r.kind.as_str() {
                "skill" => self.definitions.push(Definition {
                    id: r.id.clone(),
                    family: "skill".into(),
                    name: r.name.clone(),
                    version: r.version,
                    supersedes: r.supersedes.clone(),
                    body: r.data["body"].as_str().unwrap_or("").into(),
                    agent_profile: None,
                }),
                "agent" => {
                    let a = self.resolve_agent(&r.id)?;
                    self.definitions.push(Definition {
                        id: r.id.clone(),
                        family: "agent".into(),
                        name: r.name.clone(),
                        version: r.version,
                        supersedes: r.supersedes.clone(),
                        body: a.instructions,
                        agent_profile: Some(AgentProfile {
                            role: a.role,
                            skill_revisions: a.skills,
                        }),
                    });
                }
                "harness" => {
                    let h: HarnessDefinition = r.decode()?;
                    self.harnesses.push(Harness {
                        id: r.id.clone(),
                        provider: h.provider,
                        version: h.runtime_version,
                        capabilities: h.capabilities,
                        origin: Origin::External,
                        permission: None,
                    });
                }
                "sandbox" => {
                    let b: SandboxDefinition = r.decode()?;
                    self.sandboxes.push(Sandbox {
                        id: r.id.clone(),
                        provider: b.provider,
                        version: b.enforcement,
                        platform: "linux".into(),
                        image: None,
                        network: true,
                        origin: Origin::External,
                        permission: None,
                    });
                }
                _ => {}
            }
        }
        Ok(())
    }
}

impl EngineeringState {
    pub fn resolve_workflow(&self, reference: &str) -> Result<WorkflowDefinition> {
        let r = self.configuration(reference)?;
        self.resolve_workflow_source(r)
    }
    pub fn resolve_workflow_source(&self, r: &ConfigurationRecord) -> Result<WorkflowDefinition> {
        let reference = &r.id;
        let mut doc: WorkflowDefinition = r.decode()?;
        if let Some(context) = permission_context(self) {
            for grant in context.data["step_grants"].as_array().into_iter().flatten() {
                let selection = self
                    .governance
                    .iter()
                    .filter(|r| r.kind == "project_configuration")
                    .max_by_key(|r| r.version);
                let selected = selection
                    .filter(|r| r.data["workflow_ref"].as_str() == Some(reference))
                    .and_then(|r| r.data["permission_workflow_ref"].as_str())
                    .unwrap_or(reference);
                if grant["workflow_ref"].as_str() != Some(selected) {
                    continue;
                }
                let Some(step) = doc
                    .steps
                    .iter_mut()
                    .find(|s| Some(s.key.as_str()) == grant["step"].as_str())
                else {
                    continue;
                };
                step.access.writable_paths =
                    serde_json::from_value(grant["writable_paths"].clone()).unwrap_or_default();
                step.access.read_only_paths =
                    serde_json::from_value(grant["read_only_paths"].clone()).unwrap_or_default();
                step.access.tool_refs =
                    serde_json::from_value(grant["tool_refs"].clone()).unwrap_or_default();
            }
        }
        Ok(doc)
    }
}

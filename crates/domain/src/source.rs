//! Canonical source revisions and transient execution views.
use crate::*;

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct SourceDefinition {
    pub r#type: String,
    pub identity: String,
    pub revision: u64,
    pub name: String,
    pub scope: Scope,
    #[serde(flatten)]
    pub refs: References,
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
        validate_source_data(&self.r#type, &self.refs, &self.data)?;
        let id = source_reference(&self.identity, self.revision)?;
        Ok(ConfigurationRecord {
            id,
            r#type: self.r#type,
            definition_id: self.identity,
            scope: self.scope,
            name: self.name,
            version: self.revision,
            supersedes: None,
            content_hash: content_digest(&self.refs, &self.data),
            refs: self.refs,
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

/// Flattened references must stay within the type's allowlist and use canonical
/// `identity:revision` strings; lists never repeat a revision.
pub fn validate_references(kind: &str, refs: &References) -> Result<()> {
    let allowed = reference_fields(kind);
    require(
        refs.keys().all(|key| allowed.contains(&key.as_str())),
        "引用字段不属于该定义类型",
    )?;
    for (key, value) in refs {
        let list = [
            "skills",
            "tools",
            "spec_refs",
            "model_providers",
            "root_workflow_refs",
        ]
        .contains(&key.as_str());
        require(
            if list {
                value.is_array()
            } else {
                value.is_string()
            },
            format!("{key} 引用形状不匹配"),
        )?;
        match value {
            Value::String(reference) => {
                parse_source_reference(reference)?;
            }
            Value::Array(items) => {
                let mut seen = std::collections::BTreeSet::new();
                for item in items {
                    let reference = item
                        .as_str()
                        .ok_or_else(|| err("invalid_source", format!("{key} 引用必须为字符串")))?;
                    parse_source_reference(reference)?;
                    require(seen.insert(reference), format!("{key} 引用不得重复"))?;
                }
            }
            _ => {
                return Err(err(
                    "invalid_source",
                    format!("{key} 引用必须为字符串或列表"),
                ));
            }
        }
    }
    Ok(())
}

pub fn validate_source_data(kind: &str, refs: &References, data: &Value) -> Result<()> {
    let invalid = |e: serde_json::Error| err("invalid_source", e.to_string());
    validate_references(kind, refs)?;
    match kind {
        "agent" => {
            require(
                data.as_object().is_some_and(|data| data.is_empty()),
                "Agent data 必须为空对象；引用属于信封根级",
            )?;
            reference_id(refs, "role_ref")?;
            reference_id(refs, "capability_ref")?;
        }
        "capability" => {
            require(
                data.as_object().is_some_and(|data| data.is_empty()),
                "Capability data 必须为空对象；引用属于信封根级",
            )?;
            reference_list(refs, "skills")?;
            reference_list(refs, "tools")?;
            reference_id(refs, "model_ref")?;
            reference_id(refs, "harness_ref")?;
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
            serde_json::from_value::<HarnessDefinition>(data.clone()).map_err(invalid)?;
        }
        "permission_context" => {
            let context: PermissionContextDefinition =
                serde_json::from_value(data.clone()).map_err(invalid)?;
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
            let roots = reference_list(refs, "root_workflow_refs")?;
            require(
                !roots.is_empty() && roots.len() <= 100,
                "Standard 需要 1—100 个 Workflow 入口",
            )?;
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
            r#type: self.r#type.clone(),
            identity: self.definition_id.clone(),
            revision: self.version,
            name: self.name.clone(),
            scope: self.scope.clone(),
            refs: self.refs.clone(),
            data: self.data.clone(),
        }
    }
}

impl EngineeringState {
    pub fn resolve_agent(&self, reference: &str) -> Result<AgentComposition> {
        let record = self.configuration(reference)?;
        require(record.r#type == "agent", "需要 Agent 源修订")?;
        let role_reference = reference_id(&record.refs, "role_ref")?;
        let capability_reference = reference_id(&record.refs, "capability_ref")?;
        let role = self.configuration(&role_reference)?;
        let capability = self.configuration(&capability_reference)?;
        require(
            role.r#type == "role" && capability.r#type == "capability",
            "Agent 引用类型不匹配",
        )?;
        let role_data: RoleDefinition = role.decode()?;
        Ok(AgentComposition {
            role_ref: Some(role_reference),
            capability_ref: Some(capability_reference),
            role: role.definition_id.clone(),
            instructions: role_data.instructions,
            model_ref: reference_id(&capability.refs, "model_ref")?,
            harness_ref: reference_id(&capability.refs, "harness_ref")?,
            skills: reference_list(&capability.refs, "skills")?,
            tools: reference_list(&capability.refs, "tools")?,
        })
    }

    /// These vectors are request-local indexes, never independent definitions.
    pub fn refresh_configuration_views(&mut self) -> Result<()> {
        self.definitions.clear();
        self.harnesses.clear();
        self.sandboxes.clear();
        for r in &self.governance {
            match r.r#type.as_str() {
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
                    .filter(|r| r.r#type == "project_configuration")
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

#[cfg(test)]
mod envelope_tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn flattened_references_validate_shape_payload_and_content_digest() {
        let source = serde_json::json!({"type":"capability","identity":"cap","revision":1,"name":"Cap","scope":"global","harness_ref":"h:1","model_ref":"m:1","skills":[],"tools":["t:1"],"data":{}});
        let decode = |value| {
            serde_json::from_value::<SourceDefinition>(value)
                .unwrap()
                .into_record()
        };
        let record = decode(source.clone()).unwrap();
        assert_eq!(
            record.source(),
            serde_json::from_value::<SourceDefinition>(source.clone()).unwrap()
        );
        assert_eq!(
            ConfigurationRecord::from_storage(record.to_storage()).unwrap(),
            record
        );
        let mut changed = source.clone();
        changed["model_ref"] = json!("m:2");
        assert_ne!(record.content_hash, decode(changed).unwrap().content_hash);
        for (key, value) in [
            ("model_ref", json!(["m:1"])),
            ("skills", json!("s:1")),
            ("tools", json!(["t:1", "t:1"])),
            ("tools", json!(["t:0"])),
            ("role_ref", json!("r:1")),
            ("data", json!({"model_ref":"m:2"})),
        ] {
            let mut invalid = source.clone();
            invalid[key] = value;
            assert!(decode(invalid).is_err(), "accepted invalid {key}");
        }
    }

    #[test]
    fn scope_encoding_rejects_malformed_locators_and_preserves_visibility() {
        for scope in [
            "",
            "project:",
            "feature:p",
            "project:p:extra",
            "feature:p:",
            "project:has space",
        ] {
            assert!(
                serde_json::from_value::<Scope>(json!(scope)).is_err(),
                "accepted {scope}"
            );
        }
        let project: Scope = serde_json::from_value(json!("project:p")).unwrap();
        let feature: Scope = serde_json::from_value(json!("feature:p:f")).unwrap();
        assert!(project.visible_from(&feature));
        assert!(!feature.visible_from(&project));
        assert!(!project.visible_from(&Scope::feature("other", "f")));
        assert_eq!(feature.project_id(), Some("p"));
    }
}

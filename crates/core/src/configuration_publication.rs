//! Publication of a complete configuration candidate. Only configuration
//! records are compared; execution history is never loaded or diffed here.
use crate::{concurrency::*, *};
use std::collections::{BTreeMap, BTreeSet};

pub struct ConfigurationPublication {
    pub read_set: ReadSet,
    pub changes: Vec<ResourceChange>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub(crate) struct ConfigurationWorkFact {
    pub work: WorkReference,
    pub scope: Scope,
    pub owner: ResourceSnapshot,
    pub read_set: ReadSet,
}

/// The caller pins the complete catalog used by its pure configuration rules.
/// Graph-wide validation reads the catalog predicate; exact source heads and
/// changed rows retain their own CAS identities in the same short commit.
pub fn prepare_configuration_publication(
    store: &dyn EngineeringStore,
    before: &ConfigurationCatalogSnapshot,
    candidate: &ConfigurationCatalogSnapshot,
) -> Result<ConfigurationPublication> {
    require(
        before.state.projects.len() == candidate.state.projects.len()
            && before
                .state
                .projects
                .iter()
                .all(|p| candidate.state.projects.iter().any(|n| n.id == p.id)),
        "配置发布不能增加或删除项目",
    )?;
    let projects = candidate
        .state
        .projects
        .iter()
        .filter(|p| {
            before
                .state
                .projects
                .iter()
                .find(|old| old.id == p.id)
                .is_some_and(|old| json!(old) != json!(p))
        })
        .collect::<Vec<_>>();
    let old: BTreeMap<_, _> = before.state.governance.iter().map(|r| (&r.id, r)).collect();
    require(
        old.keys().all(|id| candidate.record(id).is_some()),
        "配置源及目录记录不能删除",
    )?;
    let selected = candidate
        .state
        .governance
        .iter()
        .filter(|r| old.get(&r.id).is_none_or(|v| *v != *r))
        .collect::<Vec<_>>();
    require(selected.len() <= 2048, "单次配置发布超过记录预算")?;
    let mut keys = vec![ResourceKey::new("catalog_clock", "configuration")];
    keys.extend(projects.iter().map(|p| ResourceKey::new("project", &p.id)));
    let mut heads = BTreeMap::new();
    for row in &selected {
        require(catalog_record(row), "候选目录包含运行事实")?;
        if row.is_definition()
            || matches!(
                row.r#type.as_str(),
                "project_configuration" | "integration_project_configuration"
            )
        {
            require(!old.contains_key(&row.id), "已发布源修订不可替换")?;
            keys.push(ResourceKey::new("source", &row.id));
            let head = ResourceKey::new("source_head", &row.definition_id);
            heads
                .entry(head)
                .and_modify(|r: &mut &ConfigurationRecord| {
                    if row.version > r.version {
                        *r = row
                    }
                })
                .or_insert(row);
        } else {
            keys.push(ResourceKey::new("governance", &row.id));
        }
    }
    keys.extend(heads.keys().cloned());
    let rows = store.read_resources(&keys)?;
    require(
        rows.len() == keys.len() && rows.iter().zip(&keys).all(|(r, k)| &r.resource == k),
        "配置发布版本读取不完整",
    )?;
    if rows[0].revision != before.revision {
        return Err(err("revision_conflict", "配置目录已变化，请重新载入并合并"));
    }
    let mut guards = rows
        .iter()
        .map(|r| ResourceVersion {
            resource: r.resource.clone(),
            revision: r.revision,
        })
        .collect::<Vec<_>>();
    let snapshots = rows
        .iter()
        .map(|r| (&r.resource, r))
        .collect::<BTreeMap<_, _>>();
    let mut changes = Vec::new();
    for project in projects {
        let key = ResourceKey::new("project", &project.id);
        let old = before
            .state
            .projects
            .iter()
            .find(|p| p.id == project.id)
            .unwrap();
        require(
            snapshots[&key].revision == old.revision && project.revision == old.revision + 1,
            "项目配置必须按原版本推进一次",
        )?;
        changes.push(ResourceChange {
            resource: key,
            value: json!(project),
        });
    }
    for row in selected {
        if row.is_definition()
            || matches!(
                row.r#type.as_str(),
                "project_configuration" | "integration_project_configuration"
            )
        {
            let key = ResourceKey::new("source", &row.id);
            require(snapshots[&key].value.is_none(), "新源修订已经存在")?;
            changes.push(ResourceChange {
                resource: key,
                value: json!(row.source()),
            });
        } else {
            let key = ResourceKey::new("governance", &row.id);
            let original = snapshots[&key];
            match old.get(&row.id) {
                Some(previous) => require(
                    original.revision == previous.revision,
                    "配置目录原记录已变化",
                )?,
                None => require(original.revision == 0, "配置目录新记录已经存在")?,
            }
            let mut next = row.clone();
            next.revision = original.revision + 1;
            changes.push(ResourceChange {
                resource: key,
                value: store.encode_configuration_record(candidate, &next)?,
            });
        }
    }
    for (head, row) in heads {
        if snapshots[&head].value.as_ref().is_some_and(|current| {
            current["revision"]
                .as_u64()
                .is_some_and(|r| r >= row.version)
        }) {
            continue;
        }
        changes.push(ResourceChange {
            resource: head,
            value: json!({"reference":row.id}),
        });
    }
    // Deduplicate witnesses without refreshing any original version.
    let mut seen = BTreeSet::new();
    guards.retain(|g| seen.insert(g.resource.clone()));
    Ok(ConfigurationPublication {
        read_set: ReadSet::new(guards)?,
        changes,
    })
}

pub fn configuration_transfer_witness(store: &dyn EngineeringStore) -> Result<ResourceVersion> {
    let key = ResourceKey::new("configuration_transfers_active", "configuration");
    let rows = store.read_resources(std::slice::from_ref(&key))?;
    require(
        rows.len() == 1 && rows[0].resource == key,
        "跨库发布谓词读取不完整",
    )?;
    if !rows[0]
        .value
        .as_ref()
        .and_then(Value::as_array)
        .is_some_and(Vec::is_empty)
    {
        return Err(err(
            "configuration_transfer_pending",
            "有待完成的跨库配置操作，请先完成或取消",
        ));
    }
    Ok(ResourceVersion {
        resource: key,
        revision: rows[0].revision,
    })
}

impl Engine {
    pub(crate) fn configuration_work_facts(
        &self,
        change: &ConfigurationChangeSet,
    ) -> Result<BTreeMap<Id, ConfigurationWorkFact>> {
        let mut facts = BTreeMap::new();
        let project = self.store.project_id();
        let mut selected = BTreeSet::new();
        for operation in &change.operations {
            if let ConfigurationOperation::SelectProject { project_id, .. } = operation {
                require(
                    selected.insert(project_id.clone()),
                    "同一变更集不能重复选择工作配置",
                )?;
            }
            let ConfigurationOperation::SelectWork {
                owner_id,
                expected_revision,
                ..
            } = operation
            else {
                continue;
            };
            require(
                selected.insert(owner_id.clone()),
                "同一变更集不能重复选择工作配置",
            )?;
            let project = project
                .as_deref()
                .ok_or_else(|| err("project_required", "工作配置需要项目"))?;
            let keys = [
                ResourceKey::new("project", owner_id),
                ResourceKey::new("feature", owner_id),
                ResourceKey::new("task", owner_id),
                ResourceKey::new("workflows_for_owner", owner_id),
            ];
            let rows = self.store.read_resources(&keys)?;
            require(
                rows.len() == 4 && rows.iter().zip(&keys).all(|(r, k)| &r.resource == k),
                "工作选择原事实读取不完整",
            )?;
            require(
                rows[3].value.as_ref().is_some_and(|v| v["exists"] == false),
                "工作已经开始，请通过计划变更切换规范",
            )?;
            let owners = rows[..3]
                .iter()
                .filter(|r| r.value.is_some())
                .collect::<Vec<_>>();
            require(owners.len() == 1, "工作对象不存在或身份有歧义")?;
            let owner = (*owners[0]).clone();
            let raw = owner.value.as_ref().unwrap();
            if owner.revision != *expected_revision {
                return Err(err(
                    "revision_conflict",
                    "原工作版本已改变，请重新预演并保留草稿",
                ));
            }
            let mut versions = rows
                .iter()
                .map(|r| ResourceVersion {
                    resource: r.resource.clone(),
                    revision: r.revision,
                })
                .collect::<Vec<_>>();
            let (work, scope) = match owner.resource.kind.as_str() {
                "project" => {
                    require(owner_id == project, "工作对象跨项目")?;
                    (
                        WorkReference::Project {
                            id: owner_id.clone(),
                        },
                        Scope::project(project),
                    )
                }
                "feature" => {
                    require(raw["project_id"] == project, "工作对象跨项目")?;
                    (
                        WorkReference::Feature {
                            id: owner_id.clone(),
                        },
                        Scope::feature(project, owner_id),
                    )
                }
                "task" => {
                    let feature = text(raw, "feature_id")?;
                    let parent = self
                        .store
                        .read_resources(&[ResourceKey::new("feature", &feature)])?
                        .into_iter()
                        .next()
                        .ok_or_else(|| err("storage", "任务父对象读取不完整"))?;
                    require(
                        parent
                            .value
                            .as_ref()
                            .is_some_and(|v| v["project_id"] == project),
                        "任务父对象跨项目",
                    )?;
                    versions.push(ResourceVersion {
                        resource: parent.resource,
                        revision: parent.revision,
                    });
                    (
                        WorkReference::Task {
                            id: owner_id.clone(),
                        },
                        Scope::feature(project, &feature),
                    )
                }
                _ => return Err(err("invalid_scope", "无效工作归属")),
            };
            facts.insert(
                owner_id.clone(),
                ConfigurationWorkFact {
                    work,
                    scope,
                    owner,
                    read_set: ReadSet::new(versions)?,
                },
            );
        }
        Ok(facts)
    }

    pub(crate) fn native_configuration_command(
        &self,
        method: &str,
        c: &Command,
        fingerprint: &str,
        context: &InvocationContext,
    ) -> Result<Option<Value>> {
        if method == "work.configuration.update" {
            return self
                .native_work_configuration(c, fingerprint, context)
                .map(Some);
        }
        let (family, action) = method.split_once('.').unwrap_or((method, ""));
        let source = template_definition_kind(family) && matches!(action, "create" | "revise");
        let selected = source
            || matches!(
                method,
                "role.delete"
                    | "configuration.retire"
                    | "agent.instance.create"
                    | "agent.instance.rebind"
                    | "agent.instance.remove"
                    | "permission.profile.create"
                    | "permission.profile.revise"
                    | "permission.profile.import"
                    | "permission.bindings.apply"
                    | "skill.import"
                    | "skill.import_package"
                    | "integration.delete"
                    | "integration.disable"
                    | "integration.retire"
                    | "integration.create"
                    | "integration.revise"
                    | "integration.binding.create"
                    | "integration.binding.revise"
                    | "integration.tool.create"
                    | "integration.tool.revise"
                    | "integration.openapi.import"
                    | "integration.tools.publish"
                    | "integration.project.bind"
                    | "project.configuration.update"
                    | "workflow.publish"
                    | "workflow.draft.create"
                    | "workflow.draft.update"
                    | "standard.capture"
                    | "standard.archive"
                    | "configuration.change.apply"
            );
        if !selected {
            return Ok(None);
        }
        if !matches!(context.actor(), WorkflowActor::Human) {
            return Ok(None);
        }
        if method == "agent.instance.create"
            && (c.payload["frozen_controller_id"].is_string()
                || c.payload["frozen_work_id"].is_string()
                || c.payload["frozen_feature_id"].is_string())
        {
            return Err(err(
                "permission_denied",
                "冻结实例只能由确切正式会话授权创建，不能从普通配置接口指定运行来源",
            ));
        }
        context.require_scope(
            self.store
                .project_id()
                .as_deref()
                .filter(|id| *id != GLOBAL_DISCUSSION_NAMESPACE),
        )?;
        if let Some(receipt) = self.store.replay_commit(&c.command_id, fingerprint)? {
            return Ok(Some(receipt.result));
        }
        let before = self.store.configuration_snapshot()?;
        let transfer = configuration_transfer_witness(self.store.as_ref())?;
        if template_definition_write(method)
            && let Some(target) = c.target.as_deref().and_then(|id| before.record(id))
        {
            require(
                template_record_current(&before.state, target),
                "模板目录已重置，请重新载入后再编辑",
            )?;
        }
        if matches!(
            method,
            "integration.delete" | "integration.disable" | "integration.retire"
        ) {
            let source = before
                .record(c.target.as_deref().unwrap_or(""))
                .ok_or_else(|| err("not_found", "原接入源不存在"))?;
            if c.expected_revision != Some(source.revision) {
                return Err(err(
                    "revision_conflict",
                    "原接入源版本已改变，请保留草稿并刷新",
                ));
            }
            if method != "integration.retire" {
                require(
                    before
                        .state
                        .governance
                        .iter()
                        .filter(|r| {
                            r.r#type == source.r#type && r.definition_id == source.definition_id
                        })
                        .all(|r| r.version <= source.version),
                    "接入源已有新修订，请重新选择",
                )?;
            }
        }
        let mut work = BTreeMap::new();
        let mut next = before.state.as_ref().clone();
        let result = if method == "configuration.change.apply" {
            let change: ConfigurationChangeSet =
                serde_json::from_value(c.payload["changeset"].clone())
                    .map_err(|e| err("invalid_params", e.to_string()))?;
            work = self.configuration_work_facts(&change)?;
            let preview =
                self.configuration_preview_with(&before.state, &change, context.actor(), &work)?;
            require(
                c.payload["preview_hash"] == preview["preview_hash"],
                "配置或授权已变化，请保留草稿并重新预演",
            )?;
            next = self
                .configuration_candidate_with(&before.state, &change, context.actor(), &work)?
                .0;
            json!({"id":c.command_id,"accepted":true,"published_refs":preview["published_refs"],"selections":preview["selections"]})
        } else {
            self.apply_command(method, c, &mut next, context.actor())?
        };
        register_template_members(&mut next, template_catalog_generation(&before.state));
        let candidate =
            ConfigurationCatalogSnapshot::new(next, before.revision, before.repository_ready)?;
        let publication =
            prepare_configuration_publication(self.store.as_ref(), &before, &candidate)?;
        let ctx = context.prepare_record(method, c);
        let mut guards = vec![
            transfer,
            ResourceVersion {
                resource: ctx.resource.clone(),
                revision: 0,
            },
        ];
        let mut changes = publication.changes;
        for fact in work.values() {
            guards.extend(fact.read_set.versions().iter().cloned());
            if fact.owner.resource.kind != "project" {
                let mut raw = fact
                    .owner
                    .value
                    .clone()
                    .ok_or_else(|| err("not_found", "原工作对象缺失"))?;
                raw["revision"] = json!(fact.owner.revision + 1);
                changes.push(ResourceChange {
                    resource: fact.owner.resource.clone(),
                    value: raw,
                });
            }
        }
        changes.push(ctx);
        let receipt = self.store.commit_changes(&PreparedChangeSet {
            command_id: c.command_id.clone(),
            fingerprint: fingerprint.into(),
            read_set: publication.read_set.merge(guards)?,
            changes,
            result,
            time: now() as i64,
        })?;
        Ok(Some(receipt.result))
    }

    fn native_work_configuration(
        &self,
        c: &Command,
        fingerprint: &str,
        context: &InvocationContext,
    ) -> Result<Value> {
        require(
            matches!(context.actor(), WorkflowActor::Human),
            "工作配置选择需要实际 Human 来源",
        )?;
        let project = self
            .store
            .project_id()
            .ok_or_else(|| err("project_required", "工作配置需要明确项目"))?;
        context.require_project(&project)?;
        if let Some(receipt) = self.store.replay_commit(&c.command_id, fingerprint)? {
            return Ok(receipt.result);
        }
        let owner = c
            .target
            .as_deref()
            .ok_or_else(|| err("invalid_params", "缺少工作对象"))?;
        let before = self.store.configuration_snapshot()?;
        let keys = [
            ResourceKey::new("project", owner),
            ResourceKey::new("feature", owner),
            ResourceKey::new("task", owner),
            ResourceKey::new("workflows_for_owner", owner),
        ];
        let rows = self.store.read_resources(&keys)?;
        require(
            rows.len() == 4 && rows.iter().zip(&keys).all(|(r, k)| &r.resource == k),
            "工作配置原事实读取不完整",
        )?;
        require(
            rows[3].value.as_ref().is_some_and(|v| v["exists"] == false),
            "工作已开始，请通过计划变更切换规范",
        )?;
        let owners = rows[..3]
            .iter()
            .filter(|r| r.value.is_some())
            .collect::<Vec<_>>();
        require(owners.len() == 1, "工作对象不存在或身份有歧义")?;
        let row = owners[0];
        let raw = row.value.as_ref().unwrap();
        if c.expected_revision != Some(row.revision) {
            return Err(err(
                "revision_conflict",
                "工作原版本已变化，请保留草稿并刷新",
            ));
        }
        let mut guards = rows
            .iter()
            .map(|r| ResourceVersion {
                resource: r.resource.clone(),
                revision: r.revision,
            })
            .collect::<Vec<_>>();
        let (work, scope) = match row.resource.kind.as_str() {
            "project" => {
                require(owner == project, "工作对象跨项目")?;
                (
                    WorkReference::Project { id: owner.into() },
                    Scope::project(&project),
                )
            }
            "feature" => {
                require(raw["project_id"] == project, "工作对象跨项目")?;
                (
                    WorkReference::Feature { id: owner.into() },
                    Scope::feature(&project, owner),
                )
            }
            "task" => {
                let feature = text(raw, "feature_id")?;
                let parent = self
                    .store
                    .read_resources(&[ResourceKey::new("feature", &feature)])?
                    .into_iter()
                    .next()
                    .ok_or_else(|| err("storage", "任务父对象读取不完整"))?;
                require(
                    parent
                        .value
                        .as_ref()
                        .is_some_and(|v| v["project_id"] == project),
                    "任务父对象跨项目",
                )?;
                guards.push(ResourceVersion {
                    resource: parent.resource,
                    revision: parent.revision,
                });
                (
                    WorkReference::Task { id: owner.into() },
                    Scope::feature(&project, &feature),
                )
            }
            _ => return Err(err("invalid_scope", "无效工作归属")),
        };
        guards.push(configuration_transfer_witness(self.store.as_ref())?);
        let mut next = before.state.as_ref().clone();
        let inherited = crate::standards::work_configuration(&next, owner).cloned();
        let selection = crate::standards::select_work_configuration_for(
            &mut next,
            &work,
            &scope,
            &c.payload,
            inherited.as_ref(),
        )?;
        let selected = crate::standards::selection_state(&next, &selection.data);
        let workflow = next.configuration(selection.data["workflow_ref"].as_str().unwrap())?;
        let bindings = serde_json::from_value(selection.data["role_agent_bindings"].clone())
            .map_err(|e| err("invalid_params", e.to_string()))?;
        self.compile_configured_workflow(&selected, workflow, &selection.scope, Some(&bindings))?;
        register_template_members(&mut next, template_catalog_generation(&before.state));
        let mut owner_next = raw.clone();
        owner_next["revision"] = json!(row.revision + 1);
        if row.resource.kind == "project" {
            let project = next
                .projects
                .iter_mut()
                .find(|p| p.id == owner)
                .ok_or_else(|| err("not_found", "项目配置归属缺失"))?;
            project.revision += 1;
        }
        let candidate =
            ConfigurationCatalogSnapshot::new(next, before.revision, before.repository_ready)?;
        let publication =
            prepare_configuration_publication(self.store.as_ref(), &before, &candidate)?;
        let ctx = context.prepare_record("work.configuration.update", c);
        guards.push(ResourceVersion {
            resource: ctx.resource.clone(),
            revision: 0,
        });
        let read_set = publication.read_set.merge(guards)?;
        let mut changes = publication.changes;
        if row.resource.kind != "project" {
            changes.push(ResourceChange {
                resource: row.resource.clone(),
                value: owner_next,
            });
        }
        changes.push(ctx);
        self.store
            .commit_changes(&PreparedChangeSet {
                command_id: c.command_id.clone(),
                fingerprint: fingerprint.into(),
                read_set,
                changes,
                result: json!(selection),
                time: now() as i64,
            })
            .map(|r| r.result)
    }
}

impl Engine {
    /// Adopt a package's immutable revisions into the project's catalog before starting work.
    pub fn adopt_domain_sources(&self, sources: &[ConfigurationRecord]) -> Result<()> {
        let before = self.store.configuration_snapshot()?;
        let mut candidate = before.state.as_ref().clone();
        for source in sources {
            if let Some(existing) = before.record(&source.id) {
                require(existing.source() == source.source(), "领域源定义修订冲突")?;
            } else {
                candidate.governance.push(source.clone());
            }
        }
        register_template_members(&mut candidate, template_catalog_generation(&before.state));
        candidate.validate_governance(&before.state)?;
        let candidate =
            ConfigurationCatalogSnapshot::new(candidate, before.revision, before.repository_ready)?;
        for source in sources.iter().filter(|source| source.r#type == "workflow") {
            self.compile_configured_workflow(&candidate.state, source, &Scope::global(), None)?;
        }
        let publication =
            prepare_configuration_publication(self.store.as_ref(), &before, &candidate)?;
        if !publication.changes.is_empty() {
            let fingerprint = configuration_hash(&(
                "domain-source-adoption",
                &candidate.fingerprint,
                before.revision,
            ));
            self.store.commit_changes(&PreparedChangeSet {
                command_id: format!("domain-sources:{fingerprint}"),
                fingerprint,
                read_set: publication.read_set,
                changes: publication.changes,
                result: Value::Null,
                time: now() as i64,
            })?;
        }
        Ok(())
    }
}

#[derive(Default)]
pub struct DomainConfiguration {
    pub role_agent_bindings: BTreeMap<Id, Id>,
    pub agent_workflows: BTreeSet<Id>,
}

/// A package's source graph resolves against the platform bundle. Reopening
/// an installed package can also resolve originals retained in its dataset.
pub fn validate_domain_configuration(
    namespace: &str,
    bundle: &ConfigurationBundle,
    installed_catalog: Option<&EngineeringState>,
) -> Result<DomainConfiguration> {
    require(
        configuration_usage_index(&bundle.entries)? == bundle.usage_index
            && bundle.entries.iter().all(|record| {
                record.scope.is_global() && record.definition_id.starts_with(namespace)
            }),
        "领域源定义命名空间或引用索引不符",
    )?;
    let bindings: BTreeMap<Id, Id> = match bundle.defaults.get("role_agent_bindings") {
        Some(value) => serde_json::from_value(value.clone())
            .map_err(|_| err("invalid_domain_package", "领域默认 Role 与 Agent 绑定无效"))?,
        None => BTreeMap::new(),
    };
    let mut catalog = EngineeringState::default();
    initialize_template_catalog(&mut catalog)?;
    // Validate against the same factory permission targets as a fresh Host.
    // These in-memory bindings only check graph closure. Activation resolves
    // the real enforcement contracts from the installed runtime.
    template_permission_defaults(&mut catalog, &|_, _| Ok("domain-package-validation".into()))?;
    if let Some(installed) = installed_catalog {
        // An installed package can reference an exact factory source that the
        // current Host no longer bundles. Only retain original factory content.
        let mut bindings = Vec::new();
        let defaults = current_template_defaults(&catalog)
            .ok_or_else(|| err("invalid_template", "默认模板目录未初始化"))?;
        for source in installed.governance.iter().filter(|source| {
            source.immutable
                && source.scope.is_global()
                && template_catalog_member(installed, source).is_some_and(|member| {
                    member.data["template_key"].as_str() == Some(source.id.as_str())
                        && member.data["source_hash"] == template_source_hash(source)
                })
        }) {
            if catalog.configuration(&source.id).is_ok() {
                continue;
            }
            catalog.governance.push(source.clone());
            if source.r#type == "harness" {
                let target =
                    permission_target(&catalog, PermissionTargetKind::Harness, &source.id)?;
                bindings.push(json!({"target":target,"permission_revision_id":defaults["harness_permission_ref"],"expected_revision":0,"enforcement_contract":"domain-package-validation"}));
            }
        }
        if !bindings.is_empty() {
            apply_permission_bindings(&mut catalog, &bindings)?;
        }
    }
    catalog.governance.extend(bundle.entries.iter().cloned());
    for (role, agent) in &bindings {
        let role = catalog.configuration(role)?;
        let agent = catalog.resolve_agent(agent)?;
        require(
            role.r#type == "role" && agent.role == role.definition_id,
            "领域默认 Agent 必须属于绑定的 Role",
        )?;
    }
    let mut agent_workflows = BTreeSet::new();
    for source in &bundle.entries {
        for edge in configuration_edges(source)? {
            let target = catalog.configuration(&edge.to)?;
            require(target.r#type == edge.expected_type, "领域源引用类型不匹配")?;
        }
        if source.r#type != "workflow" {
            continue;
        }
        let plan =
            compile_workflow_sources(None, &catalog, source, &Scope::global(), Some(&bindings))?;
        if let Some(agent) = plan
            .document
            .orchestrator_role_ref
            .as_ref()
            .and_then(|role| bindings.get(role))
        {
            let composition = catalog.resolve_agent(agent)?;
            crate::governance::validate_composition(&catalog, &composition, &Scope::global())?;
            require(
                composition.tools.iter().any(|tool| {
                    catalog.configuration(tool).is_ok_and(|tool| {
                        tool.data["qualified_name"]
                            .as_str()
                            .is_some_and(|name| crate::controllers::CONTROL_TOOLS.contains(&name))
                    })
                }),
                "领域 Workflow 的编排 Agent 需要明确选择编排工具",
            )?;
            let harness = catalog.configuration(&composition.harness_ref)?;
            require(
                harness.data["provider"]
                    .as_str()
                    .is_some_and(|provider| driver_supports(provider, "orchestration")),
                "领域 Workflow 的编排 Agent 需要支持编排工具的 Harness",
            )?;
            agent_workflows.insert(source.id.clone());
        }
    }
    Ok(DomainConfiguration {
        role_agent_bindings: bindings,
        agent_workflows,
    })
}

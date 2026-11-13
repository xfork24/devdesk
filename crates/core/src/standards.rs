//! Standard publication and consumer selections. No independent quality policy lives here.
use crate::governance::{record, record_with_refs, reference};
use crate::*;
use std::collections::BTreeMap;

pub(crate) fn source_scope(scope: &Scope) -> Scope {
    scope
        .project_id()
        .map(|p| Scope::project(p))
        .unwrap_or(Scope::global())
}

pub(crate) fn standard_archived(state: &EngineeringState, reference: &str) -> bool {
    state
        .governance
        .iter()
        .rev()
        .find(|r| r.r#type == "standard_archive" && r.definition_id == reference)
        .map(|r| r.data["archived"] == true)
        .unwrap_or_else(|| {
            state
                .configuration(reference)
                .is_ok_and(|r| !template_record_current(state, r))
        })
}

pub(crate) fn capture_standard(
    state: &mut EngineeringState,
    mut roots: Vec<Id>,
    scope: &Scope,
    name: &str,
) -> Result<ConfigurationRecord> {
    roots.sort();
    roots.dedup();
    let scope = source_scope(scope);
    let definition = ConfiguredStandard { schema_version: 4 };
    resolve_standard_sources(state, &roots, &scope)?;
    if let Some(existing) = state.governance.iter().find(|r| {
        r.r#type == "standard"
            && r.scope.visible_from(&scope)
            && *reference_value(&r.refs, "root_workflow_refs") == json!(roots)
            && r.data == json!(definition)
            && !standard_archived(state, &r.id)
    }) {
        return Ok(existing.clone());
    }
    let identity = format!(
        "standard.{}",
        configuration_hash(&json!({"scope":scope,"roots":roots}))
    );
    let version = state
        .governance
        .iter()
        .filter(|r| r.r#type == "standard" && r.definition_id == identity)
        .map(|r| r.version)
        .max()
        .unwrap_or(0)
        + 1;
    let row = record_with_refs(
        "standard",
        identity,
        scope,
        name.into(),
        version,
        None,
        References::from([("root_workflow_refs".to_string(), json!(roots))]),
        json!(definition),
        true,
    );
    state.governance.push(row.clone());
    Ok(row)
}

pub(crate) fn work_configuration<'a>(
    state: &'a EngineeringState,
    owner: &str,
) -> Option<&'a ConfigurationRecord> {
    state
        .governance
        .iter()
        .filter(|r| r.r#type == "work_configuration" && r.definition_id == owner)
        .max_by_key(|r| r.version)
}

pub(crate) fn select_work_configuration(
    state: &mut EngineeringState,
    owner: &str,
    payload: &Value,
    inherited: Option<&ConfigurationRecord>,
) -> Result<ConfigurationRecord> {
    let work = work_reference(state, owner)?;
    let scope = work.scope(state)?;
    let data = inherited.map(|r| &r.data).unwrap_or(&Value::Null);
    let workflow = payload["workflow_ref"]
        .as_str()
        .or_else(|| data["workflow_ref"].as_str())
        .ok_or_else(|| err("work_configuration_required", "工作尚未选择入口 Workflow"))?
        .to_owned();
    let standard = payload["standard_ref"]
        .as_str()
        .or_else(|| {
            (payload["workflow_ref"]
                .as_str()
                .is_none_or(|r| Some(r) == data["workflow_ref"].as_str()))
            .then(|| data["standard_ref"].as_str())
            .flatten()
        })
        .map(str::to_owned);
    let standard = if let Some(standard) = standard {
        standard_entry(state, &standard, &workflow, &scope)?;
        standard
    } else {
        capture_standard(state, vec![workflow.clone()], &scope, "规范快照")?.id
    };
    let bindings = payload
        .get("role_agent_bindings")
        .unwrap_or(&data["role_agent_bindings"])
        .clone();
    let bindings = if bindings.is_null() {
        json!({})
    } else {
        bindings
    };
    let context = payload["permission_context_ref"]
        .as_str()
        .or_else(|| data["permission_context_ref"].as_str())
        .map(str::to_owned)
        .or_else(|| permission_context(state).map(|r| r.id.clone()))
        .ok_or_else(|| err("permission_unconfigured", "工作需要执行授权上下文"))?;
    reference(state, &context, "permission_context", &scope)?;
    let old = work_configuration(state, owner).cloned();
    let selection = json!({"schema_version":1,"work":work,"standard_ref":standard,"workflow_ref":workflow,"role_agent_bindings":bindings,"permission_context_ref":context,"source_configuration_ref":inherited.map(|r|json!(r.id)).or_else(||payload.get("source_configuration_ref").cloned())});
    if let Some(old) = &old {
        if old.data == selection {
            return Ok(old.clone());
        }
    }
    let row = record(
        "work_configuration",
        owner.into(),
        scope,
        "采用的规范".into(),
        old.as_ref().map_or(1, |r| r.version + 1),
        old.map(|r| r.id),
        selection,
        true,
    );
    state.governance.push(row.clone());
    Ok(row)
}

/// Compile against the work's frozen execution selection, not the latest project default.
pub(crate) fn selection_state(state: &EngineeringState, selection: &Value) -> EngineeringState {
    let mut selected = state.clone();
    selected.governance.push(record(
        "project_configuration",
        "execution-selection".into(),
        Scope::global(),
        "执行选择".into(),
        u64::MAX,
        None,
        selection.clone(),
        true,
    ));
    selected
}

impl Engine {
    pub(crate) fn standard_command(
        &self,
        method: &str,
        c: &Command,
        state: &mut EngineeringState,
    ) -> Result<Option<Value>> {
        if method == "standard.archive" {
            let source = state
                .configuration(c.target.as_deref().unwrap_or(""))?
                .clone();
            require(
                source.r#type == "standard"
                    && source.scope.project_id() == self.store.project_id().as_deref()
                    && c.expected_revision == Some(source.revision),
                "Standard 类型、维护范围或修订不匹配",
            )?;
            state.governance.push(record(
                "standard_archive",
                source.id.clone(),
                source.scope,
                source.name,
                1,
                None,
                json!({"archived":c.payload["archived"]!=false}),
                true,
            ));
            return Ok(Some(
                json!({"id":source.id,"archived":c.payload["archived"]!=false}),
            ));
        }
        if method == "work.configuration.update" {
            let owner = c
                .target
                .as_deref()
                .ok_or_else(|| err("invalid_params", "缺少工作引用"))?;
            let actual = state
                .projects
                .iter()
                .find(|r| r.id == owner)
                .map(|r| r.revision)
                .or_else(|| {
                    state
                        .features
                        .iter()
                        .find(|r| r.id == owner)
                        .map(|r| r.revision)
                })
                .or_else(|| {
                    state
                        .tasks
                        .iter()
                        .find(|r| r.id == owner)
                        .map(|r| r.revision)
                });
            require(
                actual.is_some() && c.expected_revision == actual,
                "工作修订冲突",
            )?;
            require(
                !state
                    .governance
                    .iter()
                    .any(|r| r.r#type == "workflow_instance" && r.data["owner_id"] == owner),
                "工作已开始，请通过计划变更切换规范",
            )?;
            let inherited = work_configuration(state, owner).cloned();
            let row = select_work_configuration(state, owner, &c.payload, inherited.as_ref())?;
            // Validate runtime compatibility without dispatching anything.
            let selected = selection_state(state, &row.data);
            let workflow = state.configuration(row.data["workflow_ref"].as_str().unwrap())?;
            let bindings: BTreeMap<Id, Id> =
                serde_json::from_value(row.data["role_agent_bindings"].clone())
                    .map_err(|e| err("invalid_params", e.to_string()))?;
            self.compile_configured_workflow(&selected, workflow, &row.scope, Some(&bindings))?;
            if let Some(work) = state.features.iter_mut().find(|r| r.id == owner) {
                work.revision += 1;
            }
            if let Some(work) = state.tasks.iter_mut().find(|r| r.id == owner) {
                work.revision += 1;
            }
            return Ok(Some(json!(row)));
        }
        if method != "standard.capture" {
            return Ok(None);
        }
        let p = &c.payload;
        let mut roots: Vec<Id> = serde_json::from_value(p["root_workflow_refs"].clone())
            .map_err(|e| err("invalid_params", e.to_string()))?;
        roots.sort();
        let scope: Scope = serde_json::from_value(p.get("scope").cloned().unwrap_or_else(|| {
            self.store
                .project_id()
                .map(|p| json!(format!("project:{p}")))
                .unwrap_or(json!("global"))
        }))
        .map_err(|e| err("invalid_params", e.to_string()))?;
        require(
            !scope.is_feature() && scope.project_id() == self.store.project_id().as_deref(),
            "Standard 源可见范围与当前入口不一致",
        )?;
        let definition = ConfiguredStandard { schema_version: 4 };
        let snapshot = resolve_standard_sources(state, &roots, &scope)?;
        if let Some(expected) = p["preview_digest"].as_str() {
            require(expected == snapshot.snapshot_digest, "规范候选已变化")?;
        }
        let name = text(p, "name")?;
        let row = if let Some(base) = c.target.as_deref() {
            let previous = reference(state, base, "standard", &scope)?.clone();
            require(
                previous.scope == scope && c.expected_revision == Some(previous.revision),
                "Standard 修订冲突",
            )?;
            require(
                !state.governance.iter().any(|r| {
                    r.r#type == "standard"
                        && r.definition_id == previous.definition_id
                        && r.version > previous.version
                }),
                "Standard 已有新修订",
            )?;
            let row = record_with_refs(
                "standard",
                previous.definition_id,
                scope,
                name,
                previous.version + 1,
                Some(previous.id),
                References::from([("root_workflow_refs".to_string(), json!(roots.clone()))]),
                json!(definition),
                true,
            );
            state.governance.push(row.clone());
            row
        } else {
            capture_standard(state, roots, &scope, &name)?
        };
        Ok(Some(json!(row)))
    }

    pub(crate) fn standard_query(&self, method: &str, p: &Value) -> Option<Result<Value>> {
        if ![
            "standard.capture.preview",
            "query.standard",
            "query.standard_diff",
            "query.standard_usage",
            "query.work_standard_diff",
            "query.work_configuration",
            "query.standard_catalog",
        ]
        .contains(&method)
        {
            return None;
        }
        Some((|| {
            let state = self.store.snapshot()?;
            let scope = self
                .store
                .project_id()
                .or_else(|| p["project_id"].as_str().map(str::to_owned))
                .map(|p| Scope::project(p))
                .unwrap_or(Scope::global());
            let limit = p["limit"].as_u64().unwrap_or(100).clamp(1, 1000) as usize;
            let offset = p["offset"].as_u64().unwrap_or(0) as usize;
            if method == "query.standard_catalog" {
                let rows = state
                    .governance
                    .iter()
                    .filter(|r| r.r#type == "standard" && r.scope.visible_from(&scope))
                    .filter(|r| p["include_archived"] == true || !standard_archived(&state, &r.id))
                    .map(|r| {
                        let mut value = json!(r);
                        value["archived"] = json!(standard_archived(&state, &r.id));
                        value["workflow_refs"] = json!(
                            standard_snapshot(&state, &r.id, &scope)?
                                .nodes
                                .into_iter()
                                .filter(|n| n.kind == "workflow")
                                .map(|n| n.reference)
                                .collect::<Vec<_>>()
                        );
                        Ok(value)
                    })
                    .collect::<Result<Vec<_>>>()?;
                return Ok(json!({"records":rows}));
            }
            if method == "query.work_configuration" {
                let owner = text(p, "owner_id")?;
                let work = work_reference(&state, &owner)?;
                require(
                    scope.project_id().is_none_or(|project| {
                        work.scope(&state)
                            .ok()
                            .and_then(|s| s.project_id().map(str::to_owned))
                            .as_deref()
                            == Some(project)
                    }),
                    "工作跨项目",
                )?;
                return Ok(
                    json!({"configuration":work_configuration(&state,&owner),"history":state.governance.iter().filter(|r|is_work_plan(&state,r,&owner)).collect::<Vec<_>>() }),
                );
            }
            if method == "standard.capture.preview" {
                let roots: Vec<Id> = serde_json::from_value(p["root_workflow_refs"].clone())
                    .map_err(|e| err("invalid_params", e.to_string()))?;
                return Ok(json!(resolve_standard_sources(&state, &roots, &scope)?));
            }
            if method == "query.standard_diff" {
                let mut left = standard_snapshot(&state, &text(p, "before_ref")?, &scope)?;
                let mut right = standard_snapshot(&state, &text(p, "after_ref")?, &scope)?;
                if let Some(entry) = p["before_workflow_ref"].as_str() {
                    left = standard_entry_snapshot(&left, entry)?;
                }
                if let Some(entry) = p["after_workflow_ref"].as_str() {
                    right = standard_entry_snapshot(&right, entry)?;
                }
                return Ok(diff_standard_snapshots(&left, &right));
            }
            if method == "query.work_standard_diff" {
                return compare_work_standards(&state, &state, p);
            }
            let id = text(p, "id")?;
            let snapshot = standard_snapshot(&state, &id, &scope)?;
            if method == "query.standard_usage" {
                let all: Vec<_> = state
                    .governance
                    .iter()
                    .filter(|r| {
                        [
                            "project_configuration",
                            "work_configuration",
                            "workflow_plan_revision",
                        ]
                        .contains(&r.r#type.as_str())
                            && r.data["standard_ref"] == id
                            && scope
                                .project_id()
                                .is_none_or(|project| r.scope.project_id() == Some(project))
                    })
                    .collect();
                return Ok(
                    json!({"records":all.iter().skip(offset).take(limit).collect::<Vec<_>>(),"total":all.len(),"offset":offset}),
                );
            }
            let mut result = json!(snapshot);
            result["total_nodes"] = json!(snapshot.nodes.len());
            result["total_edges"] = json!(snapshot.edges.len());
            result["nodes"] = json!(
                snapshot
                    .nodes
                    .iter()
                    .skip(offset)
                    .take(limit)
                    .collect::<Vec<_>>()
            );
            result["edges"] = json!(
                snapshot
                    .edges
                    .iter()
                    .skip(p["edge_offset"].as_u64().unwrap_or(0) as usize)
                    .take(limit)
                    .collect::<Vec<_>>()
            );
            result["record"] = json!(state.configuration(&id)?);
            result["archived"] = json!(standard_archived(&state, &id));
            Ok(result)
        })())
    }
}

/// A replacement may change internals while preserving the exact external material contract.
pub(crate) fn check_invocation_contract(
    expected: &WorkflowDefinition,
    actual: &WorkflowDefinition,
) -> Result<()> {
    let output = |doc: &WorkflowDefinition| -> Result<BTreeMap<String, PortDefinition>> {
        let keys = doc.steps.iter().map(|s| s.key.as_str()).collect();
        doc.outputs
            .iter()
            .map(|(name, source)| Ok((name.clone(), doc.source(source, &keys)?.clone())))
            .collect()
    };
    if expected.inputs != actual.inputs || output(expected)? != output(actual)? {
        return Err(err(
            "invocation_contract_mismatch",
            "所选工作流程的输入输出合同与父调用不兼容",
        ));
    }
    Ok(())
}

pub(crate) fn is_work_root(state: &EngineeringState, row: &ConfigurationRecord) -> bool {
    row.r#type == "workflow_instance"
        && (row.data["parent_execution_id"].is_null()
            || state.governance.iter().any(|invocation| {
                invocation.r#type == "workflow_invocation"
                    && invocation.data["child_instance_id"] == row.id
                    && invocation.data["member_identity"] == row.data["owner_id"]
            }))
}

pub(crate) fn current_work_instance<'a>(
    state: &'a EngineeringState,
    owner: &str,
) -> Option<&'a ConfigurationRecord> {
    state
        .governance
        .iter()
        .rev()
        .find(|row| row.data["owner_id"] == owner && is_work_root(state, row))
}

fn is_work_plan(state: &EngineeringState, row: &ConfigurationRecord, owner: &str) -> bool {
    row.r#type == "workflow_plan_revision"
        && row.data["owner_id"] == owner
        && row.data["instance_id"]
            .as_str()
            .and_then(|id| state.configuration(id).ok())
            .is_some_and(|instance| is_work_root(state, instance))
}

fn selected_work_record<'a>(
    state: &'a EngineeringState,
    owner: &str,
) -> Option<&'a ConfigurationRecord> {
    current_work_instance(state, owner)
        .and_then(|r| {
            state
                .configuration(r.data["active_plan_revision_id"].as_str()?)
                .ok()
        })
        .or_else(|| work_configuration(state, owner))
        .or_else(|| {
            state
                .governance
                .iter()
                .filter(|r| r.r#type == "project_configuration" && r.definition_id == owner)
                .max_by_key(|r| r.version)
        })
}

/// A read-only view of an actual consumer, independently of its project's current defaults.
pub fn work_standard_selection(
    state: &EngineeringState,
    owner: &str,
    historical: Option<&str>,
) -> Result<Value> {
    let work = work_reference(state, owner)?;
    let scope = work.scope(state)?;
    let source = if let Some(reference) = historical {
        let record = state.configuration(reference)?;
        require(
            record.r#type == "workflow_plan_revision" && record.data["owner_id"] == owner,
            "历史计划不属于该工作",
        )?;
        record
    } else {
        selected_work_record(state, owner)
            .ok_or_else(|| err("work_configuration_required", "工作尚未采用规范"))?
    };
    let standard = text(&source.data, "standard_ref")?;
    let workflow = text(&source.data, "workflow_ref")?;
    standard_entry(state, &standard, &workflow, &scope)?;
    let is_plan = source.r#type == "workflow_plan_revision";
    let children = if historical.is_some() {
        state
            .governance
            .iter()
            .filter(|r| {
                r.r#type == "workflow_invocation" && r.data["parent_plan_revision_id"] == source.id
            })
            .map(|r| r.data.clone())
            .collect::<Vec<_>>()
    } else if is_plan {
        let parent = source.data["instance_id"]
            .as_str()
            .and_then(|id| state.configuration(id).ok());
        let executions = parent
            .and_then(|r| r.data["steps"].as_array())
            .into_iter()
            .flatten()
            .filter_map(|e| e["id"].as_str())
            .collect::<std::collections::BTreeSet<_>>();
        state.governance.iter().filter(|r|r.r#type=="workflow_instance" && r.data["parent_execution_id"].as_str().is_some_and(|id|executions.contains(id)))
            .map(|r|json!({"instance_id":r.id,"owner_id":r.data["owner_id"],"standard_ref":r.data["plan"]["standard_ref"],"workflow_ref":r.data["workflow_ref"],"plan_revision_id":r.data["active_plan_revision_id"]})).collect::<Vec<_>>()
    } else {
        vec![]
    };
    let history=state.governance.iter().filter(|r|is_work_plan(&state,r,&owner))
        .map(|r|json!({"plan_revision_id":r.id,"standard_ref":r.data["standard_ref"],"workflow_ref":r.data["workflow_ref"],"reason":r.data["reason"]})).collect::<Vec<_>>();
    let exceptions = state
        .governance
        .iter()
        .filter(|r| r.r#type == "workflow_waiver" && r.data["plan_revision_id"] == source.id)
        .map(|r| json!({"id":r.id,"data":r.data}))
        .collect::<Vec<_>>();
    Ok(
        json!({"work":work,"project_id":scope.project_id(),"selection_ref":source.id,"basis":if is_plan{"execution"}else{"configured"},"plan_revision_id":if is_plan{Some(&source.id)}else{None},"standard_ref":standard,"workflow_ref":workflow,"children":children,"children_basis":if historical.is_some(){"invocation_initial"}else{"current"},"history":history,"exceptions":exceptions}),
    )
}

pub fn compare_work_standards(
    before: &EngineeringState,
    after: &EngineeringState,
    params: &Value,
) -> Result<Value> {
    let left = work_standard_selection(
        before,
        &text(params, "before_owner_id")?,
        params["before_plan_revision_id"].as_str(),
    )?;
    let right = work_standard_selection(
        after,
        &text(params, "after_owner_id")?,
        params["after_plan_revision_id"].as_str(),
    )?;
    let resolve = |state: &EngineeringState, selection: &Value| -> Result<StandardSnapshot> {
        let scope = Scope::project(text(selection, "project_id")?);
        standard_snapshot(state, &text(selection, "standard_ref")?, &scope)
    };
    let a = resolve(before, &left)?;
    let b = resolve(after, &right)?;
    let entry_diff = diff_standard_snapshots(
        &standard_entry_snapshot(&a, &text(&left, "workflow_ref")?)?,
        &standard_entry_snapshot(&b, &text(&right, "workflow_ref")?)?,
    );
    Ok(
        json!({"before":left,"after":right,"diff":diff_standard_snapshots(&a,&b),"entry_diff":entry_diff,"same_entry":left["workflow_ref"]==right["workflow_ref"]}),
    )
}

pub fn work_standard_choices(state: &EngineeringState) -> Vec<Value> {
    state.projects.iter().map(|p|(p.id.as_str(),p.title.as_str()))
        .chain(state.features.iter().map(|f|(f.id.as_str(),f.title.as_str())))
        .chain(state.tasks.iter().map(|t|(t.id.as_str(),t.title.as_str())))
        .filter_map(|(owner,title)|{
            let record=selected_work_record(state,owner)?;
            let work=work_reference(state,owner).ok()?;
            let scope=work.scope(state).ok()?;
            Some(json!({"id":owner,"title":title,"work":work,"project_id":scope.project_id(),"standard_ref":record.data["standard_ref"],"workflow_ref":record.data["workflow_ref"]}))
        }).collect()
}

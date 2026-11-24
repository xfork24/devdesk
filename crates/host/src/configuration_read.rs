//! Capture committed configuration views once; never synchronize or restore engineering history on a directory read.
use super::*;

pub(super) fn directory_query(method: &str) -> bool {
    matches!(
        method,
        "query.configurations"
            | "query.configuration_index"
            | "query.configuration_records"
            | "query.configuration_usage"
            | "query.configuration_references"
            | "query.configuration_graph"
            | "query.standard"
            | "query.project_standard_preview"
            | "query.roles"
            | "query.capabilities"
            | "query.workflows"
            | "query.workflow"
            | "query.workflow_draft"
            | "query.workflow_requirements"
            | "query.agent_revision"
            | "query.agent_instances"
            | "query.standard_catalog"
            | "query.configuration_defaults"
            | "query.project_configuration"
            | "query.integrations"
            | "query.integration_bindings"
            | "query.permission_resources"
            | "query.template_catalog"
    )
}

impl Workspace {
    pub(super) fn visit_project_catalogs(
        &self,
        mut visit: impl FnMut(&Project, &ConfigurationCatalogSnapshot) -> Result<()>,
    ) -> Result<()> {
        let root = PathBuf::from(self.global.root());
        let mut offset = 0;
        loop {
            crate::check_read_cancelled()?;
            let (projects, total) = self.global.projects(offset, 100)?;
            for project in &projects {
                crate::check_read_cancelled()?;
                if self.global.removed_project_missing(&project.id)? {
                    continue;
                }
                let catalog = SqliteStore::read_project_catalog(&root, &project.id)?;
                visit(project, &catalog)?;
            }
            offset += projects.len();
            if offset >= total || projects.is_empty() {
                return Ok(());
            }
        }
    }

    pub(super) fn configuration_bundle(
        &self,
        domain_epoch: Option<u64>,
    ) -> Result<(Arc<ConfigurationCatalogSnapshot>, String, Value)> {
        let global = self.global.configuration_snapshot()?;
        let (visibility, domain_sources) = if let Some(packages) = self.domains.get() {
            let (visibility, sources) = packages
                .as_ref()
                .map_err(Clone::clone)?
                .configuration_scope(domain_epoch)?;
            (Some(visibility), sources)
        } else {
            (None, None)
        };
        let engines: Vec<_> = self
            .opened
            .lock()
            .unwrap()
            .iter()
            .filter(|(id, _)| id.as_str() != GLOBAL_DISCUSSION_NAMESPACE)
            .filter(|(id, _)| visibility.as_ref().is_none_or(|scope| scope.includes(id)))
            .map(|(id, e)| (id.clone(), e.clone()))
            .collect();
        let mut projects = vec![];
        let mut clocks = vec![];
        for (id, engine) in engines {
            let catalog = engine.store.configuration_snapshot()?;
            clocks.push((id, catalog.revision));
            projects.push(catalog);
        }
        let active_digest = visibility
            .as_ref()
            .and_then(|scope| scope.active_digest.as_deref());
        let clock = configuration_hash(&(active_digest, &clocks));
        let source_epochs = json!(
            clocks
                .iter()
                .cloned()
                .collect::<std::collections::BTreeMap<_, _>>()
        );
        let key = format!("directory:{}:{clock}", global.revision);
        if let Some((_, snapshot)) = self
            .configuration_views
            .lock()
            .unwrap()
            .iter()
            .find(|(k, _)| k == &key)
        {
            return Ok((snapshot.clone(), clock, source_epochs));
        }
        let snapshot = if projects.is_empty()
            && visibility.is_none()
            && domain_sources
                .as_ref()
                .is_none_or(|catalog| catalog.records.is_empty())
        {
            global
        } else {
            let mut view = compose_configuration_directory(
                &global,
                domain_sources
                    .as_ref()
                    .map_or(&[], |catalog| catalog.records.as_ref()),
                &projects,
                |project| {
                    visibility
                        .as_ref()
                        .is_none_or(|scope| scope.includes(project))
                },
            )?;
            if let Some(sources) = domain_sources {
                view = view.with_automation_sources(sources.automation);
            }
            Arc::new(view)
        };
        self.retain_configuration_view(key, snapshot.clone())?;
        Ok((snapshot, clock, source_epochs))
    }
    fn retain_configuration_view(
        &self,
        key: String,
        view: Arc<ConfigurationCatalogSnapshot>,
    ) -> Result<()> {
        if serde_json::to_vec(view.state.as_ref())
            .map_err(|e| err("storage", e.to_string()))?
            .len()
            <= 4 * 1024 * 1024
        {
            let mut views = self.configuration_views.lock().unwrap();
            if !views.iter().any(|(k, _)| k == &key) {
                if views.len() >= 8 {
                    views.remove(0);
                }
                views.push((key, view));
            }
        }
        Ok(())
    }
    pub(crate) fn configuration_version(
        &self,
        engine: &Engine,
        domain_epoch: Option<u64>,
    ) -> Result<Value> {
        let global = self.global.catalog_revision()?;
        let visibility = self.project_visibility(domain_epoch)?;
        let engines: Vec<_> = self
            .opened
            .lock()
            .map_err(|e| err("storage", e.to_string()))?
            .iter()
            .filter(|(id, _)| id.as_str() != GLOBAL_DISCUSSION_NAMESPACE)
            .filter(|(id, _)| visibility.as_ref().is_none_or(|scope| scope.includes(id)))
            .map(|(id, engine)| (id.clone(), engine.clone()))
            .collect();
        let mut clocks = Vec::with_capacity(engines.len());
        for (id, engine) in engines {
            clocks.push((id, engine.store.catalog_revision()?));
        }
        let active_digest = visibility
            .as_ref()
            .and_then(|scope| scope.active_digest.as_deref());
        let clock = configuration_hash(&(active_digest, &clocks));
        let source_epochs = json!(
            clocks
                .into_iter()
                .collect::<std::collections::BTreeMap<_, _>>()
        );
        let project = if engine.store.project_id().is_some() {
            engine.store.catalog_revision()?
        } else {
            0
        };
        Ok(
            json!({"host":self.catalog_instance,"global":global,"project":project,"project_sources":clock,"source_epochs":source_epochs}),
        )
    }
    pub(crate) fn event_configuration_version(&self, domain_epoch: Option<u64>) -> Result<Value> {
        self.configuration_version(&self.configuration, domain_epoch)
    }
    pub(super) fn project_directory(
        &self,
        params: &Value,
        domain_epoch: Option<u64>,
    ) -> Result<Value> {
        let offset = params["project_offset"].as_u64().unwrap_or(0) as usize;
        let limit = params["limit"].as_u64().unwrap_or(50).clamp(1, 100) as usize;
        let visibility = self.project_visibility(domain_epoch)?;
        let (projects, total) = self.global.projects_visible(
            offset,
            limit,
            params["github_only"] == true,
            visibility.as_ref(),
        )?;
        let mut projects = projects;
        for summary in &mut projects {
            let engine = self
                .opened
                .lock()
                .map_err(|e| err("storage", e.to_string()))?
                .get(&summary.id)
                .cloned();
            if let Some(engine) = engine {
                *summary = engine.store.project_metadata()?;
            }
        }
        Ok(json!({"projects":projects,"total_projects":total}))
    }
    pub(super) fn execution_workspace(
        &self,
        params: &Value,
        domain_epoch: Option<u64>,
    ) -> Result<Value> {
        let project = devdesk_core::read_models::identifier(params, "project_id")?;
        let query = devdesk_core::read_models::ExecutionWorkspaceQuery::parse(params)?;
        let engine = self.project_engine(Some(&project))?;
        let mut result = json!(engine.store.execution_workspace(&query)?);
        let directory = self.project_directory(params, domain_epoch)?;
        result["projects"] = directory["projects"].clone();
        result["total_projects"] = directory["total_projects"].clone();
        result["active_project_id"] = json!(project);
        result["catalog_version"] = self.configuration_version(&engine, domain_epoch)?;
        let settings = devdesk_core::settings::read(self.global.as_ref())?;
        result["global_concurrency"] = json!(settings.global_concurrency);
        result["default_project_concurrency"] = json!(settings.default_project_concurrency);
        result["default_harness"] = json!(settings.default_harness);
        result["settings_versions"] = json!(settings.versions);
        result["data_root"] = json!(self.global.root());
        result["execution_mode"] = result["active_project"]["execution_mode"].clone();
        if serde_json::to_vec(&result)
            .map_err(|e| err("storage", e.to_string()))?
            .len()
            > 128 * 1024
        {
            return Err(err("read_model_limit", "执行摘要超过预算，请缩小分页"));
        }
        Ok(result)
    }
    pub(super) fn ui_bootstrap(&self, params: &Value, domain_epoch: Option<u64>) -> Result<Value> {
        let _span = performance::Span::new("ui.bootstrap");
        let (global, clock, source_epochs) = self.configuration_bundle(domain_epoch)?;
        let engine = match self.project_engine(params["project_id"].as_str()) {
            Ok(engine) => engine,
            Err(e) if e.code == "project_required" => self.configuration.clone(),
            Err(e) => return Err(e),
        };
        let project_id = engine.store.project_id();
        let view = if let Some(project) = project_id.as_deref() {
            let local = engine.store.configuration_snapshot()?;
            self.configuration_view(&global, &local, project, &clock)?
        } else {
            global.clone()
        };
        let current = view.state.projects.first();
        let health = if current.is_some() {
            engine.configuration_status_at(view.clone())?
        } else {
            json!({"ready":false,"issues":[]})
        };
        let mut result = self.project_directory(params, domain_epoch)?;
        if let Some(current) = current
            && let Some(rows) = result["projects"].as_array_mut()
            && let Some(summary) = rows.iter_mut().find(|row| row["id"] == current.id)
        {
            *summary = json!(current);
        }
        result["kind"] = json!("ui_bootstrap");
        result["active_project"] = json!(current);
        result["active_project_id"] = json!(project_id);
        result["repository_ready"] = json!(view.repository_ready);
        result["configuration_ready"] = health["ready"].clone();
        result["configuration_status"] = health;
        result["catalog_generation"] = json!(template_catalog_generation(&global.state));
        result["catalog_version"] = json!({"host":self.catalog_instance,"global":global.revision,"project":if project_id.is_some(){view.revision}else{0},"project_sources":clock,"source_epochs":source_epochs});
        result["definitions"]=json!(view.state.definitions.iter().filter(|d|template_projection_current(&view.state,d)).map(|d|json!({"id":d.id,"name":d.name,"family":d.family,"version":d.version,"supersedes":d.supersedes,"agent_profile":d.agent_profile})).collect::<Vec<_>>());
        result["configurations"] = json!(
            view.state
                .governance
                .iter()
                .filter(|r| matches!(
                    r.r#type.as_str(),
                    "project_configuration" | "work_configuration"
                ))
                .collect::<Vec<_>>()
        );
        result["features"] = json!([]);
        result["total_features"] = json!(0);
        result["runtime_installations"] = json!(self.global.runtime_installations()?);
        let settings = devdesk_core::settings::read(self.global.as_ref())?;
        result["global_concurrency"] = json!(settings.global_concurrency);
        result["default_project_concurrency"] = json!(settings.default_project_concurrency);
        result["default_harness"] = json!(settings.default_harness);
        result["settings_versions"] = json!(settings.versions);
        result["data_root"] = json!(self.global.root());
        result["execution_mode"] = json!(current.map_or("live", |p| p.execution_mode.as_str()));
        Ok(result)
    }
    fn configuration_view(
        &self,
        global: &Arc<ConfigurationCatalogSnapshot>,
        local: &Arc<ConfigurationCatalogSnapshot>,
        project: &str,
        clock: &str,
    ) -> Result<Arc<ConfigurationCatalogSnapshot>> {
        let key = format!("{project}:{}:{}:{clock}", global.revision, local.revision);
        if let Some((_, view)) = self
            .configuration_views
            .lock()
            .unwrap()
            .iter()
            .find(|(k, _)| k == &key)
        {
            return Ok(view.clone());
        }
        let _span = performance::Span::new("catalog.compose");
        let view = Arc::new(compose_configuration_catalog(global, local, project)?);
        self.retain_configuration_view(key, view.clone())?;
        Ok(view)
    }

    pub(super) fn configuration_read(
        &self,
        method: &str,
        params: &Value,
        domain_epoch: Option<u64>,
    ) -> Option<Result<Value>> {
        if !directory_query(method)
            || (method == "query.configurations"
                && params["kind"]
                    .as_str()
                    .is_some_and(|kind| !catalog_kind(kind)))
        {
            return None;
        }
        Some((|| {
            let _span = performance::Span::new("catalog.query");
            let (global, clock, source_epochs) = self.configuration_bundle(domain_epoch)?;
            let target = params["id"].as_str().or_else(|| params["target"].as_str());
            let project = params["project_id"].as_str();
            let local = match method {
                "query.configuration_defaults" | "query.template_catalog" => false,
                "query.project_configuration" => true,
                "query.configuration_index"
                | "query.configuration_records"
                | "query.configuration_usage"
                | "query.configuration_references"
                | "query.configuration_graph"
                | "query.project_standard_preview" => project.is_some() || params["local"] == true,
                "query.integrations"
                | "query.integration_bindings"
                | "query.permission_resources" => project.is_some() || params["local"] == true,
                _ => {
                    params["local"] == true
                        || (project.is_some()
                            && (method.starts_with("query.workflow")
                                || method.starts_with("query.standard")))
                        || target.is_some_and(|id| global.record(id).is_none())
                }
            };
            let engine = if local {
                self.project_engine(project)?
            } else {
                self.configuration.clone()
            };
            let view = if local {
                let local = engine.store.configuration_snapshot()?;
                self.configuration_view(
                    &global,
                    &local,
                    engine.store.project_id().as_deref().unwrap_or("global"),
                    &clock,
                )?
            } else {
                global.clone()
            };
            let pinned = engine.at_configuration(view.clone());
            let mut compute_params = params.clone();
            if matches!(method, "query.standard" | "query.project_standard_preview") {
                compute_params.as_object_mut().map(|p| p.remove("view"));
            }
            let runtime_key = if method == "query.permission_resources" {
                json!({"installations":self.global.runtime_installations()?,"assets":devdesk_adapters::permissions::asset_digests()})
            } else {
                Value::Null
            };
            let key = configuration_hash(&(
                "configuration-query/v1",
                method,
                &compute_params,
                runtime_key,
            ));
            crate::check_read_cancelled()?;
            let mut computed = false;
            let mut result = view.cached_query(key, || {
                crate::check_read_cancelled()?;
                computed = true;
                let _compile = performance::Span::new("catalog.query_build");
                match method {
                    "query.configuration_index" => configuration_index(&view, params, project),
                    "query.configuration_records" => configuration_records(&view, params, project),
                    "query.configuration_usage" => configuration_reference_usages(&view, params, project),
                    "query.configuration_defaults" if params["view"]=="selection" => pinned.configuration_default_selection(project),
                    "query.configuration_defaults" => pinned.configuration_defaults(project),
                    "query.project_configuration" if params["view"]=="selection" => project_configuration_context(&view,project),
                    "query.template_catalog" => {
                        let info=template_bundle_info()?;
                        Ok(json!({"generation":template_catalog_generation(&view.state),"bundle_version":info["bundle_version"],"bundle_hash":info["bundle_hash"],"current_bundle_version":template_catalog_state(&view.state).map(|r|&r.data["bundle_version"]),"counts":template_catalog_counts(&view.state)}))
                    }
                    "query.permission_resources" => self.permission_resource_api_at(method,params,Some(view.clone()),Some(global.state.as_ref())).expect("permission read"),
                    "query.configurations" | "query.workflows" if local => local_directory(&view,&global,method,params,project),
                    _ => pinned.query(method,&compute_params,self.global.concurrency()?),
                }
            })?;
            performance::record(
                if computed {
                    "catalog.memo_miss"
                } else {
                    "catalog.memo_hit"
                },
                0.0,
            );
            if params["view"] == "summary" {
                summarize_configuration_result(method, &mut result);
            }
            // Sources and rows come from the same pinned view, including project-only and historical revisions.
            let sources = result["records"]
                .as_array()
                .map(|rows| {
                    rows.iter()
                        .filter_map(|row| {
                            let id = row["id"].as_str()?;
                            view.origin(id)
                                .or_else(|| global.origin(id))
                                .map(|origin| (id.to_owned(), origin.clone()))
                        })
                        .collect::<serde_json::Map<String, Value>>()
                })
                .unwrap_or_default();
            if let Some(object) = result.as_object_mut() {
                object.insert(
                    "catalog_generation".into(),
                    json!(template_catalog_generation(&global.state)),
                );
                if !matches!(
                    method,
                    "query.configuration_index" | "query.configuration_records"
                ) {
                    object.insert("template_sources".into(), json!(sources));
                }
                object.insert("catalog_version".into(),json!({"host":self.catalog_instance,"global":global.revision,"project":if local {view.revision}else{0},"project_sources":clock,"source_epochs":source_epochs}));
            }
            Ok(result)
        })())
    }
}

pub(super) fn local_directory(
    view: &ConfigurationCatalogSnapshot,
    global: &ConfigurationCatalogSnapshot,
    method: &str,
    params: &Value,
    project: Option<&str>,
) -> Result<Value> {
    let state = &view.state;
    let mut rows: Vec<_> = state
        .governance
        .iter()
        .filter(|r| {
            if method == "query.workflows" {
                matches!(r.r#type.as_str(), "workflow" | "workflow_draft")
            } else {
                params["kind"]
                    .as_str()
                    .map_or(r.is_definition() || r.r#type == "agent_instance", |kind| {
                        r.r#type == kind
                    })
            }
        })
        .collect();
    if method == "query.workflows" {
        for record in global
            .records("workflow_draft")
            .filter(|r| r.scope == Scope::global())
        {
            if !rows.iter().any(|old| old.id == record.id) {
                rows.push(record);
            }
        }
    }
    rows.retain(|r| {
        view.is_listed(r)
            && (params["include_archived"] == true || template_record_current(state, r))
            && configuration_visible(state, r, project)
            && (params["include_deleted"] == true || !configuration_retired(state, r))
            && (params["include_history"] == true
                || if params["include_archived"] == true {
                    !state.governance.iter().any(|n| {
                        n.r#type == r.r#type
                            && n.definition_id == r.definition_id
                            && n.version > r.version
                    })
                } else {
                    view.is_current_head(r)
                })
            && params
                .get("scope")
                .is_none_or(|scope| json!(r.scope) == *scope)
    });
    rows.sort_by_key(|r| std::cmp::Reverse(r.created_at));
    let offset = params["offset"].as_u64().unwrap_or(0) as usize;
    let limit = params["limit"].as_u64().unwrap_or(50).clamp(1, 100) as usize;
    Ok(
        json!({"records":rows.iter().skip(offset).take(limit).collect::<Vec<_>>(),"total":rows.len()}),
    )
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::workspace::tests::create;

    #[test]
    fn thin_catalog_exact_records_and_memo_respect_committed_versions() {
        let dir = tempfile::tempdir().unwrap();
        let w = Workspace::open_with_adapters(dir.path(), crate::test_runtime::adapters()).unwrap();
        let project = create(&w, "thin configuration");
        let index = w.dispatch("query.configuration_index", json!({})).unwrap();
        assert!(
            serde_json::to_vec(&index).unwrap().len() < 80 * 1024,
            "index bytes {}",
            index.to_string().len()
        );
        for row in index["records"].as_array().unwrap() {
            assert!(row["data"]["instructions"].is_null());
            assert!(row["data"]["steps"].is_null());
            assert!(row["data"]["installations"].is_null());
        }
        let role = index["records"]
            .as_array()
            .unwrap()
            .iter()
            .find(|r| r["type"] == "role")
            .unwrap();
        let detail = w
            .dispatch("query.configuration_records", json!({"refs":[role["id"]]}))
            .unwrap();
        assert!(detail["records"][0]["data"]["instructions"].is_string());
        let params = json!({"project_id":project,"view":"selection"});
        let first = w
            .dispatch("query.configuration_defaults", params.clone())
            .unwrap();
        let trace = performance::Trace::start();
        let second = w.dispatch("query.configuration_defaults", params).unwrap();
        let measured = trace.finish();
        assert_eq!(first, second);
        assert!(first["records"].is_null());
        assert_eq!(measured["spans"]["catalog.memo_hit"]["calls"], 1);
        assert!(measured["spans"]["project.full_snapshot"].is_null());
        let published=w.dispatch("role.create",json!({"command_id":id(),"payload":{"name":"another role","scope":"global","data":{"instructions":"body only on selection"}}})).unwrap();
        let after = w.dispatch("query.configuration_index", json!({})).unwrap();
        assert_ne!(index["index_version"], after["index_version"]);
        let stale = w
            .dispatch(
                "query.configuration_index",
                json!({"offset":1,"index_version":index["index_version"]}),
            )
            .unwrap_err();
        assert_eq!(stale.code, "catalog_changed");
        assert!(
            after["records"]
                .as_array()
                .unwrap()
                .iter()
                .any(|r| r["id"] == published["id"])
        );
        let identity = w
            .dispatch(
                "query.model_catalog_identity",
                json!({"model_provider":"openai"}),
            )
            .unwrap();
        assert_eq!(identity["available"], false);
        assert!(identity["identity"].is_string());
    }

    #[test]
    fn reference_usage_reads_packaged_positions_and_tracks_scoped_committed_heads() {
        let dir = tempfile::tempdir().unwrap();
        let w = Workspace::open_with_adapters(dir.path(), crate::test_runtime::adapters()).unwrap();
        let project = create(&w, "usage owner");
        let other = create(&w, "usage other");
        let skill = "builtin-skill-engineering-context:4";
        let params = json!({"id":skill,"project_id":project});
        let before = w
            .dispatch("query.configuration_usage", params.clone())
            .unwrap();
        let factory = w
            .dispatch(
                "query.configuration_usage",
                json!({"id":skill,"view":"builtin"}),
            )
            .unwrap();
        assert_eq!(before["users"], factory["users"]);
        let payload = json!({"name":"Scoped usage","scope":format!("project:{project}"),
            "model_ref":"builtin-model-gpt-6-2-sol-high:1","harness_ref":"builtin-harness-codex-managed:2",
            "skills":[skill],"tools":["builtin-tool-codex-native:2"],"data":{}});
        let capability = w
            .dispatch(
                "capability.create",
                json!({"command_id":id(),"project_id":project,"payload":payload}),
            )
            .unwrap();
        let after = w
            .dispatch("query.configuration_usage", params.clone())
            .unwrap();
        assert!(
            after["users"]
                .as_array()
                .unwrap()
                .iter()
                .any(|u| u["reference"] == capability["id"])
        );
        let isolated = w
            .dispatch(
                "query.configuration_usage",
                json!({"id":skill,"project_id":other}),
            )
            .unwrap();
        assert!(
            !isolated["users"]
                .as_array()
                .unwrap()
                .iter()
                .any(|u| u["reference"] == capability["id"])
        );
        assert!(w.dispatch("query.configuration_usage",json!({"id":skill,"project_id":project,"offset":1,"index_version":before["index_version"]})).is_err());
        let mut revised = payload;
        revised["skills"] = json!([]);
        w.dispatch("capability.revise",json!({"command_id":id(),"project_id":project,"target":capability["id"],"expected_revision":capability["revision"],"payload":revised})).unwrap();
        let changed = w.dispatch("query.configuration_usage", params).unwrap();
        assert!(
            !changed["users"]
                .as_array()
                .unwrap()
                .iter()
                .any(|u| u["name"] == "Scoped usage")
        );
        assert_eq!(
            w.dispatch(
                "query.configuration_usage",
                json!({"id":skill,"view":"builtin"})
            )
            .unwrap()["users"],
            factory["users"]
        );
        let delivery = w
            .dispatch(
                "query.configuration_usage",
                json!({"id":"builtin-delivery-prd:5"}),
            )
            .unwrap();
        let locations = delivery["users"].as_array().unwrap();
        assert!(locations.iter().any(|u| u["step"]["key"] == "requirements"
            && u["kind"] == "step_output"
            && u["stage"]["name"] == "需求"));
        assert!(
            locations
                .iter()
                .any(|u| u["step"]["key"] == "requirements_review" && u["kind"] == "step_input")
        );
        let recipe = w
            .dispatch(
                "query.configuration_usage",
                json!({"id":"builtin-delivery-test-design:2"}),
            )
            .unwrap();
        assert!(
            recipe["users"]
                .as_array()
                .unwrap()
                .iter()
                .any(|u| u["kind"] == "step_argument"
                    && u["step"]["key"] == "red"
                    && u["port"] == "recipe")
        );
        let trace = performance::Trace::start();
        w.dispatch(
            "query.configuration_usage",
            json!({"id":"builtin-delivery-prd:5"}),
        )
        .unwrap();
        let measured = trace.finish();
        assert!(measured["spans"]["project.full_snapshot"].is_null());
    }

    #[test]
    fn index_scope_history_and_record_visibility_are_exact() {
        let dir = tempfile::tempdir().unwrap();
        let w = Workspace::open_with_adapters(dir.path(), crate::test_runtime::adapters()).unwrap();
        let project = create(&w, "scope");
        let owned=w.dispatch("role.create",json!({"command_id":id(),"project_id":project,"payload":{"name":"private","scope":format!("project:{project}"),"data":{"instructions":"private body"}}})).unwrap();
        let global = w
            .dispatch("query.configuration_index", json!({"kind":"role"}))
            .unwrap();
        // The global administration catalog intentionally sees all sources. A project must not.
        assert!(
            global["records"]
                .as_array()
                .unwrap()
                .iter()
                .any(|r| r["id"] == owned["id"])
        );
        let snapshot = w.configuration.store.configuration_snapshot().unwrap();
        let other = id();
        let isolated =
            configuration_index(&snapshot, &json!({"kind":"role"}), Some(&other)).unwrap();
        assert!(
            !isolated["records"]
                .as_array()
                .unwrap()
                .iter()
                .any(|r| r["id"] == owned["id"])
        );
        assert!(
            configuration_records(&snapshot, &json!({"refs":[owned["id"]]}), Some(&other)).is_err()
        );
        let local = w
            .dispatch(
                "query.configuration_index",
                json!({"kind":"role","project_id":project,"scope":format!("project:{project}")}),
            )
            .unwrap();
        assert!(
            local["records"]
                .as_array()
                .unwrap()
                .iter()
                .any(|r| r["id"] == owned["id"])
        );
        let detail = w
            .dispatch(
                "query.configuration_records",
                json!({"refs":[owned["id"]],"project_id":project}),
            )
            .unwrap();
        assert_eq!(detail["records"][0]["data"]["instructions"], "private body");
    }

    #[test]
    fn directory_read_does_not_wait_for_definition_writer_or_restore_history() {
        let dir = tempfile::tempdir().unwrap();
        let w = Workspace::open_with_adapters(dir.path(), crate::test_runtime::adapters()).unwrap();
        let project = create(&w, "configuration read isolation");
        let params = json!({"local":true,"project_id":project,"include_history":true,"limit":100});
        w.dispatch("query.configurations", params.clone()).unwrap();
        let (send, receive) = std::sync::mpsc::channel();
        let reader = w.clone();
        let thread = std::thread::spawn(move || {
            let trace = performance::Trace::start();
            let result = reader.dispatch("query.configurations", params);
            send.send((result, trace.finish())).unwrap();
        });
        let received = receive.recv_timeout(std::time::Duration::from_secs(3));
        thread.join().unwrap();
        let (result, measurement) =
            received.expect("a directory reader must not hold the writer gate");
        assert!(!result.unwrap()["records"].as_array().unwrap().is_empty());
        for name in [
            "project.full_snapshot",
            "global.full_snapshot",
            "global.transaction",
            "project.transaction",
        ] {
            assert!(measurement["spans"][name].is_null(), "{measurement}");
        }
    }

    #[test]
    fn newly_published_sources_are_visible_without_read_side_synchronization() {
        let dir = tempfile::tempdir().unwrap();
        let w = Workspace::open_with_adapters(dir.path(), crate::test_runtime::adapters()).unwrap();
        let project = create(&w, "new committed source");
        let params = json!({"project_id":project,"local":true,"kind":"role","include_history":true,"limit":100});
        let before = w.dispatch("query.configurations", params.clone()).unwrap();
        let published=w.dispatch("role.create",json!({"command_id":id(),"payload":{"name":"new role","data":{"instructions":"new role instructions"},"scope":"global"}})).unwrap();
        let engine = w.project_engine(Some(&project)).unwrap();
        assert!(
            engine
                .store
                .configuration_snapshot()
                .unwrap()
                .record(published["id"].as_str().unwrap())
                .is_none()
        );
        let trace = performance::Trace::start();
        let after = w.dispatch("query.configurations", params).unwrap();
        let measurement = trace.finish();
        assert!(
            after["records"]
                .as_array()
                .unwrap()
                .iter()
                .any(|r| r["id"] == published["id"])
        );
        assert_ne!(before["catalog_version"], after["catalog_version"]);
        assert!(measurement["spans"]["project.transaction"].is_null());
        assert!(measurement["spans"]["global.transaction"].is_null());
        assert!(
            engine
                .store
                .configuration_snapshot()
                .unwrap()
                .record(published["id"].as_str().unwrap())
                .is_none()
        );
        assert_eq!(
            w.dispatch(
                "query.configurations",
                json!({"local":true,"project_id":id()})
            )
            .unwrap_err()
            .code,
            "not_found"
        );
    }

    #[test]
    fn bootstrap_and_project_directory_do_not_read_engineering_tables() {
        let dir = tempfile::tempdir().unwrap();
        let w = Workspace::open_with_adapters(dir.path(), crate::test_runtime::adapters()).unwrap();
        let project = create(&w, "light bootstrap");
        let trace = performance::Trace::start();
        let bootstrap = w
            .dispatch(
                "query.ui_bootstrap",
                json!({"project_id":project,"limit":100}),
            )
            .unwrap();
        let directory = w
            .dispatch("query.project_directory", json!({"limit":100}))
            .unwrap();
        let measurement = trace.finish();
        assert_eq!(bootstrap["active_project"]["id"], project);
        assert_eq!(directory["projects"][0]["id"], project);
        assert_eq!(bootstrap["catalog_version"]["host"], w.catalog_instance);
        assert!(measurement["spans"]["project.full_snapshot"].is_null());
        assert!(measurement["spans"]["global.full_snapshot"].is_null());
        assert!(bootstrap.get("project_snapshots").is_none());
    }

    #[test]
    fn project_owned_role_is_discoverable_without_query_publishing_it() {
        let dir = tempfile::tempdir().unwrap();
        let w = Workspace::open_with_adapters(dir.path(), crate::test_runtime::adapters()).unwrap();
        let project = create(&w, "project role source");
        let engine = w.project_engine(Some(&project)).unwrap();
        let before = w.configuration_version(&engine, None).unwrap();
        let role = engine.execute("role.create", Command {
            command_id: id(), target: None, expected_revision: None,
            payload: json!({"name":"Project role","scope":format!("project:{project}"),"data":{"instructions":"Project owned instructions"}}),
        }).unwrap();
        let trace = performance::Trace::start();
        let directory = w
            .dispatch("query.roles", json!({"include_history":true,"limit":100}))
            .unwrap();
        let after = w.configuration_version(&engine, None).unwrap();
        let measurement = trace.finish();
        assert_ne!(before, after);
        assert!(
            directory["records"]
                .as_array()
                .unwrap()
                .iter()
                .any(|r| r["id"] == role["id"])
        );
        assert!(
            w.global
                .configuration_snapshot()
                .unwrap()
                .record(role["id"].as_str().unwrap())
                .is_none()
        );
        for name in [
            "project.full_snapshot",
            "global.full_snapshot",
            "project.transaction",
            "global.transaction",
        ] {
            assert!(measurement["spans"][name].is_null(), "{measurement}");
        }
    }

    #[test]
    fn pinned_directory_queries_preserve_existing_query_results() {
        let dir = tempfile::tempdir().unwrap();
        let w = Workspace::open_with_adapters(dir.path(), crate::test_runtime::adapters()).unwrap();
        let project = create(&w, "read contract parity");
        let cases = [
            (
                "query.configurations",
                json!({"local":true,"project_id":project,"limit":100,"include_history":true}),
            ),
            (
                "query.configurations",
                json!({"local":true,"project_id":project,"limit":100,"offset":100,"include_history":true}),
            ),
            ("query.configurations", json!({"limit":100})),
            (
                "query.configurations",
                json!({"local":true,"project_id":project,"kind":"role","include_history":false}),
            ),
            (
                "query.roles",
                json!({"local":true,"project_id":project,"include_history":true}),
            ),
            ("query.capabilities", json!({"project_id":project})),
            (
                "query.workflows",
                json!({"project_id":project,"local":true,"include_history":true}),
            ),
            ("query.standard_catalog", json!({"project_id":project})),
            ("query.project_configuration", json!({"project_id":project})),
            (
                "query.configuration_defaults",
                json!({"project_id":project}),
            ),
            (
                "query.integrations",
                json!({"project_id":project,"local":true,"include_history":true}),
            ),
            (
                "query.integration_bindings",
                json!({"project_id":project,"local":true,"include_history":true}),
            ),
            ("query.permission_resources", json!({"project_id":project})),
        ];
        for (method, params) in cases {
            let old = w.dispatch_inner(method, params.clone(), None).unwrap();
            let trace = performance::Trace::start();
            let mut current = w.dispatch(method, params).unwrap();
            let measurement = trace.finish();
            for name in [
                "project.full_snapshot",
                "global.full_snapshot",
                "project.transaction",
                "global.transaction",
            ] {
                assert!(
                    measurement["spans"][name].is_null(),
                    "{method}: {measurement}"
                );
            }
            let object = current.as_object_mut().unwrap();
            object.remove("catalog_version");
            object.remove("template_sources");
            if old.get("catalog_generation").is_none() {
                object.remove("catalog_generation");
            }
            assert_eq!(current, old, "read response changed for {method}");
        }
    }
}

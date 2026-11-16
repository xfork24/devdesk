//! Capture committed configuration views once; never synchronize or restore engineering history on a directory read.
use super::*;

pub(super) fn directory_query(method: &str) -> bool {
    matches!(
        method,
        "query.configurations"
            | "query.roles"
            | "query.capabilities"
            | "query.workflows"
            | "query.workflow"
            | "query.workflow_draft"
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
    pub(crate) fn configuration_version(&self, engine: &Engine) -> Result<Value> {
        let global = self.global.configuration_snapshot()?;
        let project = if engine.store.project_id().is_some() {
            engine.store.configuration_snapshot()?.revision
        } else {
            0
        };
        Ok(json!({"host":self.catalog_instance,"global":global.revision,"project":project}))
    }
    pub(super) fn project_directory(&self, params: &Value) -> Result<Value> {
        let offset = params["project_offset"].as_u64().unwrap_or(0) as usize;
        let limit = params["limit"].as_u64().unwrap_or(50).clamp(1, 100) as usize;
        let (projects, total) = if params["github_only"] == true {
            self.global.github_projects(offset, limit)?
        } else {
            self.global.projects(offset, limit)?
        };
        Ok(json!({"projects":projects,"total_projects":total}))
    }
    pub(super) fn ui_bootstrap(&self, params: &Value) -> Result<Value> {
        let _span = performance::Span::new("ui.bootstrap");
        let global = self.global.configuration_snapshot()?;
        let engine = match self.active_engine(params["project_id"].as_str()) {
            Ok(engine) => engine,
            Err(e) if e.code == "project_required" => self.configuration.clone(),
            Err(e) => return Err(e),
        };
        let project_id = engine.store.project_id();
        let view = if let Some(project) = project_id.as_deref() {
            let local = engine.store.configuration_snapshot()?;
            self.configuration_view(&global, &local, project)?
        } else {
            global.clone()
        };
        let current = view.state.projects.first();
        let health = if current.is_some() {
            engine.configuration_status_at(view.clone())?
        } else {
            json!({"ready":false,"issues":[]})
        };
        let mut result = self.project_directory(params)?;
        result["kind"] = json!("ui_bootstrap");
        result["active_project"] = json!(current);
        result["active_project_id"] = json!(project_id);
        result["repository_ready"] = json!(view.repository_ready);
        result["configuration_ready"] = health["ready"].clone();
        result["configuration_status"] = health;
        result["catalog_generation"] = json!(template_catalog_generation(&global.state));
        result["catalog_version"] = json!({"host":self.catalog_instance,"global":global.revision,"project":if project_id.is_some(){view.revision}else{0}});
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
        result["global_concurrency"] = json!(self.global.concurrency()?);
        result["default_harness"] = json!(self.global.default_harness()?);
        result["data_root"] = json!(self.global.root());
        result["execution_mode"] = json!(current.map_or("live", |p| p.execution_mode.as_str()));
        Ok(result)
    }
    fn configuration_view(
        &self,
        global: &Arc<ConfigurationCatalogSnapshot>,
        local: &Arc<ConfigurationCatalogSnapshot>,
        project: &str,
    ) -> Result<Arc<ConfigurationCatalogSnapshot>> {
        let key = format!("{project}:{}:{}", global.revision, local.revision);
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
        let mut state = local.state.as_ref().clone();
        if state.projects.iter().all(|p| p.execution_mode == "live") {
            for record in global.state.governance.iter().filter(|r| {
                r.immutable
                    && (r.r#type == "role"
                        || r.r#type == "role_retirement"
                        || configuration_visible(&global.state, r, Some(project)))
            }) {
                if let Some(old) = local.record(&record.id) {
                    require(old.content_hash == record.content_hash, "共享配置摘要冲突")?;
                } else {
                    state.governance.push(record.clone());
                }
            }
            copy_template_catalog(&global.state, &mut state)?;
        }
        let view = Arc::new(ConfigurationCatalogSnapshot::new(
            state,
            local.revision,
            local.repository_ready,
        )?);
        if serde_json::to_vec(view.state.as_ref())
            .map_err(|e| err("storage", e.to_string()))?
            .len()
            > 4 * 1024 * 1024
        {
            return Ok(view);
        }
        let mut views = self.configuration_views.lock().unwrap();
        if views.len() >= 8 {
            views.remove(0);
        }
        views.push((key, view.clone()));
        Ok(view)
    }

    pub(super) fn configuration_read(&self, method: &str, params: &Value) -> Option<Result<Value>> {
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
            let global = self.global.configuration_snapshot()?;
            let target = params["id"].as_str().or_else(|| params["target"].as_str());
            let project = params["project_id"].as_str();
            let local = match method {
                "query.configuration_defaults" | "query.template_catalog" => false,
                "query.project_configuration" => true,
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
                self.active_engine(project)?
            } else {
                self.configuration.clone()
            };
            let view = if local {
                let local = engine.store.configuration_snapshot()?;
                self.configuration_view(
                    &global,
                    &local,
                    engine.store.project_id().as_deref().unwrap_or("global"),
                )?
            } else {
                global.clone()
            };
            let pinned = engine.at_configuration(view.clone());
            let mut result = if method == "query.configuration_defaults" {
                pinned.configuration_defaults(project)?
            } else if method == "query.template_catalog" {
                let info = template_bundle_info()?;
                json!({"generation":template_catalog_generation(&view.state),"bundle_version":info["bundle_version"],"bundle_hash":info["bundle_hash"],"current_bundle_version":template_catalog_state(&view.state).map(|r|&r.data["bundle_version"]),"counts":template_catalog_counts(&view.state)})
            } else if method == "query.permission_resources" {
                self.permission_resource_api_at(method, params, Some(view.clone()))
                    .expect("permission read")?
            } else if local && matches!(method, "query.configurations" | "query.workflows") {
                local_directory(&view, &global, method, params, project)?
            } else {
                pinned.query(method, params, self.global.concurrency()?)?
            };
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
                object.insert("template_sources".into(), json!(sources));
                object.insert("catalog_version".into(),json!({"host":self.catalog_instance,"global":global.revision,"project":view.revision}));
            }
            Ok(result)
        })())
    }
}

fn local_directory(
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
        (params["include_archived"] == true || template_record_current(state, r))
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
    fn directory_read_does_not_wait_for_definition_writer_or_restore_history() {
        let dir = tempfile::tempdir().unwrap();
        let w = Workspace::open_with_adapters(dir.path(), crate::test_runtime::adapters()).unwrap();
        let project = create(&w, "configuration read isolation");
        let params = json!({"local":true,"project_id":project,"include_history":true,"limit":100});
        w.dispatch("query.configurations", params.clone()).unwrap();
        let gate = w.template_gate.lock().unwrap();
        let (send, receive) = std::sync::mpsc::channel();
        let reader = w.clone();
        let thread = std::thread::spawn(move || {
            let trace = performance::Trace::start();
            let result = reader.dispatch("query.configurations", params);
            send.send((result, trace.finish())).unwrap();
        });
        let received = receive.recv_timeout(std::time::Duration::from_secs(3));
        drop(gate);
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
        let engine = w.active_engine(Some(&project)).unwrap();
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
            "project_changed"
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
}

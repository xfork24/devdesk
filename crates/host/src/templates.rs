//! Reset only the current definition directory; immutable project bindings retain their history.
use super::*;
use std::collections::{BTreeMap, BTreeSet};

impl Workspace {
    fn finish_template_reset(&self) -> Result<()> {
        let discussion = self
            .opened
            .lock()
            .map_err(|error| err("storage", error.to_string()))?
            .get(GLOBAL_DISCUSSION_NAMESPACE)
            .cloned();
        if let Some(engine) = discussion {
            self.sync_configuration(&engine)?;
        }
        let mut offset = 0;
        loop {
            let (projects, total) = self.global.projects(offset, 100)?;
            for project in &projects {
                let engine = self.project_engine(Some(&project.id))?;
                self.sync_configuration(&engine)?;
            }
            offset += projects.len();
            if offset >= total || projects.is_empty() {
                break;
            }
        }
        Ok(())
    }
    fn template_snapshot_summary(&self) -> Result<Value> {
        let catalog = self.global.configuration_snapshot()?;
        let global = catalog.state.as_ref();
        let mut fingerprints = Vec::new();
        let mut counts: BTreeMap<String, usize> = BTreeMap::new();
        let mut custom = 0usize;
        let mut drafts = 0usize;
        let mut seen = BTreeSet::new();
        let mut inspect =
            |namespace: &str, s: &EngineeringState| {
                for r in s.governance.iter().filter(|r| {
                    template_definition_kind(&r.r#type) && template_record_current(s, r)
                }) {
                    fingerprints.push(json!([
                        namespace,
                        r.id,
                        r.revision,
                        r.content_hash,
                        configuration_retired(s, r),
                        template_record_disabled(s, r)
                    ]));
                    if !seen.insert(r.id.clone()) {
                        continue;
                    }
                    if r.r#type == "workflow_draft" && r.data["published_revision_id"].is_null() {
                        drafts += 1;
                    }
                    if s.governance.iter().any(|n| {
                        n.r#type == r.r#type
                            && n.definition_id == r.definition_id
                            && n.version > r.version
                            && !template_history_retained(s, n)
                    }) {
                        continue;
                    }
                    *counts.entry(r.r#type.clone()).or_default() += 1;
                    if template_record_disabled(s, r)
                        || template_catalog_member(s, r)
                            .is_none_or(|m| m.data["source_hash"] != template_source_hash(r))
                    {
                        custom += 1;
                    }
                }
            };
        inspect("global", global);
        let mut projects = Vec::new();
        let mut pristine =
            template_catalog_pristine(global) && !template_catalog_has_custom_history(global)?;
        self.visit_project_catalogs(|p, catalog| {
            let mut s = catalog.state.as_ref().clone();
            // A read-only preview observes the global generation even for a project not yet synchronized.
            copy_template_catalog(global, &mut s)?;
            inspect(&p.id, &s);
            if s.governance
                .iter()
                .any(|r| r.r#type == "project_configuration" && r.definition_id == p.id)
            {
                projects.push(json!({"id":p.id,"title":p.title}));
            }
            pristine &= template_catalog_pristine(&s) && !template_catalog_has_custom_history(&s)?;
            Ok(())
        })?;
        fingerprints.sort_by_key(|v| v.to_string());
        let info = template_bundle_info()?;
        Ok(
            json!({"generation":template_catalog_generation(global),"bundle_version":info["bundle_version"],"bundle_hash":info["bundle_hash"],"restore_counts":template_factory_counts()?,"current_counts":counts,"custom_count":custom,"draft_count":drafts,"pinned_projects":projects,"scope":"dataset","catalog_fingerprint":configuration_hash(&json!({"generation":template_catalog_generation(global),"records":fingerprints})),"pristine":pristine}),
        )
    }
    pub(super) fn template_dispatch(&self, method: &str, params: &Value) -> Option<Result<Value>> {
        if !matches!(
            method,
            "query.template_catalog"
                | "query.template_reset_preview"
                | "configuration.templates.reset"
        ) {
            return None;
        }
        Some((|| {
            if method == "query.template_catalog" {
                let catalog = self.global.configuration_snapshot()?;
                let s = catalog.state.as_ref();
                let info = template_bundle_info()?;
                return Ok(
                    json!({"generation":template_catalog_generation(s),"bundle_version":info["bundle_version"],"bundle_hash":info["bundle_hash"],"current_bundle_version":template_catalog_state(s).map(|r|&r.data["bundle_version"]),"counts":template_catalog_counts(s)}),
                );
            }
            if method == "query.template_reset_preview" {
                return self.template_snapshot_summary();
            }
            let c: Command = serde_json::from_value(params.clone())
                .map_err(|e| err("invalid_params", e.to_string()))?;
            require(!c.command_id.is_empty(), "command_id 不能为空")?;
            let fingerprint = command_fingerprint(method, &c);
            if let Some(saved) = self.global.replay(&c.command_id, &fingerprint)? {
                if c.payload["preserve_custom"] != true {
                    self.finish_template_reset()?;
                }
                return Ok(saved);
            }
            let preview = self.template_snapshot_summary()?;
            let payload = &c.payload;
            if payload["expected_catalog_generation"] != preview["generation"]
                || payload["catalog_fingerprint"] != preview["catalog_fingerprint"]
                || payload["bundle_hash"] != preview["bundle_hash"]
            {
                return Err(err(
                    "catalog_changed",
                    "定义或默认版本已变化，请重新打开重置窗口后再操作",
                ));
            }
            let result = self.configuration.reset_factory_catalog(
                &c,
                &fingerprint,
                &preview,
                &permission_profiles::permission_contract,
            )?;
            if payload["preserve_custom"] != true {
                self.finish_template_reset()?;
            }
            Ok(result)
        })())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::workspace::tests::{create, read_project};
    fn workspace(root: &Path) -> Arc<Workspace> {
        Workspace::open_with_adapters(root, crate::test_runtime::adapters()).unwrap()
    }
    fn write(
        w: &Workspace,
        method: &str,
        target: Option<&str>,
        payload: Value,
        project: Option<&str>,
    ) -> Result<Value> {
        let expected_revision = target.and_then(|id| {
            w.global
                .snapshot()
                .unwrap()
                .governance
                .iter()
                .find(|r| r.id == id)
                .map(|r| {
                    if r.r#type == "permission_profile" {
                        r.version
                    } else {
                        r.revision
                    }
                })
        });
        w.dispatch(method,json!({"command_id":id(),"target":target,"expected_revision":expected_revision,"expected_catalog_generation":template_catalog_generation(&w.global.snapshot().unwrap()),"project_id":project,"payload":payload}))
    }
    fn reset_command(w: &Workspace) -> Value {
        let p = w
            .dispatch("query.template_reset_preview", json!({}))
            .unwrap();
        json!({"command_id":id(),"payload":{"expected_catalog_generation":p["generation"],"catalog_fingerprint":p["catalog_fingerprint"],"bundle_hash":p["bundle_hash"]}})
    }
    #[test]
    fn factory_update_preserves_custom_sources_and_replays_the_reviewed_command() {
        let dir = tempfile::tempdir().unwrap();
        let w = workspace(dir.path());
        let custom = write(&w, "role.revise", Some(&default_role_id("analyst")),
            json!({"name":"Custom analyst","data":{"instructions":"Keep custom responsibility","visibility":{"kind":"global"}}}), None).unwrap();
        let before = w.global.snapshot().unwrap();
        let target = permission_target(
            &before,
            PermissionTargetKind::Sandbox,
            "builtin-sandbox-docker:2",
        )
        .unwrap();
        assert!(active_permission_binding(&before, &target).is_some());
        // Simulate a compatible catalog whose defaults still select detailed sources.
        let retired: Vec<SourceDefinition> = serde_json::from_str(include_str!(
            "../../../tests/fixtures/configuration/retired-engineering.json"
        ))
        .unwrap();
        let mut legacy = before.clone();
        for source in retired {
            legacy.governance.push(source.into_record().unwrap());
        }
        let state = legacy
            .governance
            .iter_mut()
            .find(|r| r.id == TEMPLATE_CATALOG_STATE)
            .unwrap();
        state.data["defaults"]["workflow_ref"] = json!("builtin-workflow-feature-sol-high:12");
        state.data["defaults"]["task_workflow_ref"] = json!("builtin-workflow-task-sol-high:11");
        state.data["defaults"]["standard_ref"] = json!("builtin-standard-engineering:10");
        state.content_hash = content_digest(&state.refs, &state.data);
        let frozen_retired = legacy
            .configuration("builtin-standard-engineering:10")
            .unwrap()
            .clone();
        let receipt = update_factory_catalog(&mut legacy, "factory-test").unwrap();
        assert_eq!(
            legacy.configuration(&frozen_retired.id).unwrap(),
            &frozen_retired
        );
        assert_eq!(receipt["changed"], true);
        assert_eq!(
            current_template_defaults(&legacy).unwrap()["workflow_ref"],
            "builtin-workflow-feature-development:3"
        );
        for source in before.governance.iter().filter(|r| r.immutable) {
            assert_eq!(legacy.configuration(&source.id).unwrap(), source);
        }
        let custom_id = custom["id"].as_str().unwrap();
        assert!(template_record_current(
            &legacy,
            legacy.configuration(custom_id).unwrap()
        ));
        let generation = template_catalog_generation(&legacy);
        assert_eq!(
            update_factory_catalog(&mut legacy, "factory-test-again").unwrap()["changed"],
            false
        );
        assert_eq!(template_catalog_generation(&legacy), generation);
        let mut command = reset_command(&w);
        command["payload"]["preserve_custom"] = json!(true);
        let applied = w
            .dispatch("configuration.templates.reset", command.clone())
            .unwrap();
        assert_eq!(
            w.dispatch("configuration.templates.reset", command)
                .unwrap(),
            applied
        );
        let after = w.global.snapshot().unwrap();
        assert_eq!(
            after.configuration(custom_id).unwrap(),
            before.configuration(custom_id).unwrap()
        );
        assert!(template_record_current(
            &after,
            after.configuration(custom_id).unwrap()
        ));
    }
    #[test]
    fn revising_a_harness_does_not_rebind_defaults_or_grant_permissions() {
        let dir = tempfile::tempdir().unwrap();
        let w = workspace(dir.path());
        let source = w
            .global
            .snapshot()
            .unwrap()
            .configuration(DEFAULT_HARNESS_ID)
            .unwrap()
            .clone();
        let revised = write(
            &w,
            "harness.revise",
            Some(&source.id),
            {
                let mut payload = json!({"name":"New source revision","data":source.data});
                for (key, value) in &source.refs {
                    payload[key] = value.clone();
                }
                payload
            },
            None,
        )
        .unwrap();
        let s = w.global.snapshot().unwrap();
        let target = permission_target(
            &s,
            PermissionTargetKind::Harness,
            revised["id"].as_str().unwrap(),
        )
        .unwrap();
        assert!(active_permission_binding(&s, &target).is_none());
        assert_eq!(
            current_template_defaults(&s).unwrap()["harness_ref"],
            DEFAULT_HARNESS_ID
        );
    }
    #[test]
    fn reset_deletes_custom_definitions_clears_invalid_bindings_and_preserves_execution_records() {
        let dir = tempfile::tempdir().unwrap();
        let w = workspace(dir.path());
        let a = create(&w, "Project A");
        let b = create(&w, "Project B");
        read_project(&w, &a);
        let local=write(&w,"delivery_type.create",None,json!({"name":"Local custom delivery","scope":format!("project:{a}"),"spec_refs":["builtin-spec-stage-prd:4"],"data":{"format":"markdown","body":"custom delivery"}}),Some(&a)).unwrap();
        let role=write(&w,"role.revise",Some(&default_role_id("analyst")),json!({"name":"Edited analyst","data":{"instructions":"custom responsibility","visibility":{"kind":"global"}}}),None).unwrap();
        let defaults = w
            .dispatch("query.configuration_defaults", json!({"project_id":a}))
            .unwrap();
        let engine = w.project_engine(Some(&a)).unwrap();
        let project = engine.store.snapshot().unwrap().projects[0].clone();
        let saved=w.dispatch("project.configuration.update",json!({"command_id":id(),"project_id":a,"target":a,"expected_revision":project.revision,"payload":{"workflow_ref":defaults["workflow_ref"],"standard_ref":null,"role_agent_bindings":defaults["role_agent_bindings"],"test_configuration":crate::test_runtime::fixture_test_configuration()}})).unwrap();
        let feature = engine
            .execute(
                "feature.create",
                Command {
                    command_id: id(),
                    target: None,
                    expected_revision: None,
                    payload: json!({"project_id":a,"title":"Frozen before reset"}),
                },
            )
            .unwrap();
        let frozen = engine
            .store
            .snapshot()
            .unwrap()
            .frozen_test_plan(feature["id"].as_str().unwrap())
            .unwrap();
        assert!(!saved.is_null());
        let before_project = engine.store.snapshot().unwrap();
        let command = reset_command(&w);
        let trace = performance::Trace::start();
        let result = w
            .dispatch("configuration.templates.reset", command.clone())
            .unwrap();
        let report = trace.finish();
        assert!(
            report["spans"]["project.full_snapshot"].is_null(),
            "{report}"
        );
        assert!(
            report["spans"]["global.full_snapshot"].is_null(),
            "{report}"
        );
        assert_eq!(result["generation"], 2);
        assert_eq!(
            w.dispatch("configuration.templates.reset", command)
                .unwrap(),
            result
        );
        let global = w.global.snapshot().unwrap();
        assert!(global.configuration(role["id"].as_str().unwrap()).is_err());
        let roles = w
            .dispatch("query.roles", json!({"limit":100,"include_history":true}))
            .unwrap();
        assert!(
            !roles["records"]
                .as_array()
                .unwrap()
                .iter()
                .any(|r| r["id"] == role["id"])
        );
        let rows = w
            .dispatch(
                "query.configurations",
                json!({"local":true,"project_id":a,"kind":"delivery_type","limit":100}),
            )
            .unwrap();
        assert_eq!(
            rows["total"],
            template_factory_counts().unwrap()["delivery_type"]
        );
        assert!(
            !rows["records"]
                .as_array()
                .unwrap()
                .iter()
                .any(|r| r["id"] == local["id"])
        );
        let pinned = w
            .dispatch("query.project_configuration", json!({"project_id":a}))
            .unwrap();
        assert!(pinned["configuration"].is_null());
        assert_eq!(pinned["uses_archived_templates"], false);
        assert!(
            engine
                .store
                .snapshot()
                .unwrap()
                .configuration(local["id"].as_str().unwrap())
                .is_err()
        );
        let after = engine.store.snapshot().unwrap();
        assert_eq!(
            json!(
                after
                    .frozen_test_plan(feature["id"].as_str().unwrap())
                    .unwrap()
            ),
            json!(frozen)
        );
        assert_eq!(json!(after.features), json!(before_project.features));
        assert_eq!(json!(after.sessions), json!(before_project.sessions));
        assert_eq!(after.executions, before_project.executions);
        assert!(
            engine
                .execute(
                    "feature.create",
                    Command {
                        command_id: id(),
                        target: None,
                        expected_revision: None,
                        payload: json!({"project_id":a,"title":"Requires configuration"}),
                    }
                )
                .is_err()
        );
        let new_defaults = w
            .dispatch("query.configuration_defaults", json!({}))
            .unwrap();
        assert_eq!(new_defaults["workflow_ref"], defaults["workflow_ref"]);
        let shared = w.global.snapshot().unwrap();
        for key in new_defaults["role_agent_bindings"]
            .as_object()
            .unwrap()
            .values()
        {
            let agent_id = key.as_str().unwrap();
            let agent = shared.resolve_agent(agent_id).unwrap();
            let selection = permission_selection(&shared).ok().unwrap();
            assert_eq!(json!(selection.sandbox_ref), new_defaults["sandbox_ref"]);
            let lock = resolve_permission_lock(&shared, &agent.harness_ref, &selection).unwrap();
            assert!(template_record_current(
                &shared,
                shared.configuration(&lock.harness.profile.id).unwrap()
            ));
        }

        read_project(&w, &b);
        let second = w
            .dispatch(
                "query.configurations",
                json!({"local":true,"project_id":b,"kind":"role","limit":100}),
            )
            .unwrap();
        assert_eq!(second["total"], 6);
        assert_eq!(
            w.dispatch("configuration.templates.reset", reset_command(&w))
                .unwrap()["changed"],
            false
        );
        drop(engine);
        drop(w);
        let w = workspace(dir.path());
        read_project(&w, &a);
        assert_eq!(
            w.dispatch("query.roles", json!({"limit":100})).unwrap()["total"],
            6
        );
        assert_eq!(
            w.dispatch("query.project_configuration", json!({"project_id":a}))
                .unwrap()["configuration"],
            Value::Null
        );
        let engine = w.project_engine(Some(&a)).unwrap();
        let project = engine.store.project_metadata().unwrap();
        let defaults = w
            .dispatch("query.configuration_defaults", json!({"project_id":a}))
            .unwrap();
        let saved = w.dispatch("project.configuration.update", json!({"project_id":a,"target":a,"command_id":id(),"expected_revision":project.revision,"expected_catalog_generation":2,"payload":{"standard_ref":defaults["standard_ref"],"role_agent_bindings":defaults["role_agent_bindings"],"harness_permission_ref":defaults["harness_permission_ref"],"test_configuration":crate::test_runtime::fixture_test_configuration()}})).unwrap();
        assert_eq!(saved["data"]["standard_ref"], defaults["standard_ref"]);
        let revised = write(&w, "role.revise", Some(&default_role_id("analyst")), json!({"name":"Can edit after reset","data":{"instructions":"新的职责","visibility":{"kind":"global"}}}), None).unwrap();
        assert_eq!(revised["id"], role["id"]);
    }
    #[test]
    fn historical_session_keeps_its_agent_and_role_after_reset() {
        let dir = tempfile::tempdir().unwrap();
        let w = workspace(dir.path());
        let project = create(&w, "Historical session");
        let base_role = default_role_id("analyst");
        let role = write(&w, "role.revise", Some(&base_role),
            json!({"name":"Historical role","data":{"instructions":"Pinned instructions","visibility":{"kind":"global"}}}), None).unwrap();
        let agent = write(&w, "agent.revise", Some("builtin-agent-gpt-6-2-sol-analyst:3"),
            json!({"name":"Historical agent","role_ref":role["id"],"capability_ref":"builtin-capability-analyst-gpt-6-2-sol-codex:3","data":{}}), None).unwrap();
        let engine = w.project_engine(Some(&project)).unwrap();
        w.sync_configuration(&engine).unwrap();
        let agent_ref = agent["id"].as_str().unwrap();
        let (session, _) = crate::test_runtime::queue_conversation_for_agent(
            &engine,
            &project,
            agent_ref,
            |_, _| Ok(()),
        );
        let before = engine.store.snapshot().unwrap();
        let original = before
            .sessions
            .iter()
            .find(|row| row.id == session)
            .unwrap();
        w.dispatch("configuration.templates.reset", reset_command(&w))
            .unwrap();
        let after = engine.store.snapshot().unwrap();
        assert_eq!(
            json!(after.sessions.iter().find(|row| row.id == session).unwrap()),
            json!(original)
        );
        let archived = after.configuration(agent_ref).unwrap();
        assert!(template_history_retained(&after, archived));
        assert!(!template_record_current(&after, archived));
        assert_eq!(
            after.resolve_agent(agent_ref).unwrap().role_ref.as_deref(),
            role["id"].as_str()
        );
        assert!(!template_catalog_has_custom_history(&after).unwrap());
        let preview = w
            .dispatch("query.template_reset_preview", json!({}))
            .unwrap();
        assert_eq!(preview["pristine"], true);
    }

    #[test]
    fn reset_reads_stay_consistent_until_each_store_adopts_the_new_catalog() {
        let dir = tempfile::tempdir().unwrap();
        let w = workspace(dir.path());
        let project = create(&w, "Catalog reset");
        let engine = w.project_engine(Some(&project)).unwrap();
        let discussion = w.initialize_global_discussion().unwrap();
        let publish_reset = |params: Value,
                             preview: &Value,
                             contract: &dyn Fn(
            &EngineeringState,
            &PermissionTargetRef,
        ) -> Result<String>| {
            let command: Command = serde_json::from_value(params).unwrap();
            w.configuration
                .reset_factory_catalog(
                    &command,
                    &command_fingerprint("configuration.templates.reset", &command),
                    preview,
                    contract,
                )
                .unwrap()
        };
        let mut preview = w.template_snapshot_summary().unwrap();
        preview["pristine"] = json!(false);
        // Simulate an older installation's binding contract for the same Sandbox.
        publish_reset(reset_command(&w), &preview, &|_, _| {
            Ok("previous-permission-contract".into())
        });
        w.finish_template_reset().unwrap();
        let role = write(&w, "role.revise", Some(&default_role_id("analyst")),
            json!({"name":"Old catalog role","data":{"instructions":"Old catalog responsibilities","visibility":{"kind":"global"}}}), None).unwrap();
        let role_id = role["id"].as_str().unwrap();
        w.sync_configuration(&engine).unwrap();
        let stale = engine.store.configuration_snapshot().unwrap();
        assert!(stale.record(role_id).is_some());
        let sandbox = current_template_defaults(&stale.state).unwrap()["sandbox_ref"]
            .as_str()
            .unwrap()
            .to_owned();
        let binding_id = format!("permission-binding:{sandbox}:1");
        let old_global = w.global.configuration_snapshot().unwrap();

        // The global transaction has committed, while the project and discussion
        // stores still have the previous catalog.
        let command = reset_command(&w);
        let receipt = publish_reset(
            command.clone(),
            &w.template_snapshot_summary().unwrap(),
            &permission_profiles::permission_contract,
        );
        assert_eq!(receipt["changed"], true);
        let global = w.global.configuration_snapshot().unwrap();
        let old_binding = stale.record(&binding_id).unwrap();
        let new_binding = global.record(&binding_id).unwrap();
        assert_ne!(old_binding.content_hash, new_binding.content_hash);
        let view = compose_configuration_catalog(&global, &stale, &project).unwrap();
        assert_eq!(view.record(&binding_id), Some(old_binding));
        assert!(view.record(role_id).is_some());
        assert!(
            compose_role_directory(&global, &[stale])
                .unwrap()
                .record(role_id)
                .is_none()
        );
        w.dispatch("query.ui_bootstrap", json!({"project_id":project}))
            .unwrap();

        // Replaying a committed reset must finish synchronization and preserve its receipt.
        assert_eq!(
            w.dispatch("configuration.templates.reset", command)
                .unwrap(),
            receipt
        );
        for store in [&engine.store, &discussion.store] {
            let catalog = store.configuration_snapshot().unwrap();
            assert_eq!(catalog.record(&binding_id), Some(new_binding));
            assert!(catalog.record(role_id).is_none());
        }
        let current = engine.store.configuration_snapshot().unwrap();
        let view = compose_configuration_catalog(&old_global, &current, &project).unwrap();
        assert_eq!(view.record(&binding_id), Some(new_binding));
        assert!(view.record(role_id).is_none());
        w.dispatch("query.ui_bootstrap", json!({"project_id":project}))
            .unwrap();
    }

    #[test]
    fn stale_preview_and_stale_pages_cannot_publish_and_failed_transaction_rolls_back() {
        let dir = tempfile::tempdir().unwrap();
        let w = workspace(dir.path());
        let old = reset_command(&w);
        write(
            &w,
            "delivery_type.create",
            None,
            json!({"name":"Extra","spec_refs":["builtin-spec-stage-prd:4"],"data":{"format":"markdown","body":"extra"}}),
            None,
        )
        .unwrap();
        assert_eq!(
            w.dispatch("configuration.templates.reset", old)
                .unwrap_err()
                .code,
            "catalog_changed"
        );
        let before = w.global.snapshot().unwrap();
        assert!(
            w.global
                .seed_fixture_state(None, &mut |s| {
                    restore_template_catalog(s, "rollback")?;
                    Err(err("fixture", "rollback"))
                })
                .is_err()
        );
        assert_eq!(json!(w.global.snapshot().unwrap()), json!(before));
        w.dispatch("configuration.templates.reset", reset_command(&w))
            .unwrap();
        let stale=w.dispatch("delivery_type.create",json!({"command_id":id(),"expected_catalog_generation":1,"payload":{"name":"Stale","spec_refs":["builtin-spec-stage-prd:4"],"data":{"format":"markdown","body":"stale"}}})).unwrap_err();
        assert_eq!(stale.code, "catalog_changed");
        assert_eq!(w.dispatch("role.revise",json!({"command_id":id(),"target":default_role_id("analyst"),"expected_revision":1,"expected_catalog_generation":1,"payload":{"name":"Stale edit","data":{"instructions":"old body","visibility":{"kind":"global"}}}})).unwrap_err().code,"catalog_changed");
    }

    #[test]
    fn every_definition_kind_and_mcp_binding_reset_even_when_only_names_changed() {
        let dir = tempfile::tempdir().unwrap();
        let w = workspace(dir.path());
        w.dispatch("configuration.templates.reset", reset_command(&w))
            .unwrap();
        assert!(template_catalog_pristine(&w.global.snapshot().unwrap()));
        let initial = w.global.snapshot().unwrap();
        for kind in [
            "model",
            "harness",
            "sandbox",
            "tool",
            "skill",
            "spec",
            "standard",
            "capability",
            "agent",
            "role",
            "delivery_type",
        ] {
            let source = initial
                .governance
                .iter()
                .filter(|r| r.r#type == kind && template_record_current(&initial, r))
                .max_by_key(|r| r.version)
                .unwrap();
            let (method, payload) = if kind == "standard" {
                (
                    "standard.capture".to_owned(),
                    json!({"name":"Customized standard", "root_workflow_refs":reference_value(&source.refs,"root_workflow_refs")}),
                )
            } else {
                (format!("{kind}.revise"), {
                    let mut payload =
                        json!({"name":format!("Customized {kind}"),"data":source.data});
                    for (key, value) in &source.refs {
                        payload[key] = value.clone();
                    }
                    payload
                })
            };
            write(&w, &method, Some(&source.id), payload, None).unwrap();
        }
        let profile = initial
            .governance
            .iter()
            .find(|r| r.r#type == "permission_profile" && template_record_current(&initial, r))
            .unwrap();
        write(&w,"permission.profile.revise",Some(&profile.id),json!({"name":"Customized permission","target_kind":profile.data["target_kind"],"policy":profile.data["policy"]}),None).unwrap();
        let source=write(&w,"integration.create",None,json!({"name":"Fixture MCP","data":{"source":{"kind":"mcp","transport":"streamable_http","url":"https://mcp.example.invalid","command":[]}}}),None).unwrap();
        let defaults = w
            .dispatch("query.configuration_defaults", json!({}))
            .unwrap();
        write(&w,"integration.binding.create",None,json!({"name":"Fixture MCP binding","data":{"integration_ref":source["id"],"harness_ref":defaults["harness_ref"],"sandbox_ref":defaults["sandbox_ref"],"allowed_origins":["https://mcp.example.invalid"],"allowed_tools":[]}}),None).unwrap();
        let workflow = initial
            .configuration(defaults["workflow_ref"].as_str().unwrap())
            .unwrap();
        write(
            &w,
            "workflow.create",
            None,
            json!({"name":"Customized draft","document":workflow.data}),
            None,
        )
        .unwrap();
        assert!(!template_catalog_pristine(&w.global.snapshot().unwrap()));
        let result = w
            .dispatch("configuration.templates.reset", reset_command(&w))
            .unwrap();
        assert_eq!(
            result["generation"],
            template_catalog_generation(&initial) + 1
        );
        let current = w
            .dispatch(
                "query.configurations",
                json!({"limit":100,"include_history":true}),
            )
            .unwrap();
        assert!(
            !current["records"]
                .as_array()
                .unwrap()
                .iter()
                .any(|r| r["name"].as_str().unwrap().starts_with("Customized"))
        );
        assert_eq!(
            w.dispatch("query.integrations", json!({})).unwrap()["total"],
            0
        );
        assert_eq!(
            w.dispatch("query.integration_bindings", json!({})).unwrap()["total"],
            0
        );
        assert_eq!(
            w.dispatch("configuration.templates.reset", reset_command(&w))
                .unwrap()["changed"],
            false
        );
        let state = w.global.snapshot().unwrap();
        let native = state
            .governance
            .iter()
            .find(|r| r.r#type == "tool" && template_record_current(&state, r))
            .unwrap();
        let preview = reset_command(&w);
        write(&w, "integration.disable", Some(&native.id), json!({}), None).unwrap();
        assert_eq!(
            w.dispatch("configuration.templates.reset", preview)
                .unwrap_err()
                .code,
            "catalog_changed"
        );
        assert!(!template_catalog_pristine(&w.global.snapshot().unwrap()));
        assert_eq!(
            w.dispatch("configuration.templates.reset", reset_command(&w))
                .unwrap()["changed"],
            true
        );
    }
}

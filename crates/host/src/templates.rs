//! Reset only the current definition directory; immutable project bindings retain their history.
use super::*;
use std::collections::{BTreeMap, BTreeSet};

impl Workspace {
    fn template_snapshot_summary(&self) -> Result<Value> {
        let global = self.global.snapshot()?;
        let mut fingerprints = Vec::new();
        let mut counts: BTreeMap<String, usize> = BTreeMap::new();
        let mut custom = 0usize;
        let mut drafts = 0usize;
        let mut seen = BTreeSet::new();
        let mut inspect = |namespace: &str, s: &EngineeringState| {
            for r in s
                .governance
                .iter()
                .filter(|r| template_definition_kind(&r.kind) && template_record_current(s, r))
            {
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
                if r.kind == "workflow_draft" && r.data["published_revision_id"].is_null() {
                    drafts += 1;
                }
                if s.governance.iter().any(|n| {
                    n.kind == r.kind && n.definition_id == r.definition_id && n.version > r.version
                }) {
                    continue;
                }
                *counts.entry(r.kind.clone()).or_default() += 1;
                if template_record_disabled(s, r)
                    || template_catalog_member(s, r)
                        .is_none_or(|m| m.data["source_hash"] != template_source_hash(r))
                {
                    custom += 1;
                }
            }
        };
        inspect("global", &global);
        let mut projects = Vec::new();
        let mut pristine = template_catalog_pristine(&global);
        let mut offset = 0;
        loop {
            let (page, total) = self.global.projects(offset, 100)?;
            for p in &page {
                let mut s =
                    SqliteStore::read_project_snapshot(Path::new(&self.global.root()), &p.id)?;
                // A read-only preview observes the global generation even for a project not yet synchronized.
                copy_template_catalog(&global, &mut s)?;
                inspect(&p.id, &s);
                if s.governance
                    .iter()
                    .any(|r| r.kind == "project_configuration" && r.definition_id == p.id)
                {
                    projects.push(json!({"id":p.id,"title":p.title}));
                }
                pristine &= template_catalog_pristine(&s);
            }
            offset += page.len();
            if offset >= total || page.is_empty() {
                break;
            }
        }
        fingerprints.sort_by_key(|v| v.to_string());
        let info = template_bundle_info()?;
        Ok(
            json!({"generation":template_catalog_generation(&global),"bundle_version":info["bundle_version"],"bundle_hash":info["bundle_hash"],"restore_counts":template_factory_counts()?,"current_counts":counts,"custom_count":custom,"draft_count":drafts,"pinned_projects":projects,"scope":"dataset","catalog_fingerprint":configuration_hash(&json!({"generation":template_catalog_generation(&global),"records":fingerprints})),"pristine":pristine}),
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
                let s = self.global.snapshot()?;
                let info = template_bundle_info()?;
                return Ok(
                    json!({"generation":template_catalog_generation(&s),"bundle_version":info["bundle_version"],"bundle_hash":info["bundle_hash"],"current_bundle_version":template_catalog_state(&s).map(|r|&r.data["bundle_version"]),"counts":template_catalog_counts(&s)}),
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
                    "定义或默认版本已变化，请重新预览后再确认重置",
                ));
            }
            self.global.transact(Some((&c.command_id,&fingerprint)),&mut |s| {
                if preview["pristine"]==true {return Ok(json!({"generation":preview["generation"],"changed":false,"counts":preview["current_counts"],"bundle_version":preview["bundle_version"]}));}
                let result=restore_template_catalog(s,&c.command_id)?;
                template_permission_defaults(s)?;
                Ok(result)
            })
        })())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::workspace::tests::{create, select};
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
                    if r.kind == "permission_profile" {
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
            json!({"name":"New source revision","data":source.data}),
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
    fn reset_archives_global_and_project_definitions_preserves_bindings_and_replays() {
        let dir = tempfile::tempdir().unwrap();
        let w = workspace(dir.path());
        let a = create(&w, "Project A");
        let b = create(&w, "Project B");
        select(&w, &a);
        let local=write(&w,"delivery_type.create",None,json!({"name":"Local custom delivery","scope":{"kind":"project","project_id":a},"data":{"format":"markdown","body":"custom delivery"}}),Some(&a)).unwrap();
        let role=write(&w,"role.revise",Some(&default_role_id("analyst")),json!({"name":"Edited analyst","data":{"instructions":"custom responsibility","visibility":{"kind":"global"}}}),None).unwrap();
        let defaults = w
            .dispatch("query.configuration_defaults", json!({"project_id":a}))
            .unwrap();
        let engine = w.active_engine(Some(&a)).unwrap();
        let project = engine.store.snapshot().unwrap().projects[0].clone();
        let saved=w.dispatch("project.configuration.update",json!({"command_id":id(),"project_id":a,"target":a,"expected_revision":project.revision,"payload":{"workflow_ref":defaults["workflow_ref"],"standard_ref":null,"role_agent_bindings":defaults["role_agent_bindings"]}})).unwrap();
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
        let before = w.global.snapshot().unwrap();
        let before_project = engine.store.snapshot().unwrap();
        let command = reset_command(&w);
        let result = w
            .dispatch("configuration.templates.reset", command.clone())
            .unwrap();
        assert_eq!(result["generation"], 2);
        assert_eq!(
            w.dispatch("configuration.templates.reset", command)
                .unwrap(),
            result
        );
        let global = w.global.snapshot().unwrap();
        for old in before.governance.iter().filter(|r| r.immutable) {
            assert_eq!(global.configuration(&old.id).unwrap(), old);
        }
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
        assert_eq!(pinned["configuration"], saved);
        assert_eq!(pinned["uses_archived_templates"], false);
        for old in before_project.governance.iter().filter(|r| r.immutable) {
            assert_eq!(
                engine
                    .store
                    .snapshot()
                    .unwrap()
                    .configuration(&old.id)
                    .unwrap(),
                old
            );
        }
        let after = engine.store.snapshot().unwrap();
        assert_eq!(
            json!(
                after
                    .frozen_test_plan(feature["id"].as_str().unwrap())
                    .unwrap()
            ),
            json!(frozen)
        );
        let continued = engine
            .execute(
                "feature.create",
                Command {
                    command_id: id(),
                    target: None,
                    expected_revision: None,
                    payload: json!({"project_id":a,"title":"Old project after reset"}),
                },
            )
            .unwrap();
        assert_eq!(
            engine
                .store
                .snapshot()
                .unwrap()
                .frozen_test_plan(continued["id"].as_str().unwrap())
                .unwrap()
                .workflow_revision_id,
            frozen.workflow_revision_id
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

        select(&w, &b);
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
        select(&w, &a);
        assert_eq!(
            w.dispatch("query.roles", json!({"limit":100})).unwrap()["total"],
            6
        );
        assert_eq!(
            w.dispatch("query.project_configuration", json!({"project_id":a}))
                .unwrap()["configuration"],
            saved
        );
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
            json!({"name":"Extra","data":{"format":"markdown","body":"extra"}}),
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
                .transact(None, &mut |s| {
                    restore_template_catalog(s, "rollback")?;
                    Err(err("fixture", "rollback"))
                })
                .is_err()
        );
        assert_eq!(json!(w.global.snapshot().unwrap()), json!(before));
        w.dispatch("configuration.templates.reset", reset_command(&w))
            .unwrap();
        let stale=w.dispatch("delivery_type.create",json!({"command_id":id(),"expected_catalog_generation":1,"payload":{"name":"Stale","data":{"format":"markdown","body":"stale"}}})).unwrap_err();
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
                .filter(|r| r.kind == kind && template_record_current(&initial, r))
                .max_by_key(|r| r.version)
                .unwrap();
            write(
                &w,
                &format!("{kind}.revise"),
                Some(&source.id),
                json!({"name":format!("Customized {kind}"),"data":source.data}),
                None,
            )
            .unwrap();
        }
        let profile = initial
            .governance
            .iter()
            .find(|r| r.kind == "permission_profile" && template_record_current(&initial, r))
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
            .find(|r| r.kind == "tool" && template_record_current(&state, r))
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

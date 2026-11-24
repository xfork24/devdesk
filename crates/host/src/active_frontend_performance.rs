//! Opt-in local HTTP regression fixture, with real resource commits and scheduler.
//! Control steps have no provider I/O. This is not the full browser/W2/P6 workload.
#[path = "performance_history.rs"]
mod history;
#[path = "performance_repository.rs"]
mod repository;
use super::*;
use devdesk_core::{Command, Engine, concurrency::*, id};
use devdesk_domain::configuration_hash;

fn command(engine: &Engine, method: &str, target: Option<&str>, payload: Value) -> Value {
    let revision = target.and_then(|target| {
        let mut keys = vec![
            ResourceKey::new("governance", target),
            ResourceKey::new("project", target),
            ResourceKey::new("feature", target),
        ];
        if target
            .rsplit_once(':')
            .is_some_and(|(_, n)| n.parse::<u64>().is_ok_and(|n| n > 0))
        {
            keys.push(ResourceKey::new("source", target));
        }
        engine
            .store
            .read_resources(&keys)
            .unwrap()
            .iter()
            .find_map(|r| r.value.as_ref().and_then(|v| v["revision"].as_u64()))
    });
    engine
        .execute(
            method,
            Command {
                command_id: id(),
                target: target.map(str::to_owned),
                expected_revision: revision,
                payload,
            },
        )
        .unwrap()
}

fn seed_contexts(engine: &Engine, project: &str, owner: &Value, count: usize) -> Vec<Value> {
    let mut contexts = Vec::with_capacity(count);
    let changes = (0..count).map(|index| {
        let context = id();
        contexts.push(json!({"project_id":project,"owner_id":owner,"id":context,"revision":1}));
        ResourceChange {
            resource: ResourceKey::new("context", &context),
            value: json!({"id":context,"owner_id":owner,"content":format!("Client context {index}"),"revision":1}),
        }
    }).collect::<Vec<_>>();
    engine
        .store
        .commit_changes(&PreparedChangeSet {
            command_id: id(),
            fingerprint: configuration_hash(&changes),
            read_set: ReadSet::new(
                changes
                    .iter()
                    .map(|change| ResourceVersion {
                        resource: change.resource.clone(),
                        revision: 0,
                    })
                    .collect(),
            )
            .unwrap(),
            changes,
            result: Value::Null,
            time: 1,
        })
        .unwrap();
    contexts
}

#[test]
#[ignore = "explicit fresh-dataset local 100-client HTTP regression; no external providers"]
fn active_frontend_server() {
    let root = PathBuf::from(
        std::env::var("DEVDESK_ACTIVE_FRONTEND_DATASET")
            .expect("dedicated empty fixture directory"),
    );
    assert!(
        root.parent()
            .unwrap()
            .join(".active-frontends-fixture")
            .is_file()
    );
    let reopen = std::env::var("DEVDESK_PERFORMANCE_OPEN_SEED").is_ok_and(|v| v == "1");
    if !reopen {
        assert_eq!(
            std::fs::read_dir(&root).unwrap().count(),
            0,
            "never seed an existing dataset"
        );
    }
    let seed = root.parent().unwrap().join("performance-seed.json");
    let real_runtime = std::env::var("DEVDESK_PERFORMANCE_REAL_RUNTIME").is_ok_and(|v| v == "1");
    let hot_project = std::env::var("DEVDESK_PERFORMANCE_HOT_PROJECT").is_ok_and(|v| v == "1");
    let workspace = if real_runtime {
        Workspace::open(&root).unwrap()
    } else {
        Workspace::open_with_adapters(&root, test_runtime::adapters()).unwrap()
    };
    let mut projects = vec![];
    let mut roots = vec![];
    let mut contexts = vec![];
    let mut d2_contexts = vec![];
    let scale: usize = std::env::var("DEVDESK_PERFORMANCE_HISTORY_SCALE")
        .ok()
        .map(|v| v.parse().unwrap())
        .unwrap_or(0);
    let project_count: usize = std::env::var("DEVDESK_PERFORMANCE_PROJECTS")
        .ok()
        .map(|v| v.parse().unwrap())
        .unwrap_or(2);
    assert!([0, 1, 10].contains(&scale) && [2, 10].contains(&project_count));
    let step_count = 256;
    if !reopen {
        for project_index in 0..project_count {
            let project=workspace.dispatch("project.create",json!({"command_id":id(),"payload":{"title":format!("Local HTTP fixture {project_index}")}})).unwrap()["id"].as_str().unwrap().to_owned();
            let engine = workspace.project_engine(Some(&project)).unwrap();
            if real_runtime {
                test_runtime::retired_configuration::install(&engine);
            }
            test_runtime::bind_default_permissions(&engine);
            let steps = if real_runtime {
                repository::seed(&workspace, &root.canonicalize().unwrap(), &project);
                let image = std::env::var("DEVDESK_PERFORMANCE_TEST_IMAGE")
                    .expect("explicit pinned test image");
                let mut steps = vec![
                    json!({"key":"current","name":"Actual Git revision","stage":"work","executor":{"type":"tool","tool_ref":"builtin-tool-workspace-current:2"},"outputs":{"workspace":{"delivery_ref":"builtin-delivery-workspace-snapshot:3"}}}),
                    json!({"key":"snapshot","name":"Actual frozen files","stage":"work","dependencies":["current"],"executor":{"type":"tool","tool_ref":"builtin-tool-workspace-snapshot:2","arguments":{"workspace":{"type":"source","source":{"step":"current","port":"workspace"}},"allowed_paths":{"type":"constant","value":["probe.test.mjs","note.txt"]},"include_diff":{"type":"constant","value":false}}},"outputs":{"workspace":{"delivery_ref":"builtin-delivery-workspace-snapshot:3"}}}),
                ];
                steps.extend((0..step_count-2).map(|index|json!({"key":format!("docker-{index}"),"name":format!("Actual Docker test {index}"),"stage":"work","dependencies":[if index==0{"snapshot".into()}else{format!("docker-{}",index-1)}],"executor":{"type":"tool","tool_ref":"builtin-tool-tests-run:2","arguments":{"workspace":{"type":"source","source":{"step":"snapshot","port":"workspace"}},"recipe":{"type":"constant","value":{"runner":"node_test","files":["probe.test.mjs"],"image":image,"timeout_seconds":30}}}},"outputs":{"report":{"delivery_ref":"builtin-delivery-test-report:2"}},"completion":{"op":"equals","source":{"step":format!("docker-{index}"),"port":"report","pointer":"/status"},"value":"succeeded"}})));
                steps
            } else {
                (0..step_count).map(|index|json!({"key":format!("step-{index}"),"name":format!("Control {index}"),"stage":"work","dependencies":if index==0{vec![]}else{vec![format!("step-{}",index-1)]},"executor":{"type":"control"}})).collect::<Vec<_>>()
            };
            let draft = command(
                &engine,
                "workflow.create",
                None,
                json!({"name":"Local control progression","document":{"schema_version":6,"stages":[{"key":"work","name":"Control"}],"steps":steps}}),
            );
            let workflow = command(&engine, "workflow.publish", draft["id"].as_str(), json!({}));
            let defaults = engine.configuration_defaults(Some(&project)).unwrap();
            command(
                &engine,
                "project.configuration.update",
                Some(&project),
                json!({"workflow_ref":workflow["id"],"role_agent_bindings":defaults["role_agent_bindings"],"orchestration":{"mode":"manual"}}),
            );
            for member in 0..if scale == 0 {
                if hot_project { 4 } else { 2 }
            } else {
                4 * scale
            } {
                let active = if hot_project {
                    project_index == 0 && member < 4
                } else {
                    project_index < 2 && member < 2
                };
                let feature = command(
                    &engine,
                    "feature.create",
                    None,
                    json!({"project_id":project,"title":format!("Control member {member}"),"initial_requirement":"Explicit control-only HTTP regression fixture","start_workflow":false}),
                );
                if real_runtime && active {
                    repository::feature(&engine, feature["id"].as_str().unwrap());
                }
                let run = command(&engine, "workflow.start", feature["id"].as_str(), json!({}));
                let header = engine
                    .store
                    .read_resources(&[ResourceKey::new("governance", run["id"].as_str().unwrap())])
                    .unwrap()
                    .remove(0)
                    .value
                    .unwrap();
                if active {
                    command(
                        &engine,
                        "workflow.driver.select",
                        run["id"].as_str(),
                        json!({"mode":"rust","expected_driver_epoch":0,"plan_revision_id":header["data"]["active_plan_revision_id"],"control_version":header["data"]["control_version"]}),
                    );
                }
                if active {
                    let writer = command(
                        &engine,
                        "feature.create",
                        None,
                        json!({"project_id":project,"title":format!("Independent editable load targets {member}"),"start_workflow":false}),
                    );
                    roots.push(
                        json!({"project_id":project,"id":run["id"],"feature_id":feature["id"],"write_owner_id":writer["id"]}),
                    );
                    contexts.extend(seed_contexts(&engine, &project, &writer["id"], 25));
                }
            }
            if real_runtime && project_count == 10 {
                let writer = command(
                    &engine,
                    "feature.create",
                    None,
                    json!({
                        "project_id":project,"title":"Independent D2 load targets","start_workflow":false
                    }),
                );
                d2_contexts.push(seed_contexts(&engine, &project, &writer["id"], 10));
            }
            if scale > 0 {
                history::seed(&engine, &project, scale);
            }
            projects.push(project);
        }
    }
    if project_count == 10 && scale > 0 {
        let discussion = workspace.performance_discussion_fixture().unwrap();
        if !reopen {
            test_runtime::bind_default_permissions(&discussion);
            history::seed(&discussion, devdesk_domain::GLOBAL_DISCUSSION_NAMESPACE, 1);
        }
    }
    let fixture = if reopen {
        let fixture: Value = serde_json::from_slice(&std::fs::read(&seed).unwrap()).unwrap();
        for project in fixture["projects"].as_array().unwrap() {
            workspace.project_engine(project.as_str()).unwrap();
        }
        fixture
    } else {
        let fixture = json!({"projects":projects,"roots":roots,"contexts":contexts,"d2_contexts":d2_contexts,"scheduler":true,"external_providers":real_runtime,"history_scale":scale,"hot_project":hot_project});
        std::fs::write(&seed, serde_json::to_vec(&fixture).unwrap()).unwrap();
        fixture
    };
    if std::env::var("DEVDESK_PERFORMANCE_PREPARE_ONLY").is_ok_and(|v| v == "1") {
        println!("PREPARED_FRONTEND_FIXTURE={fixture}");
        return;
    }
    tokio::runtime::Runtime::new().unwrap().block_on(async {
        let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
        let address = listener.local_addr().unwrap();
        let app = Arc::new(App {
            lifecycle: lifecycle::Lifecycle::new(true),
            workspace: RwLock::new(Some(workspace)),
            cookie: format!("devdesk_session_{}=active-fixture", address.port()),
            authority: address.to_string(),
            origin: format!("http://{address}"),
            access_token: None,
            web_root: None,
            trusted_domains: Default::default(),
            domain_epoch: tokio::sync::watch::channel(0).0,
            stopping: AtomicBool::new(false),
            shutdown: tokio::sync::Notify::new(),
        });
        let task = tokio::spawn(scheduler_loop(app.clone()));
        println!("ACTIVE_FRONTEND_FIXTURE={}", {
            let mut metadata = fixture.clone();
            metadata["origin"] = json!(app.origin);
            metadata
        });
        let exit = app.clone();
        axum::serve(listener, router(app.clone()))
            .with_graceful_shutdown(async move {
                tokio::signal::ctrl_c().await.unwrap();
                exit.stopping.store(true, Ordering::Relaxed);
                exit.shutdown.notify_waiters();
            })
            .await
            .unwrap();
        task.await.unwrap();
        app.workspace().unwrap().stop_owned_runtime().unwrap();
    });
}

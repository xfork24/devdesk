//! Opt-in local HTTP regression fixture, with real resource commits and scheduler.
//! Control steps have no provider I/O. This is not the full browser/W2/P6 workload.
use super::*;
use devdesk_core::{Command, Engine, concurrency::*, id};
use devdesk_domain::configuration_hash;

fn command(engine: &Engine, method: &str, target: Option<&str>, payload: Value) -> Value {
    let state = engine.store.snapshot().unwrap();
    let revision = target.and_then(|target| {
        state
            .configuration(target)
            .ok()
            .map(|r| r.revision)
            .or_else(|| {
                state
                    .projects
                    .iter()
                    .find(|p| p.id == target)
                    .map(|p| p.revision)
            })
            .or_else(|| {
                state
                    .features
                    .iter()
                    .find(|f| f.id == target)
                    .map(|f| f.revision)
            })
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
    assert_eq!(
        std::fs::read_dir(&root).unwrap().count(),
        0,
        "never open an existing dataset in this fixture"
    );
    let workspace = Workspace::open_with_adapters(&root, test_runtime::adapters()).unwrap();
    let mut projects = vec![];
    let mut roots = vec![];
    let mut contexts = vec![];
    for project_index in 0..2 {
        let project=workspace.dispatch("project.create",json!({"command_id":id(),"payload":{"title":format!("Local HTTP fixture {project_index}")}})).unwrap()["id"].as_str().unwrap().to_owned();
        let engine = workspace.project_engine(Some(&project)).unwrap();
        test_runtime::bind_default_permissions(&engine);
        let steps=(0..256).map(|index|json!({"key":format!("step-{index}"),"name":format!("Control {index}"),"stage":"work","dependencies":if index==0{vec![]}else{vec![format!("step-{}",index-1)]},"executor":{"type":"control"}})).collect::<Vec<_>>();
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
        for member in 0..2 {
            let feature = command(
                &engine,
                "feature.create",
                None,
                json!({"project_id":project,"title":format!("Control member {member}"),"initial_requirement":"Explicit control-only HTTP regression fixture","start_workflow":false}),
            );
            let run = command(&engine, "workflow.start", feature["id"].as_str(), json!({}));
            let header = engine
                .store
                .snapshot()
                .unwrap()
                .configuration(run["id"].as_str().unwrap())
                .unwrap()
                .clone();
            command(
                &engine,
                "workflow.driver.select",
                run["id"].as_str(),
                json!({"mode":"rust","expected_driver_epoch":0,"plan_revision_id":header.data["active_plan_revision_id"],"control_version":header.data["control_version"]}),
            );
            roots.push(json!({"project_id":project,"id":run["id"]}));
            let changes=(0..25).map(|index| {
                let context=id();
                contexts.push(json!({"project_id":project,"id":context,"revision":1}));
                ResourceChange{resource:ResourceKey::new("context",&context),value:json!({"id":context,"owner_id":feature["id"],"content":format!("Client {member}/{index}"),"revision":1})}
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
        }
        projects.push(project);
    }
    tokio::runtime::Runtime::new().unwrap().block_on(async {
        let listener=tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
        let address=listener.local_addr().unwrap();
        let app=Arc::new(App{
            lifecycle:lifecycle::Lifecycle::new(true),workspace:RwLock::new(Some(workspace)),
            cookie:format!("devdesk_session_{}=active-fixture",address.port()),authority:address.to_string(),origin:format!("http://{address}"),access_token:None,
            stopping:AtomicBool::new(false),shutdown:tokio::sync::Notify::new(),
        });
        let scheduler=app.clone();
        let task=tokio::spawn(async move {
            let mut timer=tokio::time::interval(std::time::Duration::from_millis(75));
            loop {
                timer.tick().await;
                if scheduler.stopping.load(Ordering::Relaxed) {break;}
                let app=scheduler.clone();
                let result=tokio::task::spawn_blocking(move||app.workspace()?.tick(||app.stopping.load(Ordering::Relaxed))).await.unwrap();
                if let Err(error)=result {eprintln!("fixture scheduler: {error}");}
            }
        });
        println!("ACTIVE_FRONTEND_FIXTURE={}",json!({"origin":app.origin,"projects":projects,"roots":roots,"contexts":contexts,"scheduler":true,"external_providers":false}));
        let exit=app.clone();
        axum::serve(listener,router(app.clone())).with_graceful_shutdown(async move {
            tokio::signal::ctrl_c().await.unwrap();
            exit.stopping.store(true,Ordering::Relaxed);
            exit.shutdown.notify_waiters();
        }).await.unwrap();
        task.await.unwrap();
        app.workspace().unwrap().stop_owned_runtime().unwrap();
    });
}

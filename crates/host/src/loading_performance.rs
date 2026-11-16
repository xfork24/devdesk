//! Explicit, offline performance entrypoints. Only a copied, compatible dataset is accepted.
use super::*;
use devdesk_adapters::performance;
use std::{io::Write, time::Instant};

fn dataset() -> PathBuf {
    let root = PathBuf::from(
        std::env::var("DEVDESK_PERF_DATASET")
            .expect("set DEVDESK_PERF_DATASET to an isolated SQLite backup"),
    );
    assert!(
        root.join(".performance-copy").is_file(),
        "performance entrypoint requires .performance-copy marker"
    );
    root
}

#[test]
#[ignore = "explicit offline benchmark, requires a marked dataset copy"]
fn configuration_load_profile() {
    let w = Workspace::loading_profile(&dataset()).unwrap();
    let pid = w.active_id().expect("benchmark needs an active project");
    let output = std::env::var("DEVDESK_PERF_OUTPUT").expect("set DEVDESK_PERF_OUTPUT");
    let mut file = std::fs::File::create(output).unwrap();
    let samples = std::env::var("DEVDESK_PERF_SAMPLES")
        .ok()
        .and_then(|v| v.parse::<usize>().ok())
        .unwrap_or(30)
        .clamp(1, 1000);
    let cases = [
        (
            "query.configurations",
            json!({"local":true,"project_id":pid,"limit":100,"include_history":true}),
        ),
        (
            "query.configurations",
            json!({"local":true,"project_id":pid,"limit":100,"offset":100,"include_history":true}),
        ),
        ("query.standard_catalog", json!({"project_id":pid})),
        ("query.configuration_defaults", json!({"project_id":pid})),
        (
            "query.workflows",
            json!({"local":true,"project_id":pid,"limit":100,"include_history":true}),
        ),
        ("query.project_configuration", json!({"project_id":pid})),
        ("query.workspace", json!({"project_id":pid,"limit":50})),
        ("query.model_providers", json!({})),
        ("query.permission_resources", json!({"project_id":pid})),
    ];
    for (method, params) in cases {
        for sample in 0..samples {
            let trace = performance::Trace::start();
            let start = Instant::now();
            let result = w.dispatch(method, params.clone()).unwrap();
            let bytes = serde_json::to_vec(&result).unwrap().len();
            let ms = start.elapsed().as_secs_f64() * 1000.0;
            let measurement = trace.finish();
            writeln!(file,"{}",json!({"method":method,"params":params,"sample":sample,"ms":ms,"bytes":bytes,"measurement":measurement})).unwrap();
        }
    }
    let defaults = w
        .dispatch(
            "query.configuration_defaults",
            json!({"project_id":pid,"view":"selection"}),
        )
        .unwrap();
    let reference = w
        .global
        .configuration_snapshot()
        .unwrap()
        .records("role")
        .next()
        .unwrap()
        .id
        .clone();
    let diagnostics = [
        ("query.configuration_index", json!({})),
        ("query.configuration_index", json!({"project_id":pid})),
        (
            "query.configuration_records",
            json!({"project_id":pid,"refs":[reference]}),
        ),
        (
            "query.configuration_defaults",
            json!({"project_id":pid,"view":"selection"}),
        ),
        (
            "query.project_configuration",
            json!({"project_id":pid,"view":"selection"}),
        ),
        (
            "query.standard",
            json!({"project_id":pid,"id":defaults["standard_ref"],"view":"summary","limit":1000}),
        ),
        (
            "query.project_standard_preview",
            json!({"project_id":pid,"standard_ref":defaults["standard_ref"],"harness_permission_ref":defaults["harness_permission_ref"],"standard_overrides":[],"view":"summary"}),
        ),
    ];
    for (method, params) in diagnostics {
        for sample in 0..samples {
            let trace = performance::Trace::start();
            let start = Instant::now();
            let result = w.dispatch(method, params.clone()).unwrap();
            let bytes = serde_json::to_vec(&result).unwrap().len();
            writeln!(file,"{}",json!({"method":method,"params":params,"sample":sample,"ms":start.elapsed().as_secs_f64()*1000.0,"bytes":bytes,"measurement":trace.finish()})).unwrap();
        }
    }
    let e = w.active_engine(Some(&pid)).unwrap();
    for sample in 0..samples {
        let trace = performance::Trace::start();
        let start = Instant::now();
        let result = e
            .query("query.events", &json!({"cursor":u64::MAX,"limit":100}), 4)
            .unwrap();
        writeln!(file,"{}",json!({"method":"query.events","sample":sample,"ms":start.elapsed().as_secs_f64()*1000.0,"bytes":result.to_string().len(),"measurement":trace.finish()})).unwrap();
    }
}

#[test]
#[ignore = "explicit offline browser benchmark server, no scheduler or execution"]
fn configuration_load_server() {
    let w = Workspace::loading_profile(&dataset()).unwrap();
    tokio::runtime::Runtime::new().unwrap().block_on(async {
        let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
        let address = listener.local_addr().unwrap();
        let app = Arc::new(App {
            workspace: RwLock::new(Some(w)),
            cookie: format!("devdesk_session_{}=profile", address.port()),
            authority: address.to_string(),
            origin: format!("http://{address}"),
            access_token: None,
            stopping: AtomicBool::new(false),
            shutdown: tokio::sync::Notify::new(),
        });
        println!("PERF_ORIGIN=http://{address}");
        axum::serve(listener, router(app.clone()))
            .with_graceful_shutdown(async move {
                tokio::signal::ctrl_c().await.unwrap();
                app.stopping.store(true, Ordering::Relaxed);
            })
            .await
            .unwrap();
    });
}

#[test]
#[ignore = "explicit source scaling profile"]
fn configuration_source_scaling() {
    let w = Workspace::loading_profile(&dataset()).unwrap();
    let snapshot = w.global.configuration_snapshot().unwrap();
    let role = snapshot.records("role").next().unwrap().clone();
    for count in [200, 2000, 10000] {
        let mut state = snapshot.state.as_ref().clone();
        for n in state.governance.len()..count {
            let mut row = role.clone();
            row.definition_id = format!("scaling-role-{n}");
            row.id = format!("{}:1", row.definition_id);
            row.version = 1;
            state.governance.push(row);
        }
        let started = Instant::now();
        let catalog = devdesk_core::ConfigurationCatalogSnapshot::new(state, 1, true).unwrap();
        let construction = started.elapsed().as_secs_f64() * 1000.0;
        let started = Instant::now();
        let result =
            devdesk_core::configuration_index(&catalog, &json!({"limit":1000}), None).unwrap();
        println!(
            "{}",
            json!({"sources":count,"construction_ms":construction,"index_ms":started.elapsed().as_secs_f64()*1000.0,"page_bytes":result.to_string().len(),"total":result["total"]})
        );
    }
}

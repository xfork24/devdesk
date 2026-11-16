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

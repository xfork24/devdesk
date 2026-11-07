use axum::{
    Json, Router,
    extract::{Path, Query, Request, State},
    http::{HeaderValue, StatusCode, header},
    middleware::{self, Next},
    response::{
        IntoResponse, Response,
        sse::{Event, KeepAlive, Sse},
    },
    routing::{get, post},
};
use devdesk_adapters::*;
use devdesk_core::*;
use devdesk_domain::{Error, err};
use futures_util::stream;
use serde_json::{Value, json};
use std::{
    collections::{HashMap, VecDeque},
    path::PathBuf,
    sync::{
        Arc, RwLock,
        atomic::{AtomicBool, AtomicUsize, Ordering},
    },
};
#[derive(rust_embed::RustEmbed)]
#[folder = "../../web/dist/"]
struct Web;
struct App {
    engine: RwLock<Option<Arc<Engine>>>,
    cookie: String,
    authority: String,
    global: AtomicUsize,
    stopping: AtomicBool,
    shutdown: tokio::sync::Notify,
}
fn open(path: &std::path::Path) -> devdesk_domain::Result<Arc<Engine>> {
    let store = Arc::new(SqliteStore::open(path)?);
    let artifacts = Arc::new(FileArtifacts::new(PathBuf::from(store.root())));
    let engine = Arc::new(Engine {
        store,
        artifacts,
        harness: Arc::new(SimHarness),
        git: Arc::new(SimGit),
        tests: Arc::new(SimTests),
        sandbox: Arc::new(SimSandbox),
    });
    engine.bootstrap()?;
    Ok(engine)
}
impl App {
    fn engine(&self) -> devdesk_domain::Result<Arc<Engine>> {
        self.engine
            .read()
            .unwrap()
            .clone()
            .ok_or_else(|| err("setup_required", "请先初始化或打开数据集"))
    }
}
fn failure(e: Error) -> Value {
    json!({"code":match e.code.as_str(){"method_not_found"=>-32601,"invalid_params"=>-32602,_=>-32000},"message":e.message,"data":{"code":e.code}})
}
async fn guard(State(app): State<Arc<App>>, request: Request, next: Next) -> Response {
    let host = request
        .headers()
        .get(header::HOST)
        .and_then(|v| v.to_str().ok())
        .unwrap_or("");
    if host != app.authority {
        return (StatusCode::FORBIDDEN, "Host rejected").into_response();
    }
    if let Some(origin) = request.headers().get(header::ORIGIN) {
        if origin.to_str().unwrap_or("") != format!("http://{}", app.authority) {
            return (StatusCode::FORBIDDEN, "Origin rejected").into_response();
        }
    }
    let path = request.uri().path();
    let protected = path == "/rpc"
        || path == "/events"
        || path == "/setup"
        || path == "/shutdown"
        || path == "/capabilities"
        || path.starts_with("/artifacts/");
    let authenticated = request
        .headers()
        .get(header::COOKIE)
        .and_then(|v| v.to_str().ok())
        .is_some_and(|v| v.split(';').any(|pair| pair.trim() == app.cookie));
    if protected && !authenticated {
        return (StatusCode::UNAUTHORIZED, "Local session required").into_response();
    }
    let is_page = !protected && !path.contains('.');
    let mut response = next.run(request).await;
    response.headers_mut().insert(
        "x-content-type-options",
        HeaderValue::from_static("nosniff"),
    );
    response
        .headers_mut()
        .insert("referrer-policy", HeaderValue::from_static("no-referrer"));
    response.headers_mut().insert("content-security-policy",HeaderValue::from_static("default-src 'self'; script-src 'self'; style-src 'self' 'unsafe-inline'; connect-src 'self'; img-src 'self' data:; frame-ancestors 'none'; base-uri 'self'; form-action 'self'"));
    if protected || is_page {
        response
            .headers_mut()
            .insert(header::CACHE_CONTROL, HeaderValue::from_static("no-store"));
    }
    if is_page {
        response.headers_mut().insert(
            header::SET_COOKIE,
            HeaderValue::from_str(&format!(
                "{}; HttpOnly; SameSite=Strict; Path=/",
                app.cookie
            ))
            .unwrap(),
        );
    }
    response
}
async fn static_file(request: Request) -> Response {
    let path = request.uri().path().trim_start_matches('/');
    let key = if path.is_empty() { "index.html" } else { path };
    let file = Web::get(key).or_else(|| {
        if !key.contains('.') {
            Web::get("index.html")
        } else {
            None
        }
    });
    match file {
        Some(file) => {
            let mime = if key == "index.html" || !key.contains('.') {
                "text/html".to_string()
            } else {
                mime_guess::from_path(key)
                    .first_or_octet_stream()
                    .to_string()
            };
            ([(header::CONTENT_TYPE, mime)], file.data.into_owned()).into_response()
        }
        None => (StatusCode::NOT_FOUND, "Not found").into_response(),
    }
}
async fn rpc(
    State(app): State<Arc<App>>,
    request: std::result::Result<Json<Value>, axum::extract::rejection::JsonRejection>,
) -> Json<Value> {
    let request = match request {
        Ok(Json(value)) => value,
        Err(_) => {
            return Json(
                json!({"jsonrpc":"2.0","id":null,"error":{"code":-32700,"message":"Parse error"}}),
            );
        }
    };
    let id = request.get("id").cloned().unwrap_or(Value::Null);
    if request["jsonrpc"] != "2.0" || !request["method"].is_string() || request.get("id").is_none()
    {
        return Json(
            json!({"jsonrpc":"2.0","id":id,"error":{"code":-32600,"message":"Invalid request"}}),
        );
    }
    let method = request["method"].as_str().unwrap().to_string();
    let params = request.get("params").cloned().unwrap_or(json!({}));
    let a = app.clone();
    let result = tokio::task::spawn_blocking(move || -> devdesk_domain::Result<Value> {
        let engine = a.engine()?;
        if method == "settings.update" {
            let n = params["global_concurrency"]
                .as_u64()
                .ok_or_else(|| err("invalid_params", "需要并发额度"))?;
            devdesk_domain::require((1..=32).contains(&n), "全局并发额度需为 1—32")?;
            let runtime_root = PathBuf::from(engine.store.root());
            let pending = runtime_root.join(format!("runtime-{}.pending", uuid::Uuid::new_v4()));
            std::fs::write(&pending, json!({"global_concurrency":n}).to_string())
                .map_err(|e| err("storage", e.to_string()))?;
            std::fs::rename(&pending, runtime_root.join("runtime.json"))
                .map_err(|e| err("storage", e.to_string()))?;
            a.global.store(n as usize, Ordering::Relaxed);
            return Ok(json!({"global_concurrency":n}));
        }
        if method.starts_with("query.") {
            engine.query(&method, &params, a.global.load(Ordering::Relaxed))
        } else {
            let command: Command =
                serde_json::from_value(params).map_err(|e| err("invalid_params", e.to_string()))?;
            engine.execute(&method, command)
        }
    })
    .await;
    let result = result.unwrap_or_else(|e| Err(err("internal", e.to_string())));
    Json(match result {
        Ok(result) => json!({"jsonrpc":"2.0","id":id,"result":result}),
        Err(e) => json!({"jsonrpc":"2.0","id":id,"error":failure(e)}),
    })
}
async fn setup(State(app): State<Arc<App>>, Json(p): Json<Value>) -> Json<Value> {
    let mut current = app.engine.write().unwrap();
    if current.is_some() {
        return Json(json!({"error":"已有数据集已打开；更改路径需退出后使用 --data 重启"}));
    }
    let Some(path) = p["path"].as_str().filter(|s| !s.trim().is_empty()) else {
        return Json(json!({"error":"请输入数据目录"}));
    };
    match open(&PathBuf::from(path)) {
        Ok(engine) => {
            load_settings(&app, &engine);
            let root = engine.store.root();
            *current = Some(engine);
            Json(json!({"root":root}))
        }
        Err(e) => Json(json!({"error":e.message})),
    }
}
fn load_settings(app: &App, engine: &Engine) {
    if let Ok(body) =
        std::fs::read_to_string(PathBuf::from(engine.store.root()).join("runtime.json"))
    {
        if let Ok(v) = serde_json::from_str::<Value>(&body) {
            if let Some(n) = v["global_concurrency"]
                .as_u64()
                .filter(|n| (1..=32).contains(n))
            {
                app.global.store(n as usize, Ordering::Relaxed);
            }
        }
    }
}
async fn capabilities(State(app): State<Arc<App>>) -> Json<Value> {
    Json(
        json!({"version":"0.2.0","protocol_version":1,"setup_required":app.engine.read().unwrap().is_none(),"execution_mode":"simulation","live":"unavailable","archive":"preview","migration":"preview","platform":{"linux":"runtime_verified_by_reports","macos":"runtime_verified_by_reports"},"runtime":{"os":std::env::consts::OS,"arch":std::env::consts::ARCH},"harness":{"id":"sim-harness-v1","capabilities":["structured-output","cancel","hold","fault-injection"]},"sandbox":{"id":"sim-sandbox-v1","capabilities":["logical-environment","resource-cleanup"]}}),
    )
}
async fn artifact(State(app): State<Arc<App>>, Path(id): Path<String>) -> Response {
    let result = (|| {
        let e = app.engine()?;
        let v = e.query("query.artifact", &json!({"id":id}), 4)?;
        let a: devdesk_domain::Artifact =
            serde_json::from_value(v).map_err(|e| err("internal", e.to_string()))?;
        e.artifacts.read(&a)
    })();
    match result {
        Ok(bytes) => (
            [
                (header::CONTENT_TYPE, "text/plain; charset=utf-8"),
                (header::CONTENT_DISPOSITION, "inline"),
            ],
            bytes,
        )
            .into_response(),
        Err(e) => (StatusCode::NOT_FOUND, Json(failure(e))).into_response(),
    }
}
async fn events(
    State(app): State<Arc<App>>,
    Query(p): Query<HashMap<String, String>>,
    headers: axum::http::HeaderMap,
) -> impl IntoResponse {
    let cursor = headers
        .get("last-event-id")
        .and_then(|v| v.to_str().ok())
        .and_then(|v| v.parse::<u64>().ok())
        .or_else(|| p.get("cursor").and_then(|s| s.parse().ok()))
        .unwrap_or(0);
    let stream = stream::unfold(
        (app, cursor, VecDeque::<Value>::new()),
        |(app, mut cursor, mut queue)| async move {
            loop {
                if app.stopping.load(Ordering::Relaxed) {
                    return None;
                }
                if let Some(value) = queue.pop_front() {
                    cursor = value["id"].as_u64().unwrap_or(cursor);
                    return Some((
                        Ok::<_, std::convert::Infallible>(
                            Event::default()
                                .id(cursor.to_string())
                                .data(value.to_string()),
                        ),
                        (app, cursor, queue),
                    ));
                }
                if let Ok(e) = app.engine() {
                    if let Ok(Value::Array(values)) =
                        e.query("query.events", &json!({"cursor":cursor,"limit":100}), 4)
                    {
                        queue.extend(values);
                        if !queue.is_empty() {
                            continue;
                        }
                    }
                }
                tokio::time::sleep(std::time::Duration::from_millis(250)).await;
            }
        },
    );
    Sse::new(stream).keep_alive(KeepAlive::default())
}
async fn shutdown(State(app): State<Arc<App>>) -> Json<Value> {
    app.stopping.store(true, Ordering::Relaxed);
    app.shutdown.notify_one();
    Json(json!({"ok":true}))
}
#[tokio::main]
async fn main() {
    if let Err(e) = run().await {
        eprintln!("DevDesk: {e}");
        std::process::exit(1);
    }
}
async fn run() -> Result<(), Box<dyn std::error::Error>> {
    let args = std::env::args().skip(1).collect::<Vec<_>>();
    if args.iter().any(|a| a == "--help") {
        println!(
            "DevDesk 0.2\nUsage: devdesk-host [--data PATH] [--port 4317]\nWithout --data, choose a dataset in the Web setup page. Only simulation mode is supported."
        );
        return Ok(());
    }
    let option = |name: &str| {
        args.iter()
            .position(|a| a == name)
            .and_then(|i| args.get(i + 1))
            .cloned()
    };
    let port = option("--port")
        .unwrap_or_else(|| "4317".into())
        .parse::<u16>()?;
    let engine = option("--data")
        .map(|p| open(&PathBuf::from(p)))
        .transpose()?;
    let listener = tokio::net::TcpListener::bind((std::net::Ipv4Addr::LOCALHOST, port)).await?;
    let authority = format!("127.0.0.1:{}", listener.local_addr()?.port());
    let app = Arc::new(App {
        engine: RwLock::new(engine),
        cookie: format!("devdesk_session={}", uuid::Uuid::new_v4()),
        authority: authority.clone(),
        global: AtomicUsize::new(4),
        stopping: AtomicBool::new(false),
        shutdown: tokio::sync::Notify::new(),
    });
    if let Ok(e) = app.engine() {
        load_settings(&app, &e);
    }
    let router = Router::new()
        .route("/rpc", post(rpc))
        .route("/setup", post(setup))
        .route("/shutdown", post(shutdown))
        .route("/capabilities", get(capabilities))
        .route("/events", get(events))
        .route("/artifacts/{id}", get(artifact))
        .fallback(static_file)
        .layer(middleware::from_fn_with_state(app.clone(), guard))
        .with_state(app.clone());
    let scheduler = app.clone();
    let task = tokio::spawn(async move {
        let mut timer = tokio::time::interval(std::time::Duration::from_millis(75));
        loop {
            timer.tick().await;
            if scheduler.stopping.load(Ordering::Relaxed) {
                break;
            }
            let a = scheduler.clone();
            let result = tokio::task::spawn_blocking(move || {
                if a.stopping.load(Ordering::Relaxed) {
                    return Ok(());
                }
                if let Ok(e) = a.engine() {
                    e.tick(a.global.load(Ordering::Relaxed))
                } else {
                    Ok(())
                }
            })
            .await;
            if let Ok(Err(e)) = result {
                eprintln!("scheduler: {e}");
            }
        }
    });
    println!(
        "DevDesk · simulation\nhttp://{authority}\nRuntime: {} / {}",
        std::env::consts::OS,
        std::env::consts::ARCH
    );
    let exit = app.clone();
    axum::serve(listener, router)
        .with_graceful_shutdown(async move {
            tokio::select! {_=tokio::signal::ctrl_c()=>{},_=exit.shutdown.notified()=>{}}
            exit.stopping.store(true, Ordering::Relaxed);
        })
        .await?;
    task.abort();
    Ok(())
}

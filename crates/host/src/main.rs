#[cfg(test)]
#[path = "../../../tests/support/runtime.rs"]
mod test_runtime;
mod workspace;
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
use devdesk_domain::{Error, err};
use futures_util::stream;
use serde_json::{Value, json};
use std::{
    collections::{HashMap, VecDeque},
    path::PathBuf,
    sync::{
        Arc, RwLock,
        atomic::{AtomicBool, Ordering},
    },
};
use workspace::Workspace;
#[derive(rust_embed::RustEmbed)]
#[folder = "../../web/dist/"]
struct Web;
struct App {
    workspace: RwLock<Option<Arc<Workspace>>>,
    cookie: String,
    authority: String,
    stopping: AtomicBool,
    shutdown: tokio::sync::Notify,
}
impl App {
    fn workspace(&self) -> devdesk_domain::Result<Arc<Workspace>> {
        self.workspace
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
        a.workspace()?.dispatch(&method, params)
    })
    .await;
    let result = result.unwrap_or_else(|e| Err(err("internal", e.to_string())));
    Json(match result {
        Ok(result) => json!({"jsonrpc":"2.0","id":id,"result":result}),
        Err(e) => json!({"jsonrpc":"2.0","id":id,"error":failure(e)}),
    })
}
async fn setup(State(app): State<Arc<App>>, Json(p): Json<Value>) -> Json<Value> {
    let mut current = app.workspace.write().unwrap();
    if current.is_some() {
        return Json(json!({"error":"已有数据集已打开；更改路径需退出后使用 --data 重启"}));
    }
    let Some(path) = p["path"].as_str().filter(|s| !s.trim().is_empty()) else {
        return Json(json!({"error":"请输入数据目录"}));
    };
    match Workspace::open(&PathBuf::from(path)) {
        Ok(workspace) => {
            let root = devdesk_core::EngineeringStore::root(&*workspace.global);
            *current = Some(workspace);
            Json(json!({"root":root}))
        }
        Err(e) => Json(json!({"error":e.message})),
    }
}
async fn capabilities(State(app): State<Arc<App>>) -> Json<Value> {
    let workspace = app.workspace.read().unwrap().clone();
    Json(
        json!({"version":"0.2.0","protocol_version":1,"setup_required":workspace.is_none(),"active_project_id":workspace.as_ref().and_then(|w|w.active_id()),"storage_layout":"global-and-project-databases","execution_mode":"live","live":"requires_registered_runtime_and_credentials","conversations":{"providers":["codex","dsh"],"managed_linux_workers":true,"engineering_run_contract":true},"archive":"preview","migration":"preview","platform":{"linux":"runtime_verified_by_reports","macos":"runtime_verified_by_reports"},"runtime":{"os":std::env::consts::OS,"arch":std::env::consts::ARCH},"harness":{"providers":["codex","dsh"],"capabilities":["structured-output","cancel","poll"]},"sandbox":{"provider":"docker","capabilities":["managed-container","resource-cleanup"]}}),
    )
}
async fn artifact(
    State(app): State<Arc<App>>,
    Path(id): Path<String>,
    Query(p): Query<HashMap<String, String>>,
) -> Response {
    let result = (|| {
        let e = app
            .workspace()?
            .active_engine(p.get("project_id").map(String::as_str))?;
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
) -> Response {
    let pinned = app
        .workspace()
        .and_then(|w| w.event_engine(p.get("project_id").map(String::as_str)));
    let (scope, engine) = match pinned {
        Ok(pinned) => pinned,
        Err(e) => return (StatusCode::CONFLICT, Json(failure(e))).into_response(),
    };
    let cursor = headers
        .get("last-event-id")
        .and_then(|v| v.to_str().ok())
        .and_then(|v| v.split_once(':'))
        .filter(|(id, _)| *id == scope)
        .and_then(|(_, n)| n.parse::<u64>().ok())
        .or_else(|| p.get("cursor").and_then(|s| s.parse().ok()))
        .unwrap_or(0);
    let stream = stream::unfold(
        (app, engine, scope, cursor, VecDeque::<Value>::new()),
        |(app, engine, scope, mut cursor, mut queue)| async move {
            loop {
                if app.stopping.load(Ordering::Relaxed) {
                    return None;
                }
                if let Some(mut value) = queue.pop_front() {
                    cursor = value["id"].as_u64().unwrap_or(cursor);
                    value["project_id"] = json!(scope);
                    return Some((
                        Ok::<_, std::convert::Infallible>(
                            Event::default()
                                .id(format!("{scope}:{cursor}"))
                                .data(value.to_string()),
                        ),
                        (app, engine, scope, cursor, queue),
                    ));
                }
                if let Ok(Value::Array(values)) =
                    engine.query("query.events", &json!({"cursor":cursor,"limit":100}), 4)
                {
                    queue.extend(values);
                    if !queue.is_empty() {
                        continue;
                    }
                }
                tokio::time::sleep(std::time::Duration::from_millis(250)).await;
            }
        },
    );
    Sse::new(stream)
        .keep_alive(KeepAlive::default())
        .into_response()
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
            "DevDesk 0.2\nUsage: devdesk-host [--data PATH] [--port 4317]\nWithout --data, choose a dataset in the Web setup page. Web UI opens GitHub projects with native Codex/DSH execution."
        );
        return Ok(());
    }
    let option = |name: &str| {
        args.iter()
            .position(|a| a == name)
            .and_then(|i| args.get(i + 1))
            .cloned()
    };
    if let Some(reference) = option("--authorize-credential") {
        let dataset =
            option("--data").ok_or("credential authorization requires an existing dataset")?;
        devdesk_adapters::credentials::CredentialVault::new(&PathBuf::from(dataset))?
            .authorize_interactively(&reference)?;
        println!("credential_authorized");
        return Ok(());
    }
    let port = option("--port")
        .unwrap_or_else(|| "4317".into())
        .parse::<u16>()?;
    let workspace = option("--data")
        .map(|p| Workspace::open(&PathBuf::from(p)))
        .transpose()?;
    let listener = tokio::net::TcpListener::bind((std::net::Ipv4Addr::LOCALHOST, port)).await?;
    let authority = format!("127.0.0.1:{}", listener.local_addr()?.port());
    let app = Arc::new(App {
        workspace: RwLock::new(workspace),
        cookie: format!("devdesk_session={}", uuid::Uuid::new_v4()),
        authority: authority.clone(),
        stopping: AtomicBool::new(false),
        shutdown: tokio::sync::Notify::new(),
    });
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
                if let Ok(w) = a.workspace() {
                    w.tick()
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
        "DevDesk · local engineering\nhttp://{authority}\nRuntime: {} / {}",
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
    app.stopping.store(true, Ordering::Relaxed);
    let _ = task.await;
    if let Ok(workspace) = app.workspace() {
        tokio::task::spawn_blocking(move || workspace.stop_owned_runtime()).await??;
    }
    Ok(())
}

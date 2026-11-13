#[cfg(test)]
#[path = "../../../tests/support/defaults.rs"]
mod default_fixture;
#[cfg(test)]
#[path = "../../../tests/support/runtime.rs"]
mod test_runtime;
mod workspace;
use axum::{
    Json, Router,
    extract::{DefaultBodyLimit, Path, Query, Request, State},
    http::{HeaderValue, StatusCode, header},
    middleware::{self, Next},
    response::{
        IntoResponse, Response,
        sse::{Event, KeepAlive, Sse},
    },
    routing::{get, post},
};
use devdesk_domain::{Error, HARNESS_PROVIDERS, err};
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
    origin: String,
    access_token: Option<String>,
    stopping: AtomicBool,
    shutdown: tokio::sync::Notify,
}
impl App {
    fn session_cookie(&self) -> HeaderValue {
        HeaderValue::from_str(&format!(
            "{}; HttpOnly; SameSite=Strict; Path=/{}",
            self.cookie,
            if self.origin.starts_with("https://") {
                "; Secure"
            } else {
                ""
            },
        ))
        .expect("generated session cookie")
    }
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
    let path = request.uri().path();
    let discovery = path == "/connection" && request.method() == axum::http::Method::GET;
    let host = request
        .headers()
        .get(header::HOST)
        .and_then(|v| v.to_str().ok())
        .unwrap_or("");
    if host != app.authority {
        return (StatusCode::FORBIDDEN, "Host rejected").into_response();
    }
    if let Some(origin) = request.headers().get(header::ORIGIN) {
        if !discovery && origin.to_str().unwrap_or("") != app.origin {
            return (StatusCode::FORBIDDEN, "Origin rejected").into_response();
        }
    }
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
    let is_page = !protected && path != "/connection" && path != "/session" && !path.contains('.');
    let mut response = next.run(request).await;
    response.headers_mut().insert(
        "x-content-type-options",
        HeaderValue::from_static("nosniff"),
    );
    response
        .headers_mut()
        .insert("referrer-policy", HeaderValue::from_static("no-referrer"));
    response.headers_mut().insert("content-security-policy",HeaderValue::from_static("default-src 'self'; script-src 'self'; style-src 'self' 'unsafe-inline'; connect-src 'self' http: https:; img-src 'self' data:; frame-ancestors 'none'; base-uri 'self'; form-action 'self'"));
    if protected || is_page {
        response
            .headers_mut()
            .insert(header::CACHE_CONTROL, HeaderValue::from_static("no-store"));
    }
    if is_page && (app.access_token.is_none() || authenticated) {
        response
            .headers_mut()
            .insert(header::SET_COOKIE, app.session_cookie());
    }
    response
}
// Discovery carries no dataset, account, session or project information. It is
// the only cross-origin endpoint; actual work stays on the selected origin.
async fn connection(State(app): State<Arc<App>>) -> Response {
    let status = if app.stopping.load(Ordering::Relaxed) {
        StatusCode::SERVICE_UNAVAILABLE
    } else {
        StatusCode::OK
    };
    (
        status,
        [(header::ACCESS_CONTROL_ALLOW_ORIGIN, "*"), (header::CACHE_CONTROL, "no-store")],
        Json(json!({"product":"devdesk","protocol_version":1,"authentication_required":app.access_token.is_some()})),
    ).into_response()
}
async fn session(State(app): State<Arc<App>>, Json(body): Json<Value>) -> Response {
    if let Some(expected) = &app.access_token {
        let provided = body["access_token"].as_str().unwrap_or("");
        let matches = provided.len() == expected.len()
            && provided
                .as_bytes()
                .iter()
                .zip(expected.as_bytes())
                .fold(0u8, |difference, (a, b)| difference | (a ^ b))
                == 0;
        if !matches {
            return (
                StatusCode::UNAUTHORIZED,
                Json(json!({"error":"authentication_required"})),
            )
                .into_response();
        }
    }
    let mut response = Json(json!({"ok":true})).into_response();
    response
        .headers_mut()
        .insert(header::SET_COOKIE, app.session_cookie());
    response
        .headers_mut()
        .insert(header::CACHE_CONTROL, HeaderValue::from_static("no-store"));
    response
}

fn connection_origin(value: &str) -> devdesk_domain::Result<String> {
    let url = url::Url::parse(value).map_err(|_| {
        err(
            "invalid_params",
            "无效的 --public-url，需为 http(s)://主机:端口",
        )
    })?;
    if !matches!(url.scheme(), "http" | "https")
        || url.host_str().is_none()
        || !url.username().is_empty()
        || url.password().is_some()
        || url.path() != "/"
        || url.query().is_some()
        || url.fragment().is_some()
        || url.port() == Some(0)
    {
        return Err(err(
            "invalid_params",
            "--public-url 仅包含 http(s)://主机:端口，不包含路径、账号或参数",
        ));
    }
    Ok(url.origin().ascii_serialization())
}

fn connection_key(
    listen: std::net::IpAddr,
    public_url: bool,
    value: Option<String>,
) -> devdesk_domain::Result<Option<String>> {
    if listen.is_unspecified() && !public_url {
        return Err(err(
            "invalid_params",
            "监听所有网卡时需用 --public-url 指定客户端连接地址",
        ));
    }
    if let Some(value) = value {
        if !(32..=512).contains(&value.len()) || !value.bytes().all(|b| b.is_ascii_graphic()) {
            return Err(err(
                "invalid_params",
                "DEVDESK_ACCESS_TOKEN 需为 32–512 位非空白 ASCII 字符",
            ));
        }
        Ok(Some(value))
    } else if !listen.is_loopback() || public_url {
        Err(err(
            "authentication_required",
            "远程监听需先配置 DEVDESK_ACCESS_TOKEN（至少 32 位随机连接密钥）",
        ))
    } else {
        Ok(None)
    }
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
async fn integration_oauth_callback(
    State(app): State<Arc<App>>,
    Query(params): Query<HashMap<String, String>>,
) -> Response {
    let result = if params.contains_key("error") {
        Err(err(
            "oauth_denied",
            "授权服务未完成授权，请回到接入页面重新授权",
        ))
    } else if let (Some(state), Some(code)) = (params.get("state"), params.get("code")) {
        let app = app.clone();
        let (state, code) = (state.clone(), code.clone());
        tokio::task::spawn_blocking(move || {
            app.workspace()?.integration_oauth_callback(&state, &code)
        })
        .await
        .unwrap_or_else(|_| Err(err("oauth_failed", "授权交换未完成")))
    } else {
        Err(err("oauth_state", "缺少授权状态或授权码"))
    };
    match result {
        Ok(_) => (
            StatusCode::OK,
            "授权已完成。请关闭此页，回到 DevDesk 接入页面保存新的绑定修订。".to_owned(),
        )
            .into_response(),
        Err(e) => (
            StatusCode::BAD_REQUEST,
            format!(
                "授权未完成（{}）。请回到 DevDesk 接入页面重新授权。",
                e.code
            ),
        )
            .into_response(),
    }
}
async fn capabilities(State(app): State<Arc<App>>) -> Json<Value> {
    let workspace = app.workspace.read().unwrap().clone();
    let providers: Vec<&str> = HARNESS_PROVIDERS.iter().map(|p| p.as_str()).collect();
    Json(
        json!({"version":"0.2.0","protocol_version":1,"setup_required":workspace.is_none(),"active_project_id":workspace.as_ref().and_then(|w|w.active_id()),"storage_layout":"global-and-project-databases","execution_mode":"live","live":"requires_registered_runtime_and_credentials","conversations":{"providers":providers,"managed_linux_workers":true,"engineering_run_contract":true},"platform":{"linux":"runtime_verified_by_reports","macos":"runtime_verified_by_reports"},"runtime":{"os":std::env::consts::OS,"arch":std::env::consts::ARCH},"harness":{"providers":providers,"capabilities":["structured-output","cancel","poll"]},"sandbox":{"provider":"docker","capabilities":["managed-container","resource-cleanup"]}}),
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
            "DevDesk 0.2\nUsage: devdesk-host [--data PATH] [--host IP] [--port 4317] [--public-url http(s)://HOST:PORT]\n       devdesk-host --update-credential-helper (macOS bundle, explicit upgrade)\nDefault host: 127.0.0.1. Remote listening requires DEVDESK_ACCESS_TOKEN (32+ characters).\nWithout --data, choose a dataset in the Web setup page. Web UI opens GitHub projects with native Codex/DSH execution."
        );
        return Ok(());
    }
    let option = |name: &str| {
        args.iter()
            .position(|a| a == name)
            .and_then(|i| args.get(i + 1))
            .cloned()
    };
    if let Some(path) = option("--factory-export") {
        let mut state = devdesk_domain::EngineeringState::default();
        devdesk_core::seed_configuration(&mut state);
        std::fs::write(path, serde_json::to_vec(&state)?)?;
        return Ok(());
    }
    if args.iter().any(|a| a == "--update-credential-helper") {
        devdesk_adapters::credentials::CredentialVault::update_bundled_helper()?;
        println!(
            "credential_helper_updated; use the explicit settings authorization action if required"
        );
        return Ok(());
    }
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
    let listen = option("--host")
        .unwrap_or_else(|| "127.0.0.1".into())
        .parse::<std::net::IpAddr>()?;
    let public_url = option("--public-url")
        .map(|value| connection_origin(&value))
        .transpose()?;
    let token = match std::env::var("DEVDESK_ACCESS_TOKEN") {
        Ok(value) => Some(value),
        Err(std::env::VarError::NotPresent) => None,
        Err(std::env::VarError::NotUnicode(_)) => {
            return Err(err("invalid_params", "DEVDESK_ACCESS_TOKEN 必须使用 ASCII 字符").into());
        }
    };
    let access_token = connection_key(listen, public_url.is_some(), token)?;
    let workspace = option("--data")
        .map(|p| Workspace::open(&PathBuf::from(p)))
        .transpose()?;
    let listener = tokio::net::TcpListener::bind((listen, port)).await?;
    let origin = match public_url {
        Some(origin) => origin,
        None => connection_origin(&format!("http://{}", listener.local_addr()?))?,
    };
    let authority = origin
        .split_once("://")
        .expect("validated origin")
        .1
        .to_owned();
    let session_port = url::Url::parse(&origin)?
        .port_or_known_default()
        .expect("HTTP origin has a port");
    let app = Arc::new(App {
        workspace: RwLock::new(workspace),
        cookie: format!("devdesk_session_{session_port}={}", uuid::Uuid::new_v4()),
        authority: authority.clone(),
        origin: origin.clone(),
        access_token,
        stopping: AtomicBool::new(false),
        shutdown: tokio::sync::Notify::new(),
    });
    let router = Router::new()
        .route("/connection", get(connection))
        .route("/session", post(session).layer(DefaultBodyLimit::max(4096)))
        .route(
            "/rpc",
            post(rpc).layer(DefaultBodyLimit::max(4 * 1024 * 1024)),
        )
        .route(
            "/oauth/integrations/callback",
            get(integration_oauth_callback),
        )
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
                    w.tick(|| a.stopping.load(Ordering::Relaxed))
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
        "DevDesk · engineering kernel\n{origin}\nRuntime: {} / {}",
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

#[cfg(test)]
mod connection_tests {
    use super::*;

    #[test]
    fn remote_listeners_require_a_key_and_wildcards_require_an_address() {
        assert_eq!(
            connection_key("127.0.0.1".parse().unwrap(), false, None).unwrap(),
            None
        );
        assert!(connection_key("192.0.2.10".parse().unwrap(), false, None).is_err());
        assert!(connection_key("127.0.0.1".parse().unwrap(), true, None).is_err());
        assert!(connection_key("0.0.0.0".parse().unwrap(), false, Some("x".repeat(32))).is_err());
        assert!(connection_key("0.0.0.0".parse().unwrap(), true, Some("x".repeat(32))).is_ok());
        assert!(connection_key("::1".parse().unwrap(), false, Some("short".into())).is_err());
    }

    #[test]
    fn public_addresses_are_canonical_origins_without_embedded_credentials() {
        assert_eq!(
            connection_origin("https://KERNEL.example:443/").unwrap(),
            "https://kernel.example"
        );
        assert_eq!(
            connection_origin("http://[::1]:4317/").unwrap(),
            "http://[::1]:4317"
        );
        for invalid in [
            "file:///tmp",
            "http://user:secret@localhost:4317",
            "http://localhost:4317/path",
            "http://localhost/?token=secret",
            "http://localhost/#secret",
            "http://localhost:0",
        ] {
            assert!(connection_origin(invalid).is_err(), "{invalid}");
        }
    }
}

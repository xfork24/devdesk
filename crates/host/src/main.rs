//! HTTP assembly root: routing, auth and port wiring. No unsafe.
#![forbid(unsafe_code)]
#[cfg(test)]
#[path = "../../../tests/support/defaults.rs"]
mod default_fixture;
mod domains;
mod event_hub;
mod lifecycle;
#[cfg(test)]
#[path = "../../../tests/support/runtime.rs"]
mod test_runtime;
mod workspace;
use axum::{
    Json, Router,
    extract::{DefaultBodyLimit, Path, Query, Request, State},
    http::{HeaderMap, HeaderValue, StatusCode, header},
    middleware::{self, Next},
    response::{
        IntoResponse, Response,
        sse::{Event, KeepAlive, Sse},
    },
    routing::{get, post},
};
use devdesk_core::EngineeringStore;
use devdesk_domain::{Error, HARNESS_PROVIDERS, err};
use futures_util::{StreamExt, stream};
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
struct App {
    lifecycle: lifecycle::Lifecycle,
    workspace: RwLock<Option<Arc<Workspace>>>,
    cookie: String,
    authority: String,
    origin: String,
    access_token: Option<String>,
    web_root: Option<PathBuf>,
    domain_epoch: tokio::sync::watch::Sender<u64>,
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
    if let Some(origin) = request.headers().get(header::ORIGIN)
        && !discovery
        && origin.to_str().unwrap_or("") != app.origin
    {
        return (StatusCode::FORBIDDEN, "Origin rejected").into_response();
    }
    let protected = path == "/rpc"
        || path == "/events"
        || path == "/domain-events"
        || path.starts_with("/domain-assets/")
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
async fn static_file(State(app): State<Arc<App>>, request: Request) -> Response {
    let path = request.uri().path().trim_start_matches('/');
    let key = if path.is_empty() { "index.html" } else { path };
    if !devdesk_adapters::domain_packages::safe_path(key) {
        return StatusCode::NOT_FOUND.into_response();
    }
    let key = if key.contains('.') { key } else { "index.html" };
    let file = match &app.web_root {
        Some(root) => tokio::fs::read(root.join(key)).await.ok(),
        None => None,
    };
    match file {
        Some(file) => (
            [(
                header::CONTENT_TYPE,
                mime_guess::from_path(key)
                    .first_or_octet_stream()
                    .to_string(),
            )],
            file,
        )
            .into_response(),
        None => (StatusCode::NOT_FOUND, "Not found").into_response(),
    }
}
thread_local! {
    static READ_CANCELLATION: std::cell::RefCell<Option<Arc<AtomicBool>>> = const { std::cell::RefCell::new(None) };
}
struct ReadCancellation;
impl ReadCancellation {
    fn enter(flag: Option<Arc<AtomicBool>>) -> Self {
        READ_CANCELLATION.with(|slot| *slot.borrow_mut() = flag);
        Self
    }
}
impl Drop for ReadCancellation {
    fn drop(&mut self) {
        READ_CANCELLATION.with(|slot| slot.borrow_mut().take());
    }
}
fn check_read_cancelled() -> devdesk_domain::Result<()> {
    if READ_CANCELLATION.with(|slot| {
        slot.borrow()
            .as_ref()
            .is_some_and(|flag| flag.load(Ordering::Relaxed))
    }) {
        devdesk_adapters::performance::record("rpc.cancelled", 0.0);
        return Err(devdesk_domain::err("cancelled", "读取已取消"));
    }
    Ok(())
}
#[test]
fn cancelled_read_scope_is_checked_before_compilation_and_cleared_afterwards() {
    let flag = Arc::new(AtomicBool::new(false));
    {
        let _scope = ReadCancellation::enter(Some(flag.clone()));
        assert!(check_read_cancelled().is_ok());
        flag.store(true, Ordering::Relaxed);
        assert_eq!(check_read_cancelled().unwrap_err().code, "cancelled");
    }
    assert!(check_read_cancelled().is_ok());
}

// HTTP cancellation can skip queued read work. Already-running synchronous compilers
// finish normally; mutating commands always retain their existing completion semantics.
struct ReadLifetime(Arc<AtomicBool>);
impl Drop for ReadLifetime {
    fn drop(&mut self) {
        self.0.store(true, Ordering::Relaxed);
    }
}
async fn rpc(
    State(app): State<Arc<App>>,
    headers: HeaderMap,
    request: std::result::Result<Json<Value>, axum::extract::rejection::JsonRejection>,
) -> Response {
    let request =
        match request {
            Ok(Json(value)) => value,
            Err(_) => return Json(
                json!({"jsonrpc":"2.0","id":null,"error":{"code":-32700,"message":"Parse error"}}),
            )
            .into_response(),
        };
    let id = request.get("id").cloned().unwrap_or(Value::Null);
    if request["jsonrpc"] != "2.0" || !request["method"].is_string() || request.get("id").is_none()
    {
        return Json(
            json!({"jsonrpc":"2.0","id":id,"error":{"code":-32600,"message":"Invalid request"}}),
        )
        .into_response();
    }
    static SEQUENCE: std::sync::atomic::AtomicU64 = std::sync::atomic::AtomicU64::new(0);
    let measure = performance_logging();
    let request_id = SEQUENCE.fetch_add(1, Ordering::Relaxed);
    let trace_id = headers
        .get("x-devdesk-trace-id")
        .and_then(|value| value.to_str().ok())
        .and_then(|value| uuid::Uuid::parse_str(value).ok())
        .map(|value| value.simple().to_string());
    let submitted = std::time::Instant::now();
    let method = request["method"].as_str().unwrap().to_string();
    let params = request.get("params").cloned().unwrap_or(json!({}));
    let cancelled = Arc::new(AtomicBool::new(false));
    let _lifetime = ReadLifetime(cancelled.clone());
    let read = method.starts_with("query.")
        || matches!(method.as_str(), "models.list" | "codex.auth.models");
    let fallback_id = id.clone();
    let result = tokio::task::spawn_blocking(move || {
        let trace = measure.then(devdesk_adapters::performance::Trace::start);
        let _read_scope=ReadCancellation::enter(read.then(||cancelled.clone()));
        let queue_ms = submitted.elapsed().as_secs_f64()*1000.0;
        devdesk_adapters::performance::record("worker.queue_wait", queue_ms);
        let dispatch_started = std::time::Instant::now();
        let result = {
            let _span = devdesk_adapters::performance::Span::new("rpc.dispatch");
            if read && cancelled.load(Ordering::Relaxed) {
                Err(devdesk_domain::err("cancelled", "读取已取消"))
            } else if domains::method(&method) {
                domains::dispatch(&app, &method, params).map(|value| workspace::RpcOutcome { value, commit: None })
            } else {
                app.workspace().and_then(|w| w.dispatch_public_rpc(&method, params))
            }
        };
        let dispatch_ms = dispatch_started.elapsed().as_secs_f64()*1000.0;
        let success = result.is_ok();
        let error_code = result.as_ref().err().map(|error| error.code.clone());
        let body = {
            let _span = devdesk_adapters::performance::Span::new("rpc.serialize");
            let response = match result {
                Ok(outcome) => {
                    let mut response = json!({"jsonrpc":"2.0","id":id,"result":outcome.value});
                    if let Some(commit) = outcome.commit {response["meta"]=json!({"commit":commit});}
                    response
                },
                Err(e) => json!({"jsonrpc":"2.0","id":id,"error":failure(e)}),
            };
            if read && cancelled.load(Ordering::Relaxed) { Vec::new() } else { serde_json::to_vec(&response).expect("JSON RPC value") }
        };
        let milliseconds = submitted.elapsed().as_secs_f64()*1000.0;
        if let Some(trace) = trace {
            eprintln!("{}",json!({"type":"performance","request_id":request_id,"trace_id":trace_id,"method":method.chars().take(120).collect::<String>(),"success":success,"error_code":error_code,"response_bytes":body.len(),"request_ms":milliseconds,"measurement":trace.finish()}));
        } else if should_log_rpc_diagnostic(error_code.as_deref(), milliseconds) {
            eprintln!("{}", json!({"type":"rpc_diagnostic","request_id":request_id,"trace_id":trace_id,"method":method.chars().take(120).collect::<String>(),"error_code":error_code,"request_ms":milliseconds,"queue_ms":queue_ms,"dispatch_ms":dispatch_ms,"response_bytes":body.len(),"client_disconnected":cancelled.load(Ordering::Relaxed)}));
        }
        (body, milliseconds)
    }).await;
    match result {
        Ok((body, milliseconds)) => {
            let mut response = ([(header::CONTENT_TYPE,"application/json")],body).into_response();
            response.headers_mut().insert("server-timing",HeaderValue::from_str(&format!("rpc;dur={milliseconds:.3}")).expect("numeric timing"));
            response.headers_mut().insert("x-devdesk-request-id",HeaderValue::from_str(&request_id.to_string()).expect("numeric request id"));
            response
        }
        Err(e) => Json(json!({"jsonrpc":"2.0","id":fallback_id,"error":failure(err("internal",e.to_string()))})).into_response(),
    }
}
fn should_log_rpc_diagnostic(error_code: Option<&str>, milliseconds: f64) -> bool {
    match error_code {
        Some("storage" | "storage_busy") => true,
        Some(_) => milliseconds >= 1500.0,
        None => false,
    }
}
fn lifecycle_error(app: &App, error: Error) -> Response {
    let status = if [
        "revision_conflict",
        "operation_in_progress",
        "invalid_state",
    ]
    .contains(&error.code.as_str())
    {
        StatusCode::CONFLICT
    } else {
        StatusCode::BAD_REQUEST
    };
    (
        status,
        Json(json!({"error":error.message,"code":error.code,"lifecycle":app.lifecycle.snapshot()})),
    )
        .into_response()
}
async fn setup(State(app): State<Arc<App>>, Json(p): Json<Value>) -> Response {
    let Some(path) = p["path"]
        .as_str()
        .filter(|path| !path.trim().is_empty() && path.len() <= 4096)
    else {
        return lifecycle_error(&app, err("invalid_params", "请输入有效的数据目录"));
    };
    let ticket = match app.lifecycle.begin(true, &p) {
        Ok(lifecycle::Admission::Replay(result)) => return Json(result).into_response(),
        Ok(lifecycle::Admission::New(ticket)) => ticket,
        Err(error) => return lifecycle_error(&app, error),
    };
    let path = PathBuf::from(path);
    // This task owns completion even if the requesting connection disappears.
    let operation = tokio::spawn(open_dataset(app.clone(), ticket, path));
    match operation.await {
        Ok(Ok(result)) => Json(result).into_response(),
        Ok(Err(error)) => lifecycle_error(&app, error),
        Err(error) => lifecycle_error(&app, err("internal", error.to_string())),
    }
}
async fn open_dataset(
    app: Arc<App>,
    ticket: lifecycle::Ticket,
    path: PathBuf,
) -> devdesk_domain::Result<Value> {
    let opened = tokio::task::spawn_blocking(move || Workspace::open(&path))
        .await
        .unwrap_or_else(|error| Err(err("internal", error.to_string())));
    let result = match opened {
        Ok(workspace) => {
            let mut current = app.workspace.write().unwrap();
            match app.lifecycle.finish_setup(&ticket, true) {
                Ok(()) => {
                    let root = workspace.global.root();
                    *current = Some(workspace);
                    app.domain_epoch.send_modify(|_| {});
                    json!({"root":root})
                }
                Err(error) => json!({"error":error.message,"code":error.code}),
            }
        }
        Err(error) => {
            let _ = app.lifecycle.finish_setup(&ticket, false);
            json!({"error":error.message,"code":error.code})
        }
    };
    app.lifecycle.record(ticket, result)
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
    let (workspace, lifecycle) = {
        let current = app.workspace.read().unwrap();
        (current.clone(), app.lifecycle.snapshot())
    };
    let providers: Vec<&str> = HARNESS_PROVIDERS.iter().map(|p| p.as_str()).collect();
    Json(
        json!({"version":env!("CARGO_PKG_VERSION"),"protocol_version":1,"setup_required":workspace.is_none(),"lifecycle":lifecycle,"domain_protocol":1,"project_selection":"frontend","storage_layout":"global-and-project-databases","execution_mode":"live","live":"requires_registered_runtime_and_credentials","conversations":{"providers":providers,"managed_linux_workers":true,"engineering_run_contract":true},"platform":{"linux":"runtime_verified_by_reports","macos":"runtime_verified_by_reports"},"runtime":{"os":std::env::consts::OS,"arch":std::env::consts::ARCH},"harness":{"providers":providers,"capabilities":["structured-output","cancel","poll"]},"sandbox":{"provider":"docker","capabilities":["managed-container","resource-cleanup"]}}),
    )
}
async fn artifact(
    State(app): State<Arc<App>>,
    Path(id): Path<String>,
    Query(p): Query<HashMap<String, String>>,
) -> Response {
    let result = (|| {
        let workspace = app.workspace()?;
        let project = p.get("project_id").map(String::as_str);
        let epoch = p.get("domain_epoch").and_then(|value| value.parse().ok());
        workspace.with_public_project(project, epoch, |_| {
            let e = workspace.project_engine(project)?;
            let v = e.query("query.artifact", &json!({"id":id}), 4)?;
            let a: devdesk_domain::Artifact =
                serde_json::from_value(v).map_err(|e| err("internal", e.to_string()))?;
            e.artifacts.read(&a)
        })
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
    let workspace = match app.workspace() {
        Ok(workspace) => workspace,
        Err(e) => return (StatusCode::CONFLICT, Json(failure(e))).into_response(),
    };
    let project = p.get("project_id").map(String::as_str);
    let epoch = p.get("domain_epoch").and_then(|value| value.parse().ok());
    let pinned = workspace.with_public_project(project, epoch, |epoch| {
        let (scope, engine) = workspace.event_engine(project)?;
        Ok((scope, engine, domains::until_epoch_changes(&app, epoch)))
    });
    let (scope, engine, changed) = match pinned {
        Ok(pinned) => pinned,
        Err(e) => return (StatusCode::CONFLICT, Json(failure(e))).into_response(),
    };
    let resume = headers
        .get("last-event-id")
        .and_then(|v| v.to_str().ok())
        .and_then(|v| v.split_once(':'))
        .filter(|(id, _)| *id == scope)
        .and_then(|(_, n)| n.parse::<u64>().ok())
        .or_else(|| p.get("cursor").and_then(|s| s.parse().ok()));
    let configuration_events = p.get("configuration").is_some_and(|value| value == "true");
    if p.get("tail").is_some_and(|value| value == "true") {
        let tail = workspace.event_hub(&scope, engine).and_then(|hub| {
            let clock = configuration_events
                .then(|| workspace.event_configuration_clock(epoch))
                .transpose()?;
            Ok((hub, clock))
        });
        return match tail {
            Ok((hub, clock)) => event_hub::events(app, hub, resume, clock, changed),
            Err(e) => (StatusCode::CONFLICT, Json(failure(e))).into_response(),
        };
    }
    let cursor = resume.unwrap_or(0);
    let notify = Arc::new(tokio::sync::Notify::new());
    let listener: Arc<dyn Fn() + Send + Sync> = {
        let notify = notify.clone();
        Arc::new(move || notify.notify_one())
    };
    engine.store.subscribe_changes(Arc::downgrade(&listener));
    if configuration_events && let Ok(workspace) = app.workspace() {
        workspace
            .global
            .subscribe_changes(Arc::downgrade(&listener));
        workspace.subscribe_configuration_changes(Arc::downgrade(&listener));
    }
    let stream = stream::unfold(
        (
            app,
            engine,
            scope,
            cursor,
            VecDeque::<Value>::new(),
            notify,
            listener,
            None::<Value>,
        ),
        move |(app, engine, scope, mut cursor, mut queue, notify, listener, mut last_version)| async move {
            loop {
                if app.stopping.load(Ordering::Relaxed) {
                    return None;
                }
                if let Some(mut value) = queue.pop_front() {
                    value["project_id"] = json!(scope);
                    let mut event = Event::default();
                    if let Some(id) = value["id"].as_u64() {
                        cursor = id;
                        event = event.id(format!("{scope}:{cursor}"));
                    }
                    return Some((
                        Ok::<_, std::convert::Infallible>(event.data(value.to_string())),
                        (
                            app,
                            engine,
                            scope,
                            cursor,
                            queue,
                            notify,
                            listener,
                            last_version,
                        ),
                    ));
                }
                let read_engine = engine.clone();
                let read_app = app.clone();
                let read = tokio::task::spawn_blocking(move || -> devdesk_domain::Result<_> {
                    let values = read_engine.store.events_after(cursor, 100)?;
                    let version = if configuration_events {
                        read_app
                            .workspace()?
                            .configuration_version(&read_engine, epoch)?
                    } else {
                        Value::Null
                    };
                    Ok((values, version))
                })
                .await;
                if let Ok(Ok((values, version))) = read {
                    if configuration_events && last_version.as_ref() != Some(&version) {
                        queue.push_back(
                            json!({"kind":"configuration.changed","configuration_version":version}),
                        );
                        last_version = Some(version.clone());
                    }
                    for value in values {
                        let mut value = json!(value);
                        if configuration_events {
                            value["configuration_version"] = version.clone();
                        }
                        queue.push_back(value);
                    }
                    if !queue.is_empty() {
                        continue;
                    }
                }
                // Commit notifications provide low latency; the bounded retry repairs missed notifications.
                tokio::select! {
                    _=notify.notified()=>{},
                    _=tokio::time::sleep(std::time::Duration::from_secs(1))=>{},
                    _=app.shutdown.notified()=>{},
                }
            }
        },
    );
    Sse::new(stream.take_until(changed))
        .keep_alive(KeepAlive::default())
        .into_response()
}
async fn shutdown(State(app): State<Arc<App>>, Json(p): Json<Value>) -> Response {
    let ticket = match app.lifecycle.begin(false, &p) {
        Ok(lifecycle::Admission::Replay(result)) => return Json(result).into_response(),
        Ok(lifecycle::Admission::New(ticket)) => ticket,
        Err(error) => return lifecycle_error(&app, error),
    };
    let result = match app.lifecycle.record(ticket, json!({"ok":true})) {
        Ok(result) => result,
        Err(error) => return lifecycle_error(&app, error),
    };
    app.stopping.store(true, Ordering::Relaxed);
    app.shutdown.notify_waiters();
    Json(result).into_response()
}

fn termination_signal() -> std::io::Result<impl std::future::Future<Output = &'static str>> {
    #[cfg(unix)]
    {
        use tokio::signal::unix::{SignalKind, signal};
        let mut term = signal(SignalKind::terminate())?;
        let mut hup = signal(SignalKind::hangup())?;
        Ok(async move {
            tokio::select! {
                _ = tokio::signal::ctrl_c() => "SIGINT",
                _ = term.recv() => "SIGTERM",
                _ = hup.recv() => "SIGHUP",
            }
        })
    }
    #[cfg(not(unix))]
    Ok(async {
        let _ = tokio::signal::ctrl_c().await;
        "SIGINT"
    })
}
async fn scheduler_loop(scheduler: Arc<App>) {
    loop {
        let workspace = scheduler.workspace().ok();
        let delay = workspace
            .as_ref()
            .map(|w| w.scheduler_delay())
            .unwrap_or(std::time::Duration::from_millis(75));
        let wakeup = workspace.map(|w| w.scheduler_notify());
        tokio::select! {
            _ = tokio::time::sleep(delay) => {},
            _ = scheduler.shutdown.notified() => {},
            _ = async {
                if let Some(wakeup) = wakeup { wakeup.notified().await; }
                else { std::future::pending::<()>().await; }
            } => {},
        }
        // Coalesce bursts without accumulating missed periodic ticks. This
        // bounds scheduling work while keeping native handoffs responsive.
        tokio::time::sleep(std::time::Duration::from_millis(2)).await;
        if scheduler.stopping.load(Ordering::Relaxed) {
            break;
        }
        let a = scheduler.clone();
        let result = tokio::task::spawn_blocking(move || {
                if a.stopping.load(Ordering::Relaxed) {
                    return Ok(());
                }
                let trace = performance_logging().then(devdesk_adapters::performance::Trace::start);
                let result = if let Ok(w) = a.workspace() {
                    w.tick(|| a.stopping.load(Ordering::Relaxed))
                } else { Ok(()) };
                if let Some(trace) = trace {
                    eprintln!("{}", json!({"type":"performance","context":"workflow_tick","success":result.is_ok(),"measurement":trace.finish()}));
                }
                result
            })
            .await;
        let failure = match result {
            Ok(Err(error)) => Some(error),
            Err(error) => {
                let description = error.to_string();
                let panic_message = if error.is_panic() {
                    let payload = error.into_panic();
                    payload
                        .downcast_ref::<String>()
                        .cloned()
                        .or_else(|| payload.downcast_ref::<&str>().map(|s| (*s).into()))
                } else {
                    None
                };
                Some(err("scheduler_panic", description).with_context("scheduler_loop", json!({"panic_message":panic_message,"boundary_backtrace":std::backtrace::Backtrace::force_capture().to_string()})))
            }
            _ => None,
        };
        if let Some(error) = failure
            && let Some(event) = scheduler_error_event(&error)
        {
            eprintln!("{event}");
        }
    }
}
fn scheduler_error_event(error: &Error) -> Option<Value> {
    if error.retryable_conflict() {
        return None;
    }
    Some(
        json!({"type":"scheduler_error","level":"error","timestamp_unix_ms":std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).unwrap_or_default().as_millis(),"error":error,"scheduler_backtrace":std::backtrace::Backtrace::force_capture().to_string()}),
    )
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
    if args.iter().any(|a| a == "--version" || a == "-V") {
        println!("devdesk-host {}", env!("CARGO_PKG_VERSION"));
        return Ok(());
    }
    if args.iter().any(|a| a == "--help") {
        println!(
            "DevDesk {}\nUsage: devdesk-host [--data PATH] [--host IP] [--port 4317] [--public-url http(s)://HOST:PORT] [--web-dir DIR]\n       devdesk-host --data PATH --authorize-credential REFERENCE\n       devdesk-host storage upgrade-global --data PATH\n       devdesk-host sandbox-policy --print|--check|--install\nDefault host: 127.0.0.1. Remote listening requires DEVDESK_ACCESS_TOKEN (32+ characters).\nWithout --data, choose a dataset in the Web setup page. Without --web-dir the Host serves the client protocol only.",
            env!("CARGO_PKG_VERSION")
        );
        return Ok(());
    }
    if let [command, options @ ..] = args.as_slice()
        && command == "sandbox-policy"
    {
        use devdesk_adapters::permissions::LINUX_APPARMOR_POLICY;
        match options {
            [action] if action == "--print" => {
                use std::io::Write;
                std::io::stdout()
                    .lock()
                    .write_all(LINUX_APPARMOR_POLICY.as_bytes())?;
            }
            [action] if matches!(action.as_str(), "--check" | "--install") => {
                let status = std::process::Command::new("/bin/bash")
                    .arg("-c")
                    .arg(include_str!(
                        "../../../scripts/platform/linux/install-linux-sandbox-policy.sh"
                    ))
                    .arg("devdesk-host sandbox-policy")
                    .arg(action)
                    .arg(LINUX_APPARMOR_POLICY)
                    .status()?;
                if !status.success() {
                    return Err(format!("AppArmor policy command failed ({status})").into());
                }
            }
            _ => return Err("Usage: devdesk-host sandbox-policy --print|--check|--install".into()),
        }
        return Ok(());
    }
    let option = |name: &str| {
        args.iter()
            .position(|a| a == name)
            .and_then(|i| args.get(i + 1))
            .cloned()
    };
    if args.first().is_some_and(|arg| arg == "storage") {
        let data = PathBuf::from(option("--data").ok_or("storage requires --data")?);
        let action = args.get(1).map(String::as_str);
        let report = if action == Some("upgrade-global") {
            devdesk_adapters::global_migration::upgrade(&data)?
        } else {
            let project = option("--project").ok_or("storage requires --project")?;
            match action {
                Some("migrate") => devdesk_adapters::project_migration::migrate(&data, &project)?,
                Some("compact") => devdesk_adapters::project_migration::compact(&data, &project)?,
                Some("rebuild-workflow-display") => {
                    devdesk_adapters::project_migration::rebuild_workflow_display(&data, &project)?
                }
                _ => {
                    return Err("Usage: devdesk-host storage upgrade-global --data PATH | storage (migrate|compact|rebuild-workflow-display) --data PATH --project ID".into());
                }
            }
        };
        println!("{}", serde_json::to_string_pretty(&report)?);
        return Ok(());
    }
    if args.iter().any(|a| a == "--authorize-credential") {
        let reference = option("--authorize-credential")
            .ok_or("credential authorization requires a reference")?;
        let dataset =
            option("--data").ok_or("credential authorization requires an existing dataset")?;
        devdesk_adapters::credentials::CredentialVault::authorize_for_dataset(
            &PathBuf::from(dataset),
            &reference,
        )?;
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
    let startup = performance_logging().then(devdesk_adapters::performance::Trace::start);
    let workspace = option("--data")
        .map(|p| Workspace::open(&PathBuf::from(p)))
        .transpose();
    if let Some(trace) = startup {
        eprintln!(
            "{}",
            json!({"type":"performance","context":"startup","success":workspace.is_ok(),"measurement":trace.finish()})
        );
    }
    let workspace = workspace?;
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
    let web_root = option("--web-dir").map(std::fs::canonicalize).transpose()?;
    let app = Arc::new(App {
        lifecycle: lifecycle::Lifecycle::new(workspace.is_some()),
        workspace: RwLock::new(workspace),
        cookie: format!("devdesk_session_{session_port}={}", uuid::Uuid::new_v4()),
        authority: authority.clone(),
        origin: origin.clone(),
        access_token,
        web_root,
        domain_epoch: tokio::sync::watch::channel(0).0,
        stopping: AtomicBool::new(false),
        shutdown: tokio::sync::Notify::new(),
    });
    let router = router(app.clone());
    let task = tokio::spawn(scheduler_loop(app.clone()));
    let signal = termination_signal()?;
    println!(
        "DevDesk · engineering kernel\n{origin}\nRuntime: {} / {}",
        std::env::consts::OS,
        std::env::consts::ARCH
    );
    let exit = app.clone();
    axum::serve(listener, router)
        .with_graceful_shutdown(async move {
            let reason = tokio::select! {
                reason=signal=>reason,
                _=async {
                    // SSE subscribers share the notification. Check the durable process
                    // predicate as well, including when notification preceded this wait.
                    while !exit.stopping.load(Ordering::Relaxed) {
                        tokio::select! {
                            _=exit.shutdown.notified()=>{},
                            _=tokio::time::sleep(std::time::Duration::from_secs(1))=>{},
                        }
                    }
                }=>"http",
            };
            eprintln!("{}",json!({"type":"host_lifecycle","event":"shutdown_requested","reason":reason,"pid":std::process::id()}));
            exit.lifecycle.stop_from_signal();
            exit.stopping.store(true, Ordering::Relaxed);
            exit.shutdown.notify_waiters();
        })
        .await?;
    app.stopping.store(true, Ordering::Relaxed);
    let _ = task.await;
    if let Ok(workspace) = app.workspace() {
        tokio::task::spawn_blocking(move || workspace.stop_owned_runtime()).await??;
    }
    eprintln!(
        "{}",
        json!({"type":"host_lifecycle","event":"runtime_stopped","pid":std::process::id()})
    );
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

fn router(app: Arc<App>) -> Router {
    Router::new()
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
        .route("/domain-events", get(domains::events))
        .route(
            "/domain-assets/{epoch}/{digest}/{*path}",
            get(domains::asset),
        )
        .route("/artifacts/{id}", get(artifact))
        .fallback(static_file)
        .layer(middleware::from_fn_with_state(app.clone(), guard))
        .with_state(app.clone())
}

#[cfg(test)]
mod active_frontend_performance;
#[cfg(test)]
mod loading_performance;

fn performance_logging() -> bool {
    static ENABLED: std::sync::OnceLock<bool> = std::sync::OnceLock::new();
    *ENABLED.get_or_init(|| std::env::var("DEVDESK_PERF_LOG").is_ok_and(|value| value == "1"))
}

#[test]
fn rpc_diagnostics_keep_storage_errors_and_slow_failures_only() {
    assert!(!should_log_rpc_diagnostic(None, 4039.0));
    assert!(!should_log_rpc_diagnostic(Some("invalid_params"), 20.0));
    assert!(should_log_rpc_diagnostic(Some("provider_io"), 4039.0));
    assert!(should_log_rpc_diagnostic(Some("storage"), 5.0));
    assert!(should_log_rpc_diagnostic(Some("storage_busy"), 5.0));
}

#[cfg(test)]
mod scheduler_diagnostic_tests {
    use super::*;
    #[test]
    fn normal_conflicts_do_not_log_as_errors_and_real_faults_keep_full_context() {
        assert!(scheduler_error_event(&err("revision_conflict", "stale step")).is_none());
        assert!(scheduler_error_event(&err("storage_busy", "short transaction")).is_none());
        assert!(
            scheduler_error_event(&err("revision_conflict", "unknown effect").technical_unknown())
                .is_some()
        );
        let error = err("storage", "SQL read failed").with_context("scheduler worker", json!({"project_id":"project","stage":"workflows","workflow_instance_id":"workflow","execution_id":"step","boundary_backtrace":"stack trace"}));
        let event = scheduler_error_event(&error).unwrap();
        assert_eq!(event["type"], "scheduler_error");
        assert_eq!(event["error"]["message"], "SQL read failed");
        assert!(event["error"]["location"].is_object());
        assert_eq!(
            event["error"]["context"][0]["details"]["execution_id"],
            "step"
        );
        assert_eq!(
            event["error"]["context"][0]["details"]["boundary_backtrace"],
            "stack trace"
        );
        assert!(serde_json::from_str::<Value>(&event.to_string()).is_ok());
    }
}

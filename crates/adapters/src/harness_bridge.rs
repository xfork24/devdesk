//! One authenticated MCP transport per provider process and dispatched Turn.
use super::*;
use axum::{
    Router,
    body::to_bytes,
    extract::{Request, State},
    http::{HeaderMap, StatusCode, header},
    response::{IntoResponse, Response},
    routing::post,
};
use std::{
    collections::{BTreeMap, VecDeque},
    net::{IpAddr, Ipv4Addr, TcpListener},
    time::Duration,
};
use tokio::sync::{Semaphore, oneshot, watch};

const MAX_PENDING: usize = 8;
static REQUESTS: Semaphore = Semaphore::const_new(64);
const MAX_CALLS: usize = 256;
const MAX_BODY: usize = 1024 * 1024;
const PAGE_SIZE: usize = 32;
const LEGACY_PROTOCOLS: &[&str] = &["2024-11-05", "2025-03-26", "2025-06-18", "2025-11-25"];
const MODERN_PROTOCOL: &str = "2026-07-28";
const PATH: &str = "/mcp";

pub(super) struct Bridge {
    state: Arc<Server>,
    shutdown: Option<oneshot::Sender<()>>,
}

struct Server {
    token_digest: [u8; 32],
    transport_id: String,
    authority: String,
    lock: Mutex<Mailbox>,
    tools: BTreeMap<String, ExposedTool>,
    wakeup: Arc<dyn Fn() + Send + Sync>,
    timeout: Duration,
}

struct ExposedTool {
    name: String,
    definition: Value,
}

#[derive(Default)]
struct Mailbox {
    turn: Option<Id>,
    revoked: bool,
    initialized: bool,
    uncertain: bool,
    calls: BTreeMap<String, Entry>,
    queue: VecDeque<String>,
}

struct Entry {
    digest: String,
    call: Call,
    handed_off: bool,
    cancelled: bool,
    result: watch::Sender<Option<Value>>,
}

#[derive(Clone, Debug)]
pub(super) struct Call {
    pub turn: Id,
    pub id: Id,
    pub tool: String,
    pub arguments: Value,
}

impl Bridge {
    pub(super) fn start(
        address: IpAddr,
        definitions: Vec<Value>,
        wakeup: Arc<dyn Fn() + Send + Sync>,
        timeout: Duration,
    ) -> Result<(Self, String, String)> {
        let listener = TcpListener::bind((address, 0)).map_err(storage)?;
        listener.set_nonblocking(true).map_err(storage)?;
        let port = listener.local_addr().map_err(storage)?.port();
        let mut entropy = [0u8; 32];
        getrandom::fill(&mut entropy).map_err(storage)?;
        let token =
            base64::Engine::encode(&base64::engine::general_purpose::URL_SAFE_NO_PAD, entropy);
        let authority = format!("host.docker.internal:{port}");
        let state = Arc::new(Server {
            token_digest: Sha256::digest(token.as_bytes()).into(),
            transport_id: id(),
            authority: authority.clone(),
            lock: Mutex::new(Mailbox::default()),
            tools: expose_tools(definitions)?,
            wakeup,
            timeout,
        });
        let router = Router::new()
            .route(PATH, post(request).delete(delete_transport))
            .with_state(state.clone());
        let (shutdown, stopped) = oneshot::channel();
        let runtime = tokio::runtime::Builder::new_current_thread()
            .enable_all()
            .build()
            .map_err(storage)?;
        std::thread::Builder::new()
            .name("devdesk-harness-mcp".into())
            .spawn(move || {
                runtime.block_on(async move {
                    if let Ok(listener) = tokio::net::TcpListener::from_std(listener) {
                        let _ = axum::serve(listener, router)
                            .with_graceful_shutdown(async {
                                let _ = stopped.await;
                            })
                            .await;
                    }
                });
            })
            .map_err(storage)?;
        Ok((
            Self {
                state,
                shutdown: Some(shutdown),
            },
            format!("http://{authority}{PATH}"),
            token,
        ))
    }

    pub(super) fn arm(&self, turn: &str) -> Result<()> {
        let mut mailbox = self.state.lock.lock().map_err(storage)?;
        require(
            !mailbox.revoked && mailbox.turn.is_none(),
            "MCP worker 只承载一个轮次",
        )?;
        mailbox.turn = Some(turn.to_owned());
        Ok(())
    }

    pub(super) fn drain(&self, turn: &str) -> Result<Vec<Call>> {
        let mut mailbox = self.state.lock.lock().map_err(storage)?;
        require(mailbox.turn.as_deref() == Some(turn), "MCP 收件箱轮次不符")?;
        if mailbox.revoked {
            return Ok(Vec::new());
        }
        let mut calls = Vec::new();
        while let Some(key) = mailbox.queue.pop_front() {
            let entry = mailbox.calls.get_mut(&key).expect("queued entry");
            if !entry.cancelled {
                entry.handed_off = true;
                calls.push(entry.call.clone());
            }
        }
        Ok(calls)
    }

    pub(super) fn answer(&self, turn: &str, call: &str, receipt: &ToolReceipt) -> Result<()> {
        let mailbox = self.state.lock.lock().map_err(storage)?;
        require(mailbox.turn.as_deref() == Some(turn), "MCP 回执轮次不符")?;
        let entry = mailbox
            .calls
            .values()
            .find(|entry| entry.call.id == call && entry.handed_off)
            .ok_or_else(|| err("needs_reconciliation", "MCP 回执缺少原始调用"))?;
        if entry.result.borrow().is_none() {
            let mut content: Vec<_> = receipt
                .content
                .iter()
                .map(|item| match item["type"].as_str() {
                    Some("text" | "image") => item.clone(),
                    _ => json!({"type":"text","text":item.to_string()}),
                })
                .collect();
            if let Some(error) = &receipt.error {
                content.push(json!({"type":"text","text":error}));
            }
            let mut result = json!({"content":content,"isError":!receipt.success});
            if let Some(structured) = &receipt.structured_content {
                result["structuredContent"] = structured.clone();
            }
            entry.result.send_replace(Some(result));
        }
        Ok(())
    }

    pub(super) fn revoke(&self) {
        self.state.revoke();
    }
}

impl Drop for Bridge {
    fn drop(&mut self) {
        self.revoke();
        if let Some(shutdown) = self.shutdown.take() {
            let _ = shutdown.send(());
        }
    }
}

impl Server {
    fn authorized(&self, headers: &HeaderMap) -> bool {
        let host_matches =
            headers.get(header::HOST).and_then(|v| v.to_str().ok()) == Some(&self.authority);
        let origin_matches = headers.get(header::ORIGIN).is_none_or(|v| {
            v.to_str()
                .is_ok_and(|v| v.strip_prefix("http://") == Some(&self.authority))
        });
        let Some(token) = headers
            .get(header::AUTHORIZATION)
            .and_then(|v| v.to_str().ok())
            .and_then(|v| v.strip_prefix("Bearer "))
        else {
            return false;
        };
        let digest = Sha256::digest(token.as_bytes());
        let mut difference = 0;
        for (provided, expected) in digest.iter().zip(self.token_digest) {
            difference |= provided ^ expected;
        }
        host_matches && origin_matches && difference == 0
    }

    fn revoke(&self) {
        if let Ok(mut mailbox) = self.lock.lock() {
            mailbox.revoked = true;
            mailbox.queue.clear();
            for entry in mailbox.calls.values() {
                if entry.result.borrow().is_none() {
                    entry
                        .result
                        .send_replace(Some(unavailable(entry.handed_off)));
                }
            }
        }
    }
}

pub(super) fn listen_address() -> Result<IpAddr> {
    crate::docker::require_local_daemon()?;
    if cfg!(target_os = "macos") {
        return Ok(Ipv4Addr::LOCALHOST.into());
    }
    let output = crate::process::capture(
        "docker",
        &[
            "network".into(),
            "inspect".into(),
            "bridge".into(),
            "--format".into(),
            "{{range .IPAM.Config}}{{println .Gateway}}{{end}}".into(),
        ],
        Duration::from_secs(5),
    )?;
    require(
        output.code == 0 && !output.truncated,
        "无法核对本地 Docker 网桥",
    )?;
    String::from_utf8_lossy(&output.stdout)
        .lines()
        .find_map(|line| {
            line.trim()
                .parse::<Ipv4Addr>()
                .ok()
                .filter(Ipv4Addr::is_private)
        })
        .map(IpAddr::from)
        .ok_or_else(|| {
            err(
                "bridge_unavailable",
                "本地 Docker 网桥没有可绑定的私有 IPv4 网关",
            )
        })
}

fn expose_tools(definitions: Vec<Value>) -> Result<BTreeMap<String, ExposedTool>> {
    let mut tools = BTreeMap::new();
    for tool in definitions {
        let name = string(&tool, "name")?;
        let exposed = name
            .chars()
            .map(|c| {
                if c.is_ascii_alphanumeric() || matches!(c, '_' | '-') {
                    c
                } else {
                    '_'
                }
            })
            .collect::<String>();
        // Leave room for the provider's MCP server namespace.
        require(
            !exposed.is_empty() && exposed.len() <= 48,
            "MCP 工具名超出 Harness 预算",
        )?;
        let definition = json!({"name":exposed,"description":tool["description"],"inputSchema":tool["inputSchema"]});
        require(
            tools
                .insert(exposed, ExposedTool { name, definition })
                .is_none(),
            "MCP 工具名冲突",
        )?;
    }
    Ok(tools)
}

fn unavailable(handed_off: bool) -> Value {
    let message = if handed_off {
        "needs_reconciliation: tool outcome unknown; do not retry with a new request identity"
    } else {
        "tool_not_authorized: the request was not applied to an active turn"
    };
    tool_error(message)
}

fn tool_error(message: &str) -> Value {
    json!({"isError":true,"content":[{"type":"text","text":message}]})
}

fn result(id: &Value, value: Value) -> Response {
    axum::Json(json!({"jsonrpc":"2.0","id":id,"result":value})).into_response()
}

fn protocol_error(id: &Value, code: i32, message: &str) -> Response {
    axum::Json(json!({"jsonrpc":"2.0","id":id,"error":{"code":code,"message":message}}))
        .into_response()
}

async fn delete_transport(State(server): State<Arc<Server>>, headers: HeaderMap) -> Response {
    if !server.authorized(&headers)
        || headers.get("mcp-session-id").and_then(|v| v.to_str().ok()) != Some(&server.transport_id)
    {
        return StatusCode::UNAUTHORIZED.into_response();
    }
    server.revoke();
    StatusCode::NO_CONTENT.into_response()
}

async fn request(State(server): State<Arc<Server>>, request: Request) -> Response {
    if !server.authorized(request.headers()) {
        return StatusCode::UNAUTHORIZED.into_response();
    }
    let Ok(_budget) = REQUESTS.try_acquire() else {
        return StatusCode::SERVICE_UNAVAILABLE.into_response();
    };
    let (parts, body) = request.into_parts();
    let headers = parts.headers;
    let Ok(Ok(body)) = tokio::time::timeout(Duration::from_secs(5), to_bytes(body, MAX_BODY)).await
    else {
        return StatusCode::BAD_REQUEST.into_response();
    };
    let Ok(frame) = serde_json::from_slice::<Value>(&body) else {
        return protocol_error(&Value::Null, -32700, "Invalid JSON");
    };
    let id = &frame["id"];
    let method = frame["method"].as_str().unwrap_or_default();
    if frame["jsonrpc"] != "2.0"
        || method.is_empty()
        || (!id.is_null() && !id.is_i64() && !id.as_str().is_some_and(|s| s.len() <= 128))
    {
        return protocol_error(&Value::Null, -32600, "Invalid request");
    }
    let params = &frame["params"];
    let modern = params["_meta"]["io.modelcontextprotocol/protocolVersion"] == MODERN_PROTOCOL;
    let wait = {
        let Ok(mut mailbox) = server.lock.lock() else {
            return StatusCode::INTERNAL_SERVER_ERROR.into_response();
        };
        if mailbox.revoked {
            return StatusCode::GONE.into_response();
        }
        if method == "server/discover" && modern {
            return result(
                id,
                json!({"protocolVersion":MODERN_PROTOCOL,"capabilities":{"tools":{}},
                "serverInfo":{"name":"devdesk","version":"1"}}),
            );
        }
        if method == "initialize" {
            if mailbox.initialized {
                return protocol_error(id, -32600, "Transport already initialized");
            }
            mailbox.initialized = true;
            let version = params["protocolVersion"]
                .as_str()
                .filter(|version| LEGACY_PROTOCOLS.contains(version))
                .unwrap_or("2025-11-25");
            return (
                [("mcp-session-id", server.transport_id.as_str())],
                result(
                    id,
                    json!({"protocolVersion":version,"capabilities":{"tools":{}},
                    "serverInfo":{"name":"devdesk","version":"1"}}),
                ),
            )
                .into_response();
        }
        if !modern
            && (!mailbox.initialized
                || headers.get("mcp-session-id").and_then(|v| v.to_str().ok())
                    != Some(&server.transport_id))
        {
            return StatusCode::NOT_FOUND.into_response();
        }
        match method {
            "notifications/initialized" if id.is_null() => {
                return StatusCode::ACCEPTED.into_response();
            }
            "notifications/cancelled" if id.is_null() => {
                let key = params["requestId"].to_string();
                if let Some(entry) = mailbox.calls.get_mut(&key)
                    && entry.result.borrow().is_none()
                {
                    if entry.handed_off {
                        mailbox.uncertain = true;
                    } else {
                        entry.cancelled = true;
                        entry.result.send_replace(Some(unavailable(false)));
                    }
                }
                return StatusCode::ACCEPTED.into_response();
            }
            "ping" => return result(id, json!({})),
            "tools/list" => {
                let cursor = match params.get("cursor") {
                    None => 0,
                    Some(cursor) => match cursor.as_str().and_then(|v| v.parse::<usize>().ok()) {
                        Some(index) if index <= server.tools.len() => index,
                        _ => return protocol_error(id, -32602, "Invalid tool cursor"),
                    },
                };
                let tools: Vec<_> = server
                    .tools
                    .values()
                    .skip(cursor)
                    .take(PAGE_SIZE)
                    .map(|tool| &tool.definition)
                    .collect();
                let next = cursor + tools.len();
                let mut page = json!({"tools":tools});
                if next < server.tools.len() {
                    page["nextCursor"] = json!(next.to_string());
                }
                return result(id, page);
            }
            "tools/call" if !id.is_null() => {
                let Some(turn) = mailbox.turn.clone() else {
                    return result(id, unavailable(false));
                };
                let Some(tool) = params["name"]
                    .as_str()
                    .and_then(|name| server.tools.get(name))
                else {
                    return result(id, tool_error("tool_not_authorized: unknown frozen tool"));
                };
                let arguments = params
                    .get("arguments")
                    .cloned()
                    .unwrap_or_else(|| json!({}));
                if !arguments.is_object() {
                    return protocol_error(id, -32602, "Tool arguments must be an object");
                }
                let key = id.to_string();
                let digest = configuration_hash(&(&tool.name, &arguments));
                if let Some(entry) = mailbox.calls.get(&key) {
                    if entry.digest != digest {
                        return protocol_error(
                            id,
                            -32602,
                            "Request identity already bound to other arguments",
                        );
                    }
                    (entry.result.subscribe(), key)
                } else {
                    if mailbox.uncertain {
                        return result(id, unavailable(true));
                    }
                    let pending = mailbox
                        .calls
                        .values()
                        .filter(|entry| entry.result.borrow().is_none())
                        .count();
                    if pending >= MAX_PENDING || mailbox.calls.len() >= MAX_CALLS {
                        return result(id, tool_error("MCP call capacity exhausted"));
                    }
                    let call = Call {
                        id: format!("mcp-{}", &configuration_hash(&(&turn, &key))[..32]),
                        turn,
                        tool: tool.name.clone(),
                        arguments,
                    };
                    let (result, waiter) = watch::channel(None);
                    mailbox.calls.insert(
                        key.clone(),
                        Entry {
                            digest,
                            call,
                            handed_off: false,
                            cancelled: false,
                            result,
                        },
                    );
                    mailbox.queue.push_back(key.clone());
                    (server.wakeup)();
                    (waiter, key)
                }
            }
            _ => return protocol_error(id, -32601, "Unsupported MCP method"),
        }
    };
    let (mut receipt, key) = wait;
    let mut waiting = Waiting {
        server: &server,
        key: &key,
        completed: false,
    };
    let outcome = tokio::time::timeout(server.timeout, receipt.wait_for(Option::is_some)).await;
    if let Ok(Ok(receipt)) = outcome {
        waiting.completed = true;
        return result(id, receipt.clone().expect("completed receipt"));
    }
    let Ok(mut mailbox) = server.lock.lock() else {
        return StatusCode::INTERNAL_SERVER_ERROR.into_response();
    };
    waiting.completed = true;
    let Some(entry) = mailbox.calls.get_mut(&key) else {
        return result(id, unavailable(false));
    };
    let observed = entry.result.borrow().clone();
    if let Some(observed) = observed {
        return result(id, observed);
    }
    let unavailable = unavailable(entry.handed_off);
    if entry.handed_off {
        mailbox.uncertain = true;
    } else {
        entry.cancelled = true;
        entry.result.send_replace(Some(unavailable.clone()));
    }
    result(id, unavailable)
}

// Hyper may cancel the HTTP future when the client disconnects. Preserve the
// dispatched identity, and fence fresh IDs while its effect is unconfirmed.
struct Waiting<'a> {
    server: &'a Server,
    key: &'a str,
    completed: bool,
}

impl Drop for Waiting<'_> {
    fn drop(&mut self) {
        if self.completed {
            return;
        }
        if let Ok(mut mailbox) = self.server.lock.lock()
            && let Some(entry) = mailbox.calls.get_mut(self.key)
            && entry.result.borrow().is_none()
        {
            if entry.handed_off {
                mailbox.uncertain = true;
            } else {
                entry.cancelled = true;
                entry.result.send_replace(Some(unavailable(false)));
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::io::{Read, Write};
    use std::net::TcpStream;
    use std::sync::mpsc;

    struct Client {
        authority: String,
        token: String,
        transport: String,
    }

    impl Client {
        fn post(&self, frame: Value) -> (u16, BTreeMap<String, String>, Value) {
            let port = self.authority.rsplit_once(':').unwrap().1.parse().unwrap();
            let mut socket = TcpStream::connect((Ipv4Addr::LOCALHOST, port)).unwrap();
            socket
                .set_read_timeout(Some(Duration::from_secs(5)))
                .unwrap();
            let body = frame.to_string();
            let session = if self.transport.is_empty() {
                String::new()
            } else {
                format!("Mcp-Session-Id: {}\r\n", self.transport)
            };
            write!(socket, "POST /mcp HTTP/1.1\r\nHost: {}\r\nAuthorization: Bearer {}\r\n{session}Content-Type: application/json\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{body}",
                self.authority, self.token, body.len()).unwrap();
            let mut response = String::new();
            socket.read_to_string(&mut response).unwrap();
            let (headers, body) = response.split_once("\r\n\r\n").unwrap();
            let status = headers
                .lines()
                .next()
                .unwrap()
                .split_whitespace()
                .nth(1)
                .unwrap()
                .parse()
                .unwrap();
            let headers = headers
                .lines()
                .skip(1)
                .filter_map(|line| line.split_once(':'))
                .map(|(name, value)| (name.to_ascii_lowercase(), value.trim().to_owned()))
                .collect();
            (
                status,
                headers,
                serde_json::from_str(body).unwrap_or(Value::Null),
            )
        }

        fn call(&self, id: u32, value: &str) -> (u16, Value) {
            let (status, _, response) = self
                .post(json!({"jsonrpc":"2.0","id":id,"method":"tools/call",
                "params":{"name":"materials_capture","arguments":{"value":value}}}));
            (status, response)
        }
    }

    fn bridge(timeout: Duration) -> (Bridge, Client, mpsc::Receiver<()>) {
        let (notify, events) = mpsc::channel();
        let (bridge, url, token) = Bridge::start(Ipv4Addr::LOCALHOST.into(),
            vec![json!({"name":"materials.capture","description":"fixture","inputSchema":{"type":"object"}})],
            Arc::new(move || { let _ = notify.send(()); }), timeout).unwrap();
        let mut client = Client {
            authority: url
                .strip_prefix("http://")
                .unwrap()
                .strip_suffix(PATH)
                .unwrap()
                .into(),
            token,
            transport: String::new(),
        };
        let (status, headers, _) = client.post(json!({"jsonrpc":"2.0","id":0,"method":"initialize","params":{"protocolVersion":"2025-11-25"}}));
        assert_eq!(status, 200);
        client.transport = headers["mcp-session-id"].clone();
        (bridge, client, events)
    }

    #[test]
    fn early_call_is_bound_once_and_previous_token_cannot_enter_next_turn() {
        let (bridge, client, events) = bridge(Duration::from_secs(3));
        assert_eq!(
            client.call(1, "before dispatch").1["result"]["isError"],
            true
        );
        assert!(events.try_recv().is_err());
        bridge.arm("turn-one").unwrap();
        assert!(bridge.arm("turn-two").is_err());
        std::thread::scope(|scope| {
            let pending = scope.spawn(|| client.call(2, "early"));
            events.recv_timeout(Duration::from_secs(2)).unwrap();
            let calls = bridge.drain("turn-one").unwrap();
            assert_eq!(calls.len(), 1);
            assert_eq!(calls[0].turn, "turn-one");
            assert_eq!(calls[0].tool, "materials.capture");
            assert!(bridge.drain("turn-two").is_err());
            let receipt = ToolReceipt {
                invocation_id: "original".into(),
                success: true,
                content: vec![json!({"type":"text","text":"accepted"})],
                structured_content: None,
                remote_effect: "none".into(),
                error: None,
                origin: Origin::External,
            };
            assert!(bridge.answer("turn-two", &calls[0].id, &receipt).is_err());
            bridge.answer("turn-one", &calls[0].id, &receipt).unwrap();
            let (_, first) = pending.join().unwrap();
            assert_eq!(first["result"]["content"][0]["text"], "accepted");
            assert_eq!(client.call(2, "early").1, first);
            assert_eq!(client.call(2, "changed").1["error"]["code"], -32602);
        });
        assert!(bridge.drain("turn-one").unwrap().is_empty());
        bridge.revoke();
        assert_eq!(client.call(3, "late").0, 410);
        let (next, next_client, next_events) = self::bridge(Duration::from_secs(3));
        next.arm("turn-two").unwrap();
        let stale = Client {
            authority: next_client.authority.clone(),
            ..client
        };
        assert_eq!(stale.call(2, "old request").0, 401);
        assert!(next_events.try_recv().is_err());
    }

    #[test]
    fn timed_out_handoff_fences_new_request_ids_and_retains_original_receipt() {
        let (bridge, client, events) = bridge(Duration::from_millis(500));
        bridge.arm("turn").unwrap();
        std::thread::scope(|scope| {
            let pending = scope.spawn(|| client.call(2, "once"));
            events.recv_timeout(Duration::from_secs(2)).unwrap();
            let calls = bridge.drain("turn").unwrap();
            assert_eq!(calls.len(), 1);
            let (_, result) = pending.join().unwrap();
            assert!(result.to_string().contains("needs_reconciliation"));
            assert!(
                client
                    .call(3, "retry")
                    .1
                    .to_string()
                    .contains("needs_reconciliation")
            );
            assert!(bridge.drain("turn").unwrap().is_empty());
            let receipt = ToolReceipt {
                invocation_id: "original".into(),
                success: true,
                content: vec![json!({"type":"text","text":"recovered"})],
                structured_content: None,
                remote_effect: "applied".into(),
                error: None,
                origin: Origin::External,
            };
            bridge.answer("turn", &calls[0].id, &receipt).unwrap();
            assert_eq!(
                client.call(2, "once").1["result"]["content"][0]["text"],
                "recovered"
            );
        });
    }
}

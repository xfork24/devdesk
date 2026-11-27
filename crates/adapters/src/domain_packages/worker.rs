//! Bounded read-only domain process transport. The sandbox runs the kernel's bootstrap, which
//! loads the package entry as a module and calls one handler; see runtime/domain-worker.mjs.
use super::*;
use std::{io::Write, process::Command, time::Duration};

/// The image every handler runs in, pinned by digest.
pub const IMAGE: &str = "sha256:4660b1ca8b28d6d1906fd644abe34b2ed81d15434d26d845ef0aced307cf4b6f";
/// The Node major of that image.
pub const RUNTIME: &str = "node-24";
/// The Host-side bound of one call, container start included.
pub const TIMEOUT: Duration = Duration::from_secs(15);
/// The protocol the bootstrap speaks; a handshake names it.
pub const API: u64 = 2;
const MEMORY_MB: u64 = 128;
const CPUS: u64 = 1;
const PIDS: u64 = 48;
/// The bootstrap lets the container end the handler itself this long after the Host gave up.
const GRACE: Duration = Duration::from_secs(3);

const BOOTSTRAP: &str = include_str!("../../runtime/domain-worker.mjs");

/// What a handler can rely on when it runs. The capability catalog publishes it.
pub fn limits() -> Value {
    json!({
        "api": API,
        "image": IMAGE,
        "runtime": RUNTIME,
        "timeout_seconds": TIMEOUT.as_secs(),
        "memory_mb": MEMORY_MB,
        "cpus": CPUS,
        "pids": PIDS,
        "network": "none",
        "filesystem": "read-only",
    })
}

/// A file of the package or of the kernel, readable in the sandbox only.
fn mounted(body: &[u8]) -> Result<tempfile::NamedTempFile> {
    let mut file = tempfile::NamedTempFile::new().map_err(storage)?;
    file.write_all(body).map_err(storage)?;
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        file.as_file()
            .set_permissions(fs::Permissions::from_mode(0o444))
            .map_err(storage)?;
    }
    Ok(file)
}

fn request(package: &Package, runtime: &Runtime, method: &str, params: Value) -> Result<Value> {
    let bootstrap = mounted(BOOTSTRAP.as_bytes())?;
    let entry = mounted(&package.files[&runtime.entry])?;
    let container = format!("devdesk-domain-{}", Uuid::new_v4());
    // The limits are the ones `limits` publishes.
    let args = [
        "run".to_owned(),
        "--rm".into(),
        "-i".into(),
        "--pull=never".into(),
        "--network=none".into(),
        "--read-only".into(),
        "--cap-drop=ALL".into(),
        "--security-opt=no-new-privileges".into(),
        format!("--memory={MEMORY_MB}m"),
        format!("--cpus={CPUS}"),
        format!("--pids-limit={PIDS}"),
        "--name".into(),
        container.clone(),
        "--mount".into(),
        format!(
            "type=bind,source={},target=/bootstrap.mjs,readonly",
            bootstrap.path().display()
        ),
        "--mount".into(),
        format!(
            "type=bind,source={},target=/entry.mjs,readonly",
            entry.path().display()
        ),
        "--entrypoint".into(),
        "timeout".into(),
        IMAGE.into(),
        "--kill-after=2s".into(),
        format!("{}s", TIMEOUT.as_secs() + GRACE.as_secs()),
        "node".into(),
        "/bootstrap.mjs".into(),
    ];
    let mut command = Command::new("docker");
    command.args(args);
    let id = Uuid::new_v4().to_string();
    let mut input =
        serde_json::to_vec(&json!({"jsonrpc":"2.0","id":id,"method":method,"params":params}))
            .map_err(storage)?;
    input.push(b'\n');
    let result = crate::process::capture_command(command, TIMEOUT, Some(input));
    if result.is_err() {
        let _ = crate::process::capture(
            "docker",
            &["rm".into(), "-f".into(), container],
            Duration::from_secs(5),
        );
    }
    let result = result?;
    if result.code != 0 || result.truncated {
        return Err(err(
            "domain_worker_failed",
            format!("领域 worker 执行失败 (exit {})", result.code),
        ));
    }
    let response: Value = serde_json::from_slice(&result.stdout)
        .map_err(|_| err("domain_worker_failed", "领域 worker 响应不是 JSON-RPC"))?;
    answer(response, &id, params["handler"].as_str())
}

/// The result the worker reported, or its error. A handler that refused or failed is the
/// caller's answer with the handler's own code and message; the rest are worker failures.
pub(super) fn answer(response: Value, id: &str, handler: Option<&str>) -> Result<Value> {
    if response["id"] != id || response["jsonrpc"] != "2.0" {
        return Err(err(
            "domain_worker_failed",
            "领域 worker 返回了不匹配的请求身份",
        ));
    }
    let Some(error) = response.get("error") else {
        return response
            .get("result")
            .cloned()
            .ok_or_else(|| err("domain_worker_failed", "领域 worker 缺少结果"));
    };
    let message = error["message"].as_str().unwrap_or("领域 worker 拒绝操作");
    if error["data"]["refused"] == true {
        let code = error["data"]["code"]
            .as_str()
            .filter(|code| is_handler_code(code))
            .unwrap_or("handler_failed");
        return Err(err("domain_handler_rejected", message).with_context(
            "domain handler",
            json!({"handler":handler,"handler_code":code}),
        ));
    }
    Err(err(
        "domain_worker_failed",
        format!("领域 worker 执行失败：{message}"),
    ))
}

fn is_handler_code(code: &str) -> bool {
    let mut bytes = code.bytes();
    bytes.next().is_some_and(|first| first.is_ascii_lowercase())
        && code.len() <= 64
        && bytes.all(|byte| byte.is_ascii_lowercase() || byte.is_ascii_digit() || byte == b'_')
}

pub(super) fn describe(package: &Package, runtime: &Runtime) -> Result<()> {
    let response = request(package, runtime, "describe", json!({}))?;
    let expected = runtime.handlers.keys().collect::<Vec<_>>();
    require(
        response["api"] == API && response["handlers"] == json!(expected),
        "领域 worker 握手与清单不符",
    )
}

pub(super) fn invoke(
    package: &Package,
    runtime: &Runtime,
    name: &str,
    input: Value,
    core: Value,
) -> Result<Value> {
    request(
        package,
        runtime,
        "invoke",
        json!({"handler":name,"input":input,"core":core}),
    )
}

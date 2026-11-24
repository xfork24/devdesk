//! Bounded read-only domain process transport for the first extension slice.
use super::*;
use std::{io::Write, process::Command, time::Duration};

const IMAGE: &str = "sha256:4660b1ca8b28d6d1906fd644abe34b2ed81d15434d26d845ef0aced307cf4b6f";

fn request(package: &Package, runtime: &Runtime, method: &str, params: Value) -> Result<Value> {
    let mut entry = tempfile::NamedTempFile::new().map_err(storage)?;
    entry
        .write_all(&package.files[&runtime.entry])
        .map_err(storage)?;
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        entry
            .as_file()
            .set_permissions(fs::Permissions::from_mode(0o444))
            .map_err(storage)?;
    }
    let container = format!("devdesk-domain-{}", Uuid::new_v4());
    let mut command = Command::new("docker");
    command
        .args([
            "run",
            "--rm",
            "-i",
            "--pull=never",
            "--network=none",
            "--read-only",
            "--cap-drop=ALL",
            "--security-opt=no-new-privileges",
            "--memory=128m",
            "--cpus=1",
            "--pids-limit=48",
            "--name",
            &container,
            "--mount",
        ])
        .arg(format!(
            "type=bind,source={},target=/worker.mjs,readonly",
            entry.path().display()
        ))
        .args([
            "--entrypoint",
            "timeout",
            IMAGE,
            "--kill-after=2s",
            "18s",
            "node",
            "/worker.mjs",
        ]);
    let id = Uuid::new_v4().to_string();
    let mut input =
        serde_json::to_vec(&json!({"jsonrpc":"2.0","id":id,"method":method,"params":params}))
            .map_err(storage)?;
    input.push(b'\n');
    let result = crate::process::capture_command(command, Duration::from_secs(15), Some(input));
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
    if response["id"] != id || response["jsonrpc"] != "2.0" || response.get("error").is_some() {
        return Err(err(
            "domain_worker_failed",
            "领域 worker 拒绝操作或返回不匹配的请求身份",
        ));
    }
    response
        .get("result")
        .cloned()
        .ok_or_else(|| err("domain_worker_failed", "领域 worker 缺少结果"))
}

pub(super) fn describe(package: &Package, runtime: &Runtime) -> Result<()> {
    let response = request(package, runtime, "describe", json!({}))?;
    let expected = runtime.handlers.keys().collect::<Vec<_>>();
    require(
        response["api"] == 1 && response["handlers"] == json!(expected),
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

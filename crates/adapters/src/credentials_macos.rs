//! The immutable helper is the Keychain principal; the mutable Web Host is its authenticated caller.
use super::*;
use std::{
    os::unix::fs::{MetadataExt, PermissionsExt},
    time::Duration,
};
const HELPER: &str = "devdesk-credential-helper";
fn verify(path: &Path) -> Result<()> {
    reject_symlink(path)?;
    let metadata = fs::metadata(path).map_err(storage)?;
    require(
        metadata.is_file()
            && metadata.uid() == unsafe { libc::geteuid() }
            && metadata.mode() & 0o022 == 0,
        "凭据助手权限或归属无效",
    )?;
    let executable = std::env::current_exe().map_err(storage)?;
    let output = crate::process::capture(
        "/usr/bin/codesign",
        &[
            "-d".into(),
            "-r-".into(),
            executable.to_string_lossy().into(),
        ],
        Duration::from_secs(3),
    )?;
    let text = String::from_utf8(output.stdout).map_err(storage)?;
    let requirement = text
        .lines()
        .find_map(|line| line.strip_prefix("designated => "))
        .ok_or_else(|| err("credential_helper_unavailable", "Host 缺少稳定代码签名"))?;
    require(
        requirement.contains("identifier \"dev.devdesk.host\"")
            && !requirement.contains("cdhash")
            && requirement.contains("certificate"),
        "需要证书签名的 Host；请运行 scripts/dev/dev.sh 或 scripts/dev/build.sh",
    )?;
    let requirement = requirement.replace(
        "identifier \"dev.devdesk.host\"",
        "identifier \"dev.devdesk.credentials\"",
    );
    let output = crate::process::capture(
        "/usr/bin/codesign",
        &[
            "--verify".into(),
            "--strict".into(),
            "-R".into(),
            format!("={requirement}"),
            path.to_string_lossy().into(),
        ],
        Duration::from_secs(3),
    )?;
    require(
        output.code == 0,
        "凭据助手签名与当前 Host 不匹配，请安装同证书签名的助手",
    )
}
fn verify_manifest(binary: &Path) -> Result<()> {
    let path = binary.parent().unwrap().join("manifest.json");
    reject_symlink(&path)?;
    let bytes = fs::read(&path).map_err(|_| {
        err(
            "credential_helper_unavailable",
            "凭据助手安装清单缺失，请重新安装",
        )
    })?;
    require(bytes.len() <= 4096, "凭据助手清单过大")?;
    let value: Value = serde_json::from_slice(&bytes)
        .map_err(|_| err("credential_helper_unavailable", "凭据助手清单无效"))?;
    let actual = format!("{:x}", Sha256::digest(fs::read(binary).map_err(storage)?));
    require(
        value["protocol"] == 2 && value["binary_sha256"].as_str() == Some(actual.as_str()),
        "凭据助手内容与安装清单不一致，请显式修复安装",
    )
}

fn directory(path: &Path) -> Result<()> {
    reject_symlink(path)?;
    if !path.exists() {
        fs::create_dir(path).map_err(storage)?;
        fs::set_permissions(path, fs::Permissions::from_mode(0o700)).map_err(storage)?;
    }
    let metadata = fs::metadata(path).map_err(storage)?;
    require(
        metadata.is_dir()
            && metadata.uid() == unsafe { libc::geteuid() }
            && metadata.mode() & 0o022 == 0,
        "凭据助手目录权限无效",
    )
}
pub(super) fn helper_path() -> Result<PathBuf> {
    let home = PathBuf::from(
        std::env::var_os("HOME")
            .ok_or_else(|| err("credential_helper_unavailable", "无法定位当前用户凭据助手"))?,
    );
    let support = home.join("Library/Application Support");
    let app = support.join("DevDesk");
    directory(&app)?;
    let parent = app.join("credential-helper");
    directory(&parent)?;
    let version = parent.join("v1");
    directory(&version)?;
    let installed = version.join(HELPER);
    if !installed.exists() {
        let bundle = std::env::current_exe()
            .map_err(storage)?
            .parent()
            .unwrap()
            .join("credential-helper")
            .join(HELPER);
        if !bundle.is_file() {
            return Err(err(
                "credential_helper_unavailable",
                "请先运行 scripts/platform/macos/install-macos-credential-helper.sh；运行包需包含 credential-helper",
            ));
        }
        verify(&bundle)?;
        verify_manifest(&bundle)?;
        let temporary = version.join(format!(".install-{}", id()));
        fs::copy(&bundle, &temporary).map_err(storage)?;
        fs::set_permissions(&temporary, fs::Permissions::from_mode(0o700)).map_err(storage)?;
        verify(&temporary)?;
        // Publish without replacing a helper installed by another process.
        match fs::hard_link(&temporary, &installed) {
            Ok(()) => {}
            Err(e) if e.kind() == std::io::ErrorKind::AlreadyExists => {}
            Err(e) => {
                let _ = fs::remove_file(&temporary);
                return Err(storage(e));
            }
        }
        fs::remove_file(&temporary).map_err(storage)?;
        let manifest = version.join("manifest.json");
        if !manifest.exists() {
            let pending = version.join(format!(".manifest-{}", id()));
            fs::copy(bundle.parent().unwrap().join("manifest.json"), &pending).map_err(storage)?;
            fs::set_permissions(&pending, fs::Permissions::from_mode(0o600)).map_err(storage)?;
            match fs::hard_link(&pending, &manifest) {
                Ok(()) => {}
                Err(e) if e.kind() == std::io::ErrorKind::AlreadyExists => {}
                Err(e) => {
                    let _ = fs::remove_file(&pending);
                    return Err(storage(e));
                }
            }
            fs::remove_file(&pending).map_err(storage)?;
        }
    }
    verify(&installed)?;
    verify_manifest(&installed)?;
    Ok(installed)
}
/// Explicit CLI action. Publishes a verified bundled principal without opening Keychain.
pub(super) fn update_bundled_helper() -> Result<()> {
    let installed = helper_path()?;
    let bundle = std::env::current_exe()
        .map_err(storage)?
        .parent()
        .unwrap()
        .join("credential-helper")
        .join(HELPER);
    verify(&bundle)?;
    verify_manifest(&bundle)?;
    let manifest = fs::read(bundle.parent().unwrap().join("manifest.json")).map_err(storage)?;
    let value: Value = serde_json::from_slice(&manifest).map_err(storage)?;
    require(value["protocol"] == 2, "运行包没有支持多服务商的凭据助手")?;
    let parent = installed.parent().unwrap();
    let pending = parent.join(format!(".update-{}", id()));
    let pending_manifest = parent.join(format!(".manifest-{}", id()));
    fs::copy(&bundle, &pending).map_err(storage)?;
    fs::set_permissions(&pending, fs::Permissions::from_mode(0o700)).map_err(storage)?;
    verify(&pending)?;
    fs::write(&pending_manifest, &manifest).map_err(storage)?;
    fs::set_permissions(&pending_manifest, fs::Permissions::from_mode(0o600)).map_err(storage)?;
    fs::rename(&pending, &installed).map_err(storage)?;
    fs::rename(&pending_manifest, parent.join("manifest.json")).map_err(storage)?;
    verify_manifest(&installed)
}
pub(super) fn call(
    operation: &str,
    service: &str,
    account: &str,
    secret: Option<&str>,
) -> Result<Value> {
    let executable = helper_path()?;
    let request =
        json!({"operation":operation,"service":service,"account":account,"secret":secret});
    let output = crate::process::capture_command(
        std::process::Command::new(executable),
        Duration::from_secs(if operation == "authorize" { 30 } else { 8 }),
        Some(serde_json::to_vec(&request).map_err(storage)?),
    )
    .map_err(|error| {
        let code = error.code.as_str();
        err(
            "credential_helper_process",
            format!("凭据助手 {operation} 调用失败（{code}）；请检查宿主机助手安装及运行状态"),
        )
    })?;
    if output.code != 0 || output.truncated {
        return Err(err(
            "credential_helper_invalid_response",
            format!(
                "凭据助手 {operation} 未返回完整结果（退出码 {}）",
                output.code
            ),
        ));
    }
    let value: Value = serde_json::from_slice(&output.stdout).map_err(|_| {
        err(
            "credential_helper_invalid_response",
            format!("凭据助手 {operation} 返回了无效 JSON"),
        )
    })?;
    decode_response(operation, account, value)
}

fn decode_response(operation: &str, account: &str, value: Value) -> Result<Value> {
    if value["ok"] == true && value["result"].is_object() {
        return Ok(value["result"].clone());
    }
    let integration = account
        .strip_prefix("integration-key-")
        .is_some_and(|id| uuid::Uuid::parse_str(id).is_ok());
    let (code, message) = match value["code"].as_str() {
        Some("invalid_params") if integration => (
            "credential_helper_upgrade_required",
            "当前助手拒绝 integration-key 凭据请求。请显式升级同证书签名的 macOS 助手（源码安装运行 scripts/platform/macos/install-macos-credential-helper.sh --update；运行包执行 devdesk-host --update-credential-helper）",
        ),
        Some("invalid_params") => (
            "credential_helper_invalid_request",
            "助手拒绝请求参数；请核对 Host 和助手的版本及所配置的凭据类型",
        ),
        Some("credential_helper_caller") => (
            "credential_helper_unavailable",
            "助手无法核验当前 Host 的签名；请使用同证书签名的 Host 和助手，若 Host 程序已被重新构建，请从签名副本重启",
        ),
        Some("authentication_required") => (
            "authentication_required",
            "macOS 钥匙串需要用户交互或拒绝访问；请检查钥匙串解锁状态及 DevDesk 助手的访问权限",
        ),
        Some("credentials_required") => (
            "credentials_required",
            "macOS 钥匙串中没有对应的凭据条目；请配置该凭据",
        ),
        Some("credential_store") => (
            "credential_store",
            "macOS 钥匙串操作失败；请检查宿主机钥匙串状态",
        ),
        _ => (
            "credential_helper_invalid_response",
            "助手响应不符合协议；请检查 Host 和助手的版本",
        ),
    };
    let status = value["os_status"]
        .as_i64()
        .and_then(|status| i32::try_from(status).ok());
    let suffix = status
        .map(|status| format!("（OSStatus {status}）"))
        .unwrap_or_default();
    Err(err(
        code,
        format!("凭据助手 {operation} 失败{suffix}。{message}"),
    ))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn legacy_integration_rejection_is_an_upgrade_diagnostic() {
        let account = "integration-key-01234567-89ab-cdef-0123-456789abcdef";
        let rejected = json!({"ok":false,"code":"invalid_params","secret":"must-not-appear"});
        for operation in ["status", "read", "store"] {
            let error = decode_response(operation, account, rejected.clone()).unwrap_err();
            assert_eq!(error.code, "credential_helper_upgrade_required");
            assert!(error.message.contains(operation));
            assert!(error.message.contains("--update"));
            assert!(!json!(error).to_string().contains("must-not-appear"));
        }
        assert_eq!(
            decode_response("status", "integration-key-invalid", rejected)
                .unwrap_err()
                .code,
            "credential_helper_invalid_request"
        );
    }

    #[test]
    fn preserves_keychain_status_and_signed_caller_failures() {
        let account = "integration-key-01234567-89ab-cdef-0123-456789abcdef";
        for (code, os_status) in [
            ("authentication_required", -25308),
            ("credentials_required", -25300),
            ("credential_store", -50),
        ] {
            let error = decode_response(
                "read",
                account,
                json!({"ok":false,"code":code,"os_status":os_status}),
            )
            .unwrap_err();
            assert_eq!(error.code, code);
            assert!(error.message.contains(&format!("OSStatus {os_status}")));
        }
        let error = decode_response(
            "status",
            account,
            json!({"ok":false,"code":"credential_helper_caller"}),
        )
        .unwrap_err();
        assert_eq!(error.code, "credential_helper_unavailable");
        assert!(error.message.contains("签名"));
        assert_eq!(
            decode_response(
                "status",
                account,
                json!({"ok":true,"result":{"configured":false}})
            )
            .unwrap(),
            json!({"configured":false})
        );
    }
}

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
        "需要证书签名的 Host；请运行 scripts/dev.sh 或 scripts/build.sh",
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
        value["protocol"] == 1 && value["binary_sha256"].as_str() == Some(actual.as_str()),
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
                "请先运行 scripts/install-macos-credential-helper.sh；运行包需包含 credential-helper",
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
    .map_err(|_| {
        err(
            "authentication_required",
            "凭据助手未完成；后台不会弹窗，显式授权最长等待 30 秒",
        )
    })?;
    require(
        output.code == 0 && !output.truncated,
        "凭据助手未返回有效结果",
    )?;
    let value: Value = serde_json::from_slice(&output.stdout)
        .map_err(|_| err("credential_store", "凭据助手响应无效"))?;
    if value["ok"] != true {
        if value["code"] == "credential_helper_caller" {
            return Err(err(
                "credential_helper_unavailable",
                "无法核验当前 Host 签名；如果运行期间重新编译了程序，请重启 Host，无需重新授权凭据",
            ));
        }
        let code = match value["code"].as_str().unwrap_or("") {
            "authentication_required" => "authentication_required",
            "credentials_required" => "credentials_required",
            "credential_helper_caller" => "credential_helper_unavailable",
            _ => "credential_store",
        };
        return Err(err(
            code,
            "凭据助手尚未获准读取；请在设置页显式授权。Host 重编译不改变助手身份。",
        ));
    }
    Ok(value["result"].clone())
}

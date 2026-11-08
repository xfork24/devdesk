//! OS vault access. Background macOS calls cannot display authorization dialogs.
use super::*;

#[derive(Debug, Clone, Serialize)]
pub struct CredentialStatus {
    pub provider: HarnessProvider,
    pub configured: bool,
    pub reference: String,
    pub storage: &'static str,
    pub available: bool,
}

pub struct CredentialVault {
    service: String,
}

#[cfg(target_os = "linux")]
fn secret_tool() -> Result<PathBuf> {
    std::env::var_os("PATH").and_then(|paths| std::env::split_paths(&paths).map(|p|p.join("secret-tool")).find(|p|p.is_file()))
        .ok_or_else(|| err("credential_store_unavailable", "Linux 需要 Secret Service 与 secret-tool；Ubuntu 安装 libsecret-tools 并使用已解锁的桌面凭据库"))
}
impl CredentialVault {
    pub fn new(dataset: &Path) -> Result<Self> {
        let root = fs::canonicalize(dataset).map_err(storage)?;
        Ok(Self {
            service: format!(
                "devdesk.{:x}",
                Sha256::digest(root.to_string_lossy().as_bytes())
            ),
        })
    }
    fn provider(provider: HarnessProvider) -> Result<()> {
        require(
            provider == HarnessProvider::Dsh,
            "Codex 使用原生 OAuth 登录流程，不通过 DSH API key 接口配置",
        )
    }
    fn status_value(
        &self,
        provider: HarnessProvider,
        account: &str,
        configured: bool,
    ) -> CredentialStatus {
        CredentialStatus {
            provider,
            configured,
            reference: format!("{}:{account}", self.service),
            storage: if cfg!(target_os = "macos") {
                "macos_keychain"
            } else if cfg!(target_os = "linux") {
                "linux_secret_service"
            } else {
                "unsupported"
            },
            available: {
                #[cfg(target_os = "macos")]
                {
                    crate::credentials_macos::helper_path().is_ok()
                }
                #[cfg(not(target_os = "macos"))]
                {
                    false
                }
            } || {
                #[cfg(target_os = "linux")]
                {
                    secret_tool().is_ok()
                }
                #[cfg(not(target_os = "linux"))]
                {
                    false
                }
            },
        }
    }
    pub fn configure(&self, provider: HarnessProvider, secret: &str) -> Result<CredentialStatus> {
        Self::provider(provider)?;
        require(
            (8..=4096).contains(&secret.len()) && secret.bytes().all(|b| b.is_ascii_graphic()),
            "API key 格式无效",
        )?;
        let account = format!("deepseek-api-key-{}", id());
        #[cfg(target_os = "macos")]
        crate::credentials_macos::call("store", &self.service, &account, Some(secret))?;
        #[cfg(target_os = "linux")]
        {
            let mut command = std::process::Command::new(secret_tool()?);
            command.args([
                "store",
                "--label=DevDesk DeepSeek official API key",
                "service",
                &self.service,
                "account",
                &account,
            ]);
            let output = crate::process::capture_command(
                command,
                std::time::Duration::from_secs(20),
                Some(secret.as_bytes().to_vec()),
            )
            .map_err(|_| {
                err(
                    "credential_store_unavailable",
                    "Secret Service 写入超时，请解锁桌面凭据库",
                )
            })?;
            require(
                output.code == 0,
                "无法保存 Secret Service 凭据，请检查桌面登录会话与凭据库",
            )?;
        }
        #[cfg(not(any(target_os = "macos", target_os = "linux")))]
        return Err(err(
            "credential_store_unavailable",
            "当前系统没有受支持的凭据库",
        ));
        #[allow(unreachable_code)]
        Ok(self.status_value(provider, &account, true))
    }
    pub fn status(&self, provider: HarnessProvider) -> Result<CredentialStatus> {
        self.status_reference(provider, None)
    }
    fn reference_account(&self, reference: Option<&str>) -> Result<String> {
        let Some(reference) = reference else {
            return Ok("deepseek-official-api-key".into());
        };
        let prefix = format!("{}:", self.service);
        let account = reference
            .strip_prefix(&prefix)
            .ok_or_else(|| err("credential_store", "凭据引用不属于当前数据集"))?;
        require(
            account == "deepseek-official-api-key"
                || account
                    .strip_prefix("deepseek-api-key-")
                    .is_some_and(|id| uuid::Uuid::parse_str(id).is_ok()),
            "凭据引用格式无效",
        )?;
        Ok(account.into())
    }
    pub fn status_reference(
        &self,
        provider: HarnessProvider,
        reference: Option<&str>,
    ) -> Result<CredentialStatus> {
        Self::provider(provider)?;
        let account = self.reference_account(reference)?;
        #[cfg(target_os = "macos")]
        let configured = if crate::credentials_macos::helper_path().is_ok() {
            crate::credentials_macos::call("status", &self.service, &account, None)?["configured"]
                .as_bool()
                .unwrap_or(false)
        } else {
            reference.is_some()
        };
        #[cfg(target_os = "linux")]
        // The global vault reference is configuration metadata, not proof of an
        // unlocked collection. Status does not read/unlock a Linux secret.
        let configured = reference.is_some();
        #[cfg(not(any(target_os = "macos", target_os = "linux")))]
        let configured = false;
        Ok(self.status_value(provider, &account, configured))
    }
    pub(crate) fn read_reference(&self, reference: &str) -> Result<Vec<u8>> {
        let account = self.reference_account(Some(reference))?;
        #[cfg(target_os = "macos")]
        {
            let value = crate::credentials_macos::call("read", &self.service, &account, None)?;
            value["secret"]
                .as_str()
                .map(|v| v.as_bytes().to_vec())
                .ok_or_else(|| err("credential_store", "凭据助手未返回秘密"))
        }
        #[cfg(target_os = "linux")]
        {
            let mut command = std::process::Command::new(secret_tool()?);
            command.args(["lookup", "service", &self.service, "account", &account]);
            let mut output =
                crate::process::capture_command(command, std::time::Duration::from_secs(15), None)
                    .map_err(|_| {
                        err(
                            "authentication_required",
                            "Secret Service 读取超时，请解锁桌面凭据库后重试",
                        )
                    })?;
            if output.code != 0 || output.truncated {
                return Err(err(
                    "authentication_required",
                    "Secret Service 凭据不可用，请检查桌面会话并重新保存密钥",
                ));
            }
            while output
                .stdout
                .last()
                .is_some_and(|b| *b == b'\n' || *b == b'\r')
            {
                output.stdout.pop();
            }
            require(!output.stdout.is_empty(), "DeepSeek 凭据为空")?;
            Ok(output.stdout)
        }
        #[cfg(not(any(target_os = "macos", target_os = "linux")))]
        {
            let _ = account;
            Err(err(
                "credential_store_unavailable",
                "当前系统没有受支持的凭据库",
            ))
        }
    }
    /// Only the explicit settings action allows UI in the immutable signed helper.
    /// Its private response is consumed here; HTTP never returns a secret.
    pub fn authorize_interactively(&self, reference: &str) -> Result<()> {
        let account = self.reference_account(Some(reference))?;
        #[cfg(target_os = "macos")]
        {
            crate::credentials_macos::call("authorize", &self.service, &account, None)?;
            Ok(())
        }
        #[cfg(not(target_os = "macos"))]
        {
            let _ = account;
            self.verify_reference(reference)
        }
    }
    pub fn verify_reference(&self, reference: &str) -> Result<()> {
        let mut bytes = self.read_reference(reference)?;
        bytes.fill(0);
        Ok(())
    }
    pub fn request_authorization(dataset: &Path, reference: &str) -> Result<()> {
        Self::new(dataset)?.authorize_interactively(reference)
    }
}
#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn opaque_vault_references_are_dataset_scoped_and_never_accept_a_path() {
        let a = tempfile::tempdir().unwrap();
        let b = tempfile::tempdir().unwrap();
        let first = CredentialVault::new(a.path()).unwrap();
        let second = CredentialVault::new(b.path()).unwrap();
        let reference = first
            .status_value(HarnessProvider::Dsh, "deepseek-official-api-key", false)
            .reference;
        assert!(first.reference_account(Some(&reference)).is_ok());
        assert!(second.reference_account(Some(&reference)).is_err());
        assert!(first.reference_account(Some("/tmp/secret")).is_err());
    }
}

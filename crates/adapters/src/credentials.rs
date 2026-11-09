//! OS vault access. Background macOS calls cannot display authorization dialogs.
use super::*;

#[derive(Debug, Clone, Serialize)]
pub struct CredentialStatus {
    pub provider: HarnessProvider,
    pub model_provider: DshModelProvider,
    pub auth_kind: &'static str,
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
    pub fn update_bundled_helper() -> Result<()> {
        #[cfg(target_os = "macos")]
        { crate::credentials_macos::update_bundled_helper() }
        #[cfg(not(target_os = "macos"))]
        { Err(err("unsupported", "仅 macOS 使用凭据助手")) }
    }
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
            model_provider: DshModelProvider::Deepseek,
            auth_kind: "api_key",
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
        self.configure_model(DshModelProvider::Deepseek, secret)
    }
    pub fn configure_model(&self, model_provider: DshModelProvider, secret: &str) -> Result<CredentialStatus> {
        require(model_provider.requires_key(), "Ollama 本地服务无需保存 API key")?;
        #[cfg(target_os = "macos")]
        if model_provider != DshModelProvider::Deepseek {
            require(crate::credentials_macos::model_keys_available(), "请显式升级共享凭据助手以支持新服务商：install-macos-credential-helper.sh --update")?;
        }
        require(
            (8..=4096).contains(&secret.len()) && secret.bytes().all(|b| b.is_ascii_graphic()),
            "API key 格式无效",
        )?;
        let account = if model_provider == DshModelProvider::Deepseek { format!("deepseek-api-key-{}", id()) } else { format!("dsh-{}-api-key-{}", model_provider.as_str(), id()) };
        #[cfg(target_os = "macos")]
        crate::credentials_macos::call("store", &self.service, &account, Some(secret))?;
        #[cfg(target_os = "linux")]
        {
            let mut command = std::process::Command::new(secret_tool()?);
            command.args([
                "store",
                "--label=DevDesk model provider API key",
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
        Ok(self.model_status_value(model_provider, &account, true))
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
        require(Self::account_provider(account).is_some(), "凭据引用格式无效")?;
        Ok(account.into())
    }
    pub fn status_reference(
        &self,
        provider: HarnessProvider,
        reference: Option<&str>,
    ) -> Result<CredentialStatus> {
        Self::provider(provider)?;
        self.status_model_reference(DshModelProvider::Deepseek, reference)
    }
    pub fn status_model_reference(&self, model_provider: DshModelProvider, reference: Option<&str>) -> Result<CredentialStatus> {
        if model_provider == DshModelProvider::Ollama {
            return Ok(CredentialStatus {provider:HarnessProvider::Dsh,model_provider,auth_kind:"none",configured:true,reference:String::new(),storage:"none",available:true});
        }
        let account = if let Some(reference) = reference { self.model_account(model_provider, reference)? }
            else if model_provider == DshModelProvider::Deepseek { self.reference_account(None)? }
            else { format!("dsh-{}-api-key-{}", model_provider.as_str(), uuid::Uuid::nil()) };

        #[cfg(target_os = "macos")]
        let configured = if crate::credentials_macos::helper_path().is_ok()
            && (model_provider == DshModelProvider::Deepseek || crate::credentials_macos::model_keys_available()) {
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
        Ok(self.model_status_value(model_provider, &account, configured))
    }
    fn model_status_value(&self, p: DshModelProvider, account: &str, configured: bool) -> CredentialStatus {
        let mut status = self.status_value(HarnessProvider::Dsh, account, configured);
        status.model_provider = p;
        #[cfg(target_os = "macos")]
        if p != DshModelProvider::Deepseek { status.available = crate::credentials_macos::model_keys_available(); }
        status
    }
    fn account_provider(account: &str) -> Option<DshModelProvider> {
        if account == "deepseek-official-api-key" { return Some(DshModelProvider::Deepseek); }
        for p in DshModelProvider::ALL.into_iter().filter(|p|p.requires_key()) {
            let prefix = if p == DshModelProvider::Deepseek { "deepseek-api-key-".to_owned() } else { format!("dsh-{}-api-key-",p.as_str()) };
            if account.strip_prefix(&prefix).is_some_and(|id|id.len() == 36 && uuid::Uuid::parse_str(id).is_ok()) { return Some(p); }
        }
        None
    }
    fn model_account(&self, p: DshModelProvider, reference: &str) -> Result<String> {
        let account = self.reference_account(Some(reference))?;
        require(Self::account_provider(&account) == Some(p), "凭据引用不属于所选模型服务商")?;
        Ok(account)
    }
    pub fn validate_model_reference(&self, p: DshModelProvider, reference: &str) -> Result<()> {
        self.model_account(p, reference).map(|_|())
    }
    pub(crate) fn read_model_reference(&self, p: DshModelProvider, reference: &str) -> Result<Vec<u8>> {
        self.model_account(p, reference)?;
        self.read_reference(reference)
    }
    pub fn verify_model_reference(&self, p: DshModelProvider, reference: &str) -> Result<()> {
        let mut key = self.read_model_reference(p, reference)?;
        key.fill(0);
        Ok(())
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
            require(!output.stdout.is_empty(), "模型服务商凭据为空")?;
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
    #[test]
    fn provider_accounts_cannot_be_reused_across_providers_or_datasets() {
        let a = tempfile::tempdir().unwrap();
        let b = tempfile::tempdir().unwrap();
        let first = CredentialVault::new(a.path()).unwrap();
        let second = CredentialVault::new(b.path()).unwrap();
        for p in DshModelProvider::ALL.into_iter().filter(|p|p.requires_key()) {
            let account = if p == DshModelProvider::Deepseek {format!("deepseek-api-key-{}",id())}else{format!("dsh-{}-api-key-{}",p.as_str(),id())};
            let reference = format!("{}:{account}",first.service);
            assert!(first.validate_model_reference(p,&reference).is_ok());
            assert!(second.validate_model_reference(p,&reference).is_err());
            for other in DshModelProvider::ALL.into_iter().filter(|other|*other!=p) {
                assert!(first.validate_model_reference(other,&reference).is_err());
            }
        }
        let status = first.status_model_reference(DshModelProvider::Ollama,None).unwrap();
        assert!(status.available && status.configured && status.reference.is_empty());
        assert_eq!(status.auth_kind,"none");
        assert!(first.configure_model(DshModelProvider::Ollama,"FAKE_TEST_VALUE").is_err());
    }
}

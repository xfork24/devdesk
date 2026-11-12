//! OS vault access. Background macOS calls cannot display authorization dialogs.
use super::*;

#[derive(Debug, Clone, Serialize)]
pub struct CredentialStatus {
    pub provider: HarnessProvider,
    pub model_provider: String,
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
        {
            crate::credentials_macos::update_bundled_helper()
        }
        #[cfg(not(target_os = "macos"))]
        {
            Err(err("unsupported", "仅 macOS 使用凭据助手"))
        }
    }
    fn integration_account(reference: &str) -> Result<String> {
        let id = reference
            .strip_prefix("integration-secret:")
            .ok_or_else(|| err("credential_store", "无效接入凭据引用"))?;
        require(uuid::Uuid::parse_str(id).is_ok(), "无效接入凭据身份")?;
        Ok(format!("integration-key-{id}"))
    }
    pub fn configure_integration(&self, secret: &str) -> Result<String> {
        let reference = format!("integration-secret:{}", id());
        self.write_integration(&reference, secret)?;
        Ok(reference)
    }
    pub fn write_integration(&self, reference: &str, secret: &str) -> Result<()> {
        require(
            !secret.is_empty() && secret.len() <= 65536 && !secret.contains(['\n', '\r']),
            "凭据内容格式无效或过大",
        )?;
        let account = Self::integration_account(reference)?;
        #[cfg(target_os = "macos")]
        {
            crate::credentials_macos::call("store", &self.service, &account, Some(secret))
                .map_err(|error| {
                    if error.code != "credential_store" {
                        return error;
                    }
                    err(
                        "credential_helper_upgrade_required",
                        "接入凭据需要支持 integration-key 的签名凭据助手；请显式升级助手",
                    )
                })?;
        }
        #[cfg(target_os = "linux")]
        {
            let mut command = std::process::Command::new(secret_tool()?);
            command.args([
                "store",
                "--label=DevDesk integration",
                "service",
                &self.service,
                "account",
                &account,
            ]);
            let output = crate::process::capture_command(
                command,
                std::time::Duration::from_secs(20),
                Some(secret.as_bytes().to_vec()),
            )?;
            require(output.code == 0, "系统凭据库写入失败")?;
        }
        #[cfg(not(any(target_os = "macos", target_os = "linux")))]
        {
            return Err(err("unsupported", "当前平台没有凭据适配器"));
        }
        Ok(())
    }
    pub fn read_integration(&self, reference: &str) -> Result<String> {
        let account = Self::integration_account(reference)?;
        #[cfg(target_os = "macos")]
        {
            let value = crate::credentials_macos::call("read", &self.service, &account, None)?;
            return value["secret"]
                .as_str()
                .map(str::to_owned)
                .ok_or_else(|| err("credential_missing", "接入凭据不可用"));
        }
        #[cfg(target_os = "linux")]
        {
            let mut command = std::process::Command::new(secret_tool()?);
            command.args(["lookup", "service", &self.service, "account", &account]);
            let output =
                crate::process::capture_command(command, std::time::Duration::from_secs(20), None)?;
            require(output.code == 0, "接入凭据不可用")?;
            return String::from_utf8(output.stdout)
                .map(|s| s.trim_end_matches('\n').to_owned())
                .map_err(storage);
        }
        #[cfg(not(any(target_os = "macos", target_os = "linux")))]
        {
            Err(err("unsupported", "当前平台没有凭据适配器"))
        }
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
            model_provider: "deepseek".into(),
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
    pub fn configure(&self, provider: HarnessProvider, deepseek: &ProviderFacts, secret: &str) -> Result<CredentialStatus> {
        Self::provider(provider)?;
        self.configure_model(deepseek, secret)
    }
    /// Account namespaces come from the provider's published facts, never a code roster.
    pub fn configure_model(&self, facts: &ProviderFacts, secret: &str) -> Result<CredentialStatus> {
        require(facts.requires_key, "本地服务无需保存 API key")?;
        require(
            (8..=4096).contains(&secret.len()) && secret.bytes().all(|b| b.is_ascii_graphic()),
            "API key 格式无效",
        )?;
        let account = format!("{}{}", facts.credential_account, id());
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
        Ok(self.model_status_value(facts, &account, true))
    }
    pub fn status(&self, provider: HarnessProvider, deepseek: &ProviderFacts) -> Result<CredentialStatus> {
        self.status_reference(provider, deepseek, None)
    }
    fn reference_account(&self, reference: &str) -> Result<String> {
        let prefix = format!("{}:", self.service);
        let account = reference
            .strip_prefix(&prefix)
            .ok_or_else(|| err("credential_store", "凭据引用不属于当前数据集"))?;
        require(Self::account_shape(account).is_some(), "凭据引用格式无效")?;
        Ok(account.into())
    }
    pub fn status_reference(
        &self,
        provider: HarnessProvider,
        deepseek: &ProviderFacts,
        reference: Option<&str>,
    ) -> Result<CredentialStatus> {
        Self::provider(provider)?;
        self.status_model_reference(deepseek, reference)
    }
    pub fn status_model_reference(
        &self,
        facts: &ProviderFacts,
        reference: Option<&str>,
    ) -> Result<CredentialStatus> {
        if !facts.requires_key {
            return Ok(CredentialStatus {
                provider: HarnessProvider::Dsh,
                model_provider: facts.id.clone(),
                auth_kind: "none",
                configured: true,
                reference: String::new(),
                storage: "none",
                available: true,
            });
        }
        let Some(reference) = reference else {
            return Ok(self.model_status_value(facts, "", false));
        };
        let account = self.model_account(facts, reference)?;

        #[cfg(target_os = "macos")]
        let configured = if crate::credentials_macos::helper_path().is_ok() {
            crate::credentials_macos::call("status", &self.service, &account, None)?["configured"]
                .as_bool()
                .unwrap_or(false)
        } else {
            true
        };
        #[cfg(target_os = "linux")]
        // The global vault reference is configuration metadata, not proof of an
        // unlocked collection. Status does not read/unlock a Linux secret.
        let configured = true;
        #[cfg(not(any(target_os = "macos", target_os = "linux")))]
        let configured = false;
        Ok(self.model_status_value(facts, &account, configured))
    }
    fn model_status_value(
        &self,
        p: &ProviderFacts,
        account: &str,
        configured: bool,
    ) -> CredentialStatus {
        let mut status = self.status_value(HarnessProvider::Dsh, account, configured);
        status.model_provider = p.id.clone();
        if account.is_empty() {
            status.reference.clear();
        }
        status
    }
    /// Shape check: the account must be `<credential prefix><uuid>`.
    fn account_shape(account: &str) -> Option<(&str, &str)> {
        if account.len() < 37 {
            return None;
        }
        let split = account.len() - 36;
        let (prefix, uuid) = account.split_at(split);
        uuid::Uuid::parse_str(uuid).is_ok().then_some((prefix, uuid))
    }
    fn account_for_prefix(&self, prefix: &str, reference: &str) -> Result<String> {
        let account = self.reference_account(reference)?;
        require(
            Self::account_shape(&account).is_some_and(|(found, _)| found == prefix),
            "凭据引用不属于所选模型服务商",
        )?;
        Ok(account)
    }
    fn model_account(&self, facts: &ProviderFacts, reference: &str) -> Result<String> {
        self.account_for_prefix(&facts.credential_account, reference)
    }
    pub fn validate_model_reference(&self, facts: &ProviderFacts, reference: &str) -> Result<()> {
        self.model_account(facts, reference).map(|_| ())
    }
    pub(crate) fn read_model_reference(
        &self,
        facts: &ProviderFacts,
        reference: &str,
    ) -> Result<Vec<u8>> {
        self.model_account(facts, reference)?;
        self.read_reference(reference)
    }
    /// Reads the key for a frozen model connection; the snapshot carries the
    /// provider's credential convention so no provider roster is needed here.
    pub fn read_connection_reference(
        &self,
        connection: &ModelConnection,
        reference: &str,
    ) -> Result<Vec<u8>> {
        require(
            !connection.credential_account.is_empty(),
            "此模型连接不使用凭据",
        )?;
        self.account_for_prefix(&connection.credential_account, reference)?;
        self.read_reference(reference)
    }
    pub fn verify_model_reference(&self, facts: &ProviderFacts, reference: &str) -> Result<()> {
        let mut key = self.read_model_reference(facts, reference)?;
        key.fill(0);
        Ok(())
    }
    pub(crate) fn read_reference(&self, reference: &str) -> Result<Vec<u8>> {
        let account = self.reference_account(reference)?;
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
        let account = self.reference_account(reference)?;
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
    fn facts(id: &str, account: &str, requires_key: bool) -> ProviderFacts {
        serde_json::from_value(json!({
            "id": id, "label": id,
            "kind": if requires_key { "cloud_subscription" } else { "local_server" },
            "harness": "dsh", "endpoint": "https://provider.invalid",
            "route": id, "protocol": "openai-completions",
            "requires_key": requires_key, "credential_account": account
        }))
        .unwrap()
    }
    const KEYED: &[(&str, &str)] = &[
        ("deepseek", "deepseek-api-key-"),
        ("zai-coding-cn", "dsh-zai-coding-cn-api-key-"),
        ("minimax-cn", "dsh-minimax-cn-api-key-"),
    ];
    #[test]
    fn opaque_vault_references_are_dataset_scoped_and_never_accept_a_path() {
        let a = tempfile::tempdir().unwrap();
        let b = tempfile::tempdir().unwrap();
        let first = CredentialVault::new(a.path()).unwrap();
        let second = CredentialVault::new(b.path()).unwrap();
        let deepseek = facts("deepseek", "deepseek-api-key-", true);
        let unset = first.status_model_reference(&deepseek, None).unwrap();
        assert!(!unset.configured && unset.reference.is_empty());
        let reference = first
            .status_value(
                HarnessProvider::Dsh,
                &format!("deepseek-api-key-{}", id()),
                false,
            )
            .reference;
        assert!(first.reference_account(&reference).is_ok());
        assert!(second.reference_account(&reference).is_err());
        assert!(first.reference_account("/tmp/secret").is_err());
        assert!(
            first
                .reference_account(&format!("{}:deepseek-official-api-key", first.service))
                .is_err()
        );
    }
    #[test]
    fn provider_accounts_cannot_be_reused_across_providers_or_datasets() {
        let a = tempfile::tempdir().unwrap();
        let b = tempfile::tempdir().unwrap();
        let first = CredentialVault::new(a.path()).unwrap();
        let second = CredentialVault::new(b.path()).unwrap();
        for (provider, account) in KEYED {
            let p = facts(provider, account, true);
            let account = format!("{account}{}", id());
            let reference = format!("{}:{account}", first.service);
            assert!(first.validate_model_reference(&p, &reference).is_ok());
            assert!(second.validate_model_reference(&p, &reference).is_err());
            for (other, other_account) in KEYED {
                if other != provider {
                    assert!(
                        first
                            .validate_model_reference(&facts(other, other_account, true), &reference)
                            .is_err()
                    );
                }
            }
        }
        let ollama = facts("ollama", "", false);
        let status = first.status_model_reference(&ollama, None).unwrap();
        assert!(status.available && status.configured && status.reference.is_empty());
        assert_eq!(status.auth_kind, "none");
        assert!(first.configure_model(&ollama, "FAKE_TEST_VALUE").is_err());
    }
}

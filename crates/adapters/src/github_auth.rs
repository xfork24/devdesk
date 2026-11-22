use super::*;
use crate::process::Output;

#[derive(Serialize, serde::Deserialize)]
struct SavedCredential {
    token: String,
    revision: Id,
}

#[derive(Serialize)]
pub struct GitHubAuthStatus {
    revision: Id,
    source: &'static str,
    status: &'static str,
    login: Option<String>,
    message: Option<String>,
}

fn prepare_command(command: &mut Command) {
    command
        .env_remove("GH_DEBUG")
        .env("GH_PROMPT_DISABLED", "1");
}

fn status_code(output: &Output) -> Option<u64> {
    let body = serde_json::from_slice::<Value>(&output.stdout).ok()?;
    body["status"]
        .as_u64()
        .or_else(|| body["status"].as_str()?.parse().ok())
}

pub(super) fn api_error(output: &Output, message: &str) -> Error {
    let stderr = String::from_utf8_lossy(&output.stderr);
    if status_code(output) == Some(401) || stderr.contains("(HTTP 401)") {
        return err(
            "github_auth_invalid",
            "GitHub Token 无效或已过期，请在设置的 GitHub 连接中更新",
        );
    }
    if stderr.contains("gh auth login") || stderr.contains("GH_TOKEN environment variable") {
        return err(
            "github_auth_required",
            "请在设置的 GitHub 连接中配置 Token，或登录宿主机上的 GitHub CLI",
        );
    }
    err("github_unavailable", message)
}

impl GitHubPullRequests {
    pub fn new(root: &Path) -> Result<Self> {
        Ok(Self {
            vault: Some(credentials::CredentialVault::github(root)?),
            credential_update: Mutex::new(()),
        })
    }

    fn saved_credential(&self) -> Result<Option<SavedCredential>> {
        let Some(vault) = &self.vault else {
            return Ok(None);
        };
        vault
            .read_github()?
            .map(|mut bytes| {
                let credential = serde_json::from_slice(&bytes)
                    .map_err(|_| err("credential_store", "GitHub 凭据记录无法读取"));
                bytes.fill(0);
                credential
            })
            .transpose()
    }

    pub(crate) fn apply_git_auth(&self, command: &mut Command) -> Result<()> {
        prepare_command(command);
        if let Some(credential) = self.saved_credential()? {
            command.env("GH_TOKEN", credential.token);
        }
        Ok(())
    }

    fn github_api() -> Command {
        let mut command = Command::new("gh");
        command.args(["api", "--hostname", "github.com"]);
        prepare_command(&mut command);
        command
    }

    pub(crate) fn api_command(&self) -> Result<Command> {
        let mut command = Self::github_api();
        if let Some(credential) = self.saved_credential()? {
            command.env("GH_TOKEN", credential.token);
        }
        Ok(command)
    }

    fn verify_login(token: Option<&str>) -> Result<String> {
        let mut command = Self::github_api();
        command.args(["user", "--jq", ".login"]);
        if let Some(token) = token {
            command.env("GH_TOKEN", token);
        }
        let output =
            capture_command_split(command, Duration::from_secs(12), None).map_err(|error| {
                match error.code.as_str() {
                    "provider_unavailable" => {
                        err("github_cli_unavailable", "宿主机无法启动 GitHub CLI（gh）")
                    }
                    _ => err("github_unavailable", "无法连接 GitHub，请检查网络后重试"),
                }
            })?;
        if output.code != 0 || output.truncated {
            return Err(api_error(
                &output,
                "无法验证 GitHub 身份，请检查 GitHub 访问权限和网络连接",
            ));
        }
        let login = String::from_utf8_lossy(&output.stdout);
        let login = login.trim();
        if login.is_empty()
            || login.len() > 100
            || !login
                .bytes()
                .all(|b| b.is_ascii_alphanumeric() || b == b'-')
        {
            return Err(err("github_unavailable", "GitHub 未返回有效的账号"));
        }
        Ok(login.into())
    }

    pub fn auth_status(&self) -> Result<GitHubAuthStatus> {
        let saved = self.saved_credential()?;
        let source = if saved.is_some() {
            "devdesk"
        } else if ["GH_TOKEN", "GITHUB_TOKEN"]
            .iter()
            .any(|key| std::env::var_os(key).is_some_and(|value| !value.is_empty()))
        {
            "environment"
        } else {
            "cli"
        };
        let revision = saved.as_ref().map_or("0", |value| &value.revision).into();
        match Self::verify_login(saved.as_ref().map(|value| value.token.as_str())) {
            Ok(login) => Ok(GitHubAuthStatus {
                revision,
                source,
                status: "connected",
                login: Some(login),
                message: None,
            }),
            Err(error) => Ok(GitHubAuthStatus {
                revision,
                source,
                status: match error.code.as_str() {
                    "github_auth_required" => "unconfigured",
                    "github_auth_invalid" => "invalid",
                    _ => "unavailable",
                },
                login: None,
                message: Some(error.message),
            }),
        }
    }

    pub fn configure_auth(&self, expected: &str, token: &str) -> Result<GitHubAuthStatus> {
        let token = token.trim();
        if !(8..=4096).contains(&token.len()) || !token.bytes().all(|b| b.is_ascii_graphic()) {
            return Err(err("invalid_params", "GitHub Token 格式无效"));
        }
        let vault = self
            .vault
            .as_ref()
            .ok_or_else(|| err("credential_store_unavailable", "GitHub 凭据库未配置"))?;
        let login = Self::verify_login(Some(token))?;
        let _update = self.credential_update.lock().map_err(storage)?;
        let current = self.saved_credential()?;
        if current.as_ref().map_or("0", |value| &value.revision) != expected {
            return Err(err(
                "revision_conflict",
                "GitHub 凭据版本已变化，请检查连接后重试",
            ));
        }
        let revision = id();
        let record = SavedCredential {
            token: token.into(),
            revision: revision.clone(),
        };
        vault.write_github(&serde_json::to_string(&record).map_err(storage)?)?;
        Ok(GitHubAuthStatus {
            revision,
            source: "devdesk",
            status: "connected",
            login: Some(login),
            message: None,
        })
    }
}

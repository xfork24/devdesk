//! OAuth authorization-code + PKCE lifecycle. Secret material stays in the system vault.
use super::*;
use base64::{Engine, engine::general_purpose::URL_SAFE_NO_PAD};
use std::collections::BTreeSet;

pub trait IntegrationSecrets: Send + Sync {
    fn create(&self, secret: &str) -> Result<String>;
    fn read(&self, reference: &str) -> Result<String>;
    fn write(&self, reference: &str, secret: &str) -> Result<()>;
}
impl IntegrationSecrets for credentials::CredentialVault {
    fn create(&self, s: &str) -> Result<String> {
        self.configure_integration(s)
    }
    fn read(&self, r: &str) -> Result<String> {
        self.read_integration(r)
    }
    fn write(&self, r: &str, s: &str) -> Result<()> {
        self.write_integration(r, s)
    }
}
#[derive(Clone, Serialize, serde::Deserialize)]
pub struct OAuthFlow {
    pub id: String,
    pub binding_ref: String,
    pub project_id: Option<String>,
    pub config: OAuthConfig,
    pub redirect_uri: String,
    pub account: String,
    pub scopes: Vec<String>,
    pub verifier_ref: String,
    pub expires_at: u64,
    pub status: String,
    pub authorization_url: String,
    pub credential_ref: Option<String>,
    pub code_hash: Option<String>,
}
fn now() -> u64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap()
        .as_millis() as u64
}
fn directory(root: &Path) -> Result<PathBuf> {
    let path = root.join("integration-oauth");
    fs::create_dir_all(&path).map_err(storage)?;
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        fs::set_permissions(&path, fs::Permissions::from_mode(0o700)).map_err(storage)?;
    }
    Ok(path)
}
fn path(root: &Path, id: &str) -> Result<PathBuf> {
    require(uuid::Uuid::parse_str(id).is_ok(), "无效 OAuth state")?;
    Ok(directory(root)?.join(format!("{id}.json")))
}
fn save(root: &Path, flow: &OAuthFlow) -> Result<()> {
    let dest = path(root, &flow.id)?;
    let temp = dest.with_extension("tmp");
    let mut options = OpenOptions::new();
    options.create(true).truncate(true).write(true);
    #[cfg(unix)]
    {
        use std::os::unix::fs::OpenOptionsExt;
        options.mode(0o600);
    }
    use std::io::Write;
    let mut f = options.open(&temp).map_err(storage)?;
    f.write_all(&serde_json::to_vec(flow).map_err(storage)?)
        .map_err(storage)?;
    f.sync_all().map_err(storage)?;
    fs::rename(temp, dest).map_err(storage)
}
pub fn read_flow(root: &Path, id: &str) -> Result<OAuthFlow> {
    serde_json::from_slice(
        &fs::read(path(root, id)?).map_err(|_| err("oauth_state", "OAuth 流程不存在"))?,
    )
    .map_err(storage)
}
pub fn start(
    root: &Path,
    config: &OAuthConfig,
    binding_ref: &str,
    project_id: Option<String>,
    redirect: &str,
    account: &str,
    scopes: Vec<String>,
    vault: &dyn IntegrationSecrets,
) -> Result<OAuthFlow> {
    require(
        !config.client_id.is_empty() && !account.trim().is_empty(),
        "OAuth 需要 client_id 与账户标签",
    )?;
    require(
        config.authorization_url.starts_with("https://")
            || config.authorization_url.starts_with("http://127.0.0.1:")
            || config.authorization_url.starts_with("http://localhost:"),
        "授权地址需要 HTTPS 或本机测试服务",
    )?;
    require(
        redirect.starts_with("http://127.0.0.1:")
            && redirect.ends_with("/oauth/integrations/callback")
            && !redirect.contains(['?', '#', '@', '\n', '\r']),
        "仅允许本机固定 OAuth callback",
    )?;
    require(
        scopes.iter().all(|s| config.scopes.contains(s))
            && scopes.iter().collect::<BTreeSet<_>>().len() == scopes.len(),
        "OAuth scope 超出来源声明或重复",
    )?;
    let random = [
        uuid::Uuid::new_v4().as_bytes().as_slice(),
        uuid::Uuid::new_v4().as_bytes().as_slice(),
    ]
    .concat();
    let verifier = URL_SAFE_NO_PAD.encode(random);
    let challenge = URL_SAFE_NO_PAD.encode(Sha256::digest(verifier.as_bytes()));
    let state = id();
    let verifier_ref = vault.create(&json!({"code_verifier":verifier}).to_string())?;
    let query = [
        ("response_type", "code".to_owned()),
        ("client_id", config.client_id.clone()),
        ("redirect_uri", redirect.into()),
        ("scope", scopes.join(" ")),
        ("state", state.clone()),
        ("code_challenge", challenge),
        ("code_challenge_method", "S256".into()),
    ];
    fn encode(s: &str) -> String {
        s.bytes()
            .map(|b| {
                if b.is_ascii_alphanumeric() || b"-._~".contains(&b) {
                    (b as char).to_string()
                } else {
                    format!("%{b:02X}")
                }
            })
            .collect()
    }
    let url = format!(
        "{}{}{}",
        config.authorization_url,
        if config.authorization_url.contains('?') {
            "&"
        } else {
            "?"
        },
        query
            .iter()
            .map(|(k, v)| format!("{k}={}", encode(v)))
            .collect::<Vec<_>>()
            .join("&")
    );
    let flow = OAuthFlow {
        id: state,
        binding_ref: binding_ref.into(),
        project_id,
        config: config.clone(),
        redirect_uri: redirect.into(),
        account: account.into(),
        scopes,
        verifier_ref,
        expires_at: now() + 600_000,
        status: "pending".into(),
        authorization_url: url,
        credential_ref: None,
        code_hash: None,
    };
    save(root, &flow)?;
    Ok(flow)
}
fn tokens(
    value: &Value,
    config: &OAuthConfig,
    account: &str,
    scopes: &[String],
    previous: Option<&Value>,
) -> Result<Value> {
    require(
        value["access_token"]
            .as_str()
            .is_some_and(|s| !s.is_empty() && s.len() <= 32768)
            && value["token_type"]
                .as_str()
                .unwrap_or("Bearer")
                .eq_ignore_ascii_case("bearer"),
        "OAuth 返回了不支持的 token",
    )?;
    let granted: Vec<String> = value["scope"]
        .as_str()
        .map(|s| s.split_whitespace().map(str::to_owned).collect())
        .unwrap_or_else(|| scopes.to_vec());
    require(
        granted.iter().all(|s| scopes.contains(s)),
        "授权服务器扩大了冻结的 scope",
    )?;
    let expires = value["expires_in"]
        .as_u64()
        .map(|s| now() + s.min(63_072_000) * 1000);
    let refresh = value
        .get("refresh_token")
        .filter(|s| s.is_string())
        .cloned()
        .or_else(|| previous.and_then(|p| p.get("refresh_token").cloned()))
        .unwrap_or(Value::Null);
    Ok(
        json!({"grant_id":previous.and_then(|p|p.get("grant_id")).cloned().unwrap_or(json!(id())),"client_id":config.client_id,"token_url":config.token_url,"account":account,"scopes":granted,"access_token":value["access_token"],"refresh_token":refresh,"expires_at":expires,"revoked":false}),
    )
}
pub fn finish(
    root: &Path,
    state: &str,
    code: &str,
    vault: &dyn IntegrationSecrets,
    exchange: impl Fn(&str, Value) -> Result<Value>,
) -> Result<OAuthFlow> {
    require(!code.is_empty() && code.len() <= 8192, "OAuth code 无效")?;
    let mut flow = read_flow(root, state)?;
    let hash = configuration_hash(&json!(code));
    if flow.status == "complete" {
        require(
            flow.code_hash.as_ref() == Some(&hash),
            "OAuth state 已被不同 code 使用",
        )?;
        return Ok(flow);
    }
    require(
        flow.status == "pending" && flow.expires_at > now(),
        "OAuth state 已过期或已使用，请重新授权",
    )?;
    let claim = path(root, state)?.with_extension("claim");
    OpenOptions::new()
        .write(true)
        .create_new(true)
        .open(claim)
        .map_err(|_| err("oauth_state", "OAuth code 已在交换或需要重新授权"))?;
    flow.status = "exchanging".into();
    flow.code_hash = Some(hash);
    save(root, &flow)?;
    let secret: Value = serde_json::from_str(&vault.read(&flow.verifier_ref)?)
        .map_err(|_| err("oauth_state", "PKCE 状态不可用"))?;
    let result = exchange(
        &flow.config.token_url,
        json!({"grant_type":"authorization_code","client_id":flow.config.client_id,"code":code,"redirect_uri":flow.redirect_uri,"code_verifier":secret["code_verifier"]}),
    );
    let value = match result
        .and_then(|value| tokens(&value, &flow.config, &flow.account, &flow.scopes, None))
    {
        Ok(v) => v,
        Err(e) => {
            flow.status = "failed".into();
            save(root, &flow)?;
            return Err(e);
        }
    };
    flow.credential_ref = Some(vault.create(&value.to_string())?);
    flow.status = "complete".into();
    save(root, &flow)?;
    vault.write(&flow.verifier_ref, &json!({"used":true}).to_string())?;
    Ok(flow)
}
fn grant_lock(reference: &str) -> Result<std::sync::Arc<Mutex<()>>> {
    static LOCKS: std::sync::OnceLock<
        Mutex<std::collections::HashMap<String, std::sync::Arc<Mutex<()>>>>,
    > = std::sync::OnceLock::new();
    let lock = LOCKS
        .get_or_init(Default::default)
        .lock()
        .map_err(storage)?
        .entry(reference.into())
        .or_default()
        .clone();
    Ok(lock)
}

pub fn access_token(
    reference: &str,
    config: &OAuthConfig,
    account: &str,
    scopes: &[String],
    vault: &dyn IntegrationSecrets,
    exchange: impl Fn(&str, Value) -> Result<Value>,
) -> Result<String> {
    let lock = grant_lock(reference)?;
    let _guard = lock.lock().map_err(storage)?;
    let previous: Value = serde_json::from_str(&vault.read(reference)?)
        .map_err(|_| err("oauth_credential", "授权记录不可用"))?;
    require(
        previous["revoked"] != true
            && previous["refresh_pending"] != true
            && previous["client_id"] == config.client_id
            && previous["token_url"] == config.token_url
            && previous["account"] == account,
        "授权账户、来源或客户端与冻结绑定不符",
    )?;
    let allowed: Vec<String> =
        serde_json::from_value(previous["scopes"].clone()).map_err(storage)?;
    require(
        scopes.iter().all(|s| allowed.contains(s)),
        "绑定请求了未授权 scope",
    )?;
    if previous["expires_at"]
        .as_u64()
        .is_none_or(|v| v > now() + 30_000)
    {
        return previous["access_token"]
            .as_str()
            .map(str::to_owned)
            .ok_or_else(|| err("oauth_credential", "缺少 access token"));
    }
    let refresh = previous["refresh_token"]
        .as_str()
        .filter(|s| !s.is_empty())
        .ok_or_else(|| err("oauth_reauthorization_required", "授权已过期且无刷新凭据"))?;
    let mut pending = previous.clone();
    pending["refresh_pending"] = json!(true);
    vault.write(reference, &pending.to_string())?;
    let value = exchange(
        &config.token_url,
        json!({"grant_type":"refresh_token","client_id":config.client_id,"refresh_token":refresh}),
    )?;
    let next = tokens(&value, config, account, &allowed, Some(&previous))?;
    let granted: Vec<String> = serde_json::from_value(next["scopes"].clone()).map_err(storage)?;
    require(
        scopes.iter().all(|s| granted.contains(s)),
        "刷新后的授权缩小，需要重新授权以满足冻结绑定",
    )?;
    vault.write(reference, &next.to_string())?;
    Ok(next["access_token"].as_str().unwrap().into())
}
pub fn revoke(
    reference: &str,
    config: &OAuthConfig,
    vault: &dyn IntegrationSecrets,
    exchange: impl Fn(&str, Value) -> Result<Value>,
) -> Result<Value> {
    let lock = grant_lock(reference)?;
    let _guard = lock.lock().map_err(storage)?;
    let previous: Value = serde_json::from_str(&vault.read(reference)?)
        .map_err(|_| err("oauth_credential", "授权记录不可用"))?;
    vault.write(
        reference,
        &json!({"revoked":true,"grant_id":previous["grant_id"]}).to_string(),
    )?;
    let remote = if let Some(url) = &config.revocation_url {
        let token = previous["refresh_token"]
            .as_str()
            .or(previous["access_token"].as_str())
            .unwrap_or("");
        if exchange(url, json!({"token":token,"client_id":config.client_id})).is_ok() {
            "revoked"
        } else {
            "needs_reconciliation"
        }
    } else {
        "unsupported"
    };
    Ok(json!({"local_revoked":true,"remote_revocation":remote}))
}

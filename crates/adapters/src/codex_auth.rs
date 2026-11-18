//! Official Codex browser OAuth, isolated from the user's desktop Codex configuration.
//! This host-side process handles account methods only; engineering tools run in Docker workers.
use super::*;
use crate::jsonl::JsonlProcess;
use std::process::Command as ProcessCommand;

struct AuthWorker {
    installation: Id,
    process: JsonlProcess,
    pending: bool,
    login: Option<Value>,
    restart_required: bool,
}
impl AuthWorker {
    fn reconcile_login(&mut self) {
        if self
            .process
            .login_completed
            .as_ref()
            .is_some_and(|(id, _)| {
                self.login.as_ref().and_then(|v| v["loginId"].as_str()) == Some(id.as_str())
            })
        {
            self.pending = false;
            self.login = None;
        }
    }
    fn poll_login(&mut self) {
        self.reconcile_login();
        if !self.pending {
            return;
        }
        if let Ok(frames) = self.process.drain() {
            for frame in frames {
                if frame["method"] == "account/login/completed"
                    && let (Some(id), Some(success)) = (
                        frame["params"]["loginId"].as_str(),
                        frame["params"]["success"].as_bool(),
                    )
                {
                    self.process.login_completed = Some((id.into(), success));
                    self.reconcile_login();
                }
            }
        }
    }
    fn start_login(&mut self, device: bool, timeout: std::time::Duration) -> Result<Value> {
        self.poll_login();
        // A page reload or a lost HTTP response must recover the same login,
        // rather than dispatch another request or leave the user blocked.
        if self.pending {
            return self
                .login
                .clone()
                .ok_or_else(|| err("needs_reconciliation", "登录状态未知，请重新开始认证"));
        }
        self.process.login_completed = None;
        match self.process.request_for(
            "account/login/start",
            json!({"type":if device {"chatgptDeviceCode"} else {"chatgpt"}}),
            timeout,
        ) {
            Ok(value) => {
                self.pending = true;
                self.login = Some(value.clone());
                Ok(value)
            }
            Err(error) => {
                self.pending = false;
                self.login = None;
                // The native request may have been consumed before timing out.
                // Stop this authentication process before a later explicit retry.
                self.restart_required = true;
                Err(error)
            }
        }
    }
}
pub struct CodexAuthentication {
    home: PathBuf,
    worker: Mutex<Option<AuthWorker>>,
}

impl CodexAuthentication {
    /// Read-only identity probe: no program, keychain access, or account archival.
    /// Token refresh does not change an account's catalog identity.
    pub fn catalog_account_identity(&self) -> Result<Option<String>> {
        let path = self.home.join("auth.json");
        reject_symlink(&path)?;
        let bytes = match fs::read(path) {
            Ok(bytes) => bytes,
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => return Ok(None),
            Err(e) => return Err(storage(e)),
        };
        let value: Value = serde_json::from_slice(&bytes).map_err(storage)?;
        let Some(account) = value["tokens"]["account_id"].as_str() else {
            return Ok(None);
        };
        let owner = value["tokens"]["login_owner_id"]
            .as_str()
            .unwrap_or(account);
        configuration_digest(&json!([self.home, account, owner])).map(Some)
    }

    /// A stable account/login-owner reference, independent of token refresh values.
    /// Only opaque references reach configuration records; tokens stay in private native files.
    pub fn credential_reference(&self) -> Result<String> {
        let _guard = self.worker.lock().map_err(storage)?;
        let bytes = fs::read(self.home.join("auth.json"))
            .map_err(|_| err("authentication_required", "请先完成 Codex 官方网页登录"))?;
        let value: Value = serde_json::from_slice(&bytes)
            .map_err(|_| err("credential_store", "原生 OAuth 凭据格式无效"))?;
        let account = value["tokens"]["account_id"]
            .as_str()
            .ok_or_else(|| err("authentication_required", "需要 Codex ChatGPT OAuth 凭据"))?;
        let owner = value["tokens"]["login_owner_id"]
            .as_str()
            .unwrap_or(account);
        let identity = configuration_digest(&json!([self.home, account, owner]))?;
        let directory = self
            .home
            .parent()
            .unwrap()
            .join("codex-accounts")
            .join(&identity);
        reject_symlink(directory.parent().unwrap())?;
        reject_symlink(&directory)?;
        fs::create_dir_all(&directory).map_err(storage)?;
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            for path in [directory.parent().unwrap(), directory.as_path()] {
                fs::set_permissions(path, fs::Permissions::from_mode(0o700)).map_err(storage)?;
            }
        }
        let path = directory.join("auth.json");
        reject_symlink(&path)?;
        let pending = directory.join("auth.pending");
        reject_symlink(&pending)?;
        let mut options = OpenOptions::new();
        options.create(true).truncate(true).write(true);
        #[cfg(unix)]
        {
            use std::os::unix::fs::OpenOptionsExt;
            options.mode(0o600);
        }
        {
            use std::io::Write;
            let mut file = options.open(&pending).map_err(storage)?;
            file.write_all(&bytes).map_err(storage)?;
            file.sync_all().map_err(storage)?;
        }
        fs::rename(pending, path).map_err(storage)?;
        Ok(format!("codex:{identity}"))
    }
    pub fn models(&self, store: &GlobalStore, installation: &str) -> Result<Value> {
        self.with_worker(store, installation, |worker| {
            let mut models = vec![];
            let mut cursor = None::<String>;
            let mut seen = std::collections::BTreeSet::new();
            for _ in 0..100 {
                let directory = worker
                    .process
                    .request("model/list", json!({"limit":100,"cursor":cursor}))?;
                let page = directory["data"]
                    .as_array()
                    .ok_or_else(|| err("invalid_response", "Codex 返回了无效模型目录"))?;
                models.extend(page.iter().cloned());
                let next = directory["nextCursor"].as_str().filter(|s| !s.is_empty());
                let Some(next) = next else {
                    return Ok(json!({"models":models,"next_cursor":null}));
                };
                require(
                    seen.insert(next.to_owned()),
                    "Codex 模型目录分页重复，请重试",
                )?;
                cursor = Some(next.to_owned());
            }
            Err(err("invalid_response", "Codex 模型目录超过分页上限"))
        })
    }
    pub fn new(dataset: &Path) -> Result<Self> {
        let home = dataset.join("credentials/codex");
        for path in [dataset.join("credentials"), home.clone()] {
            reject_symlink(&path)?;
            fs::create_dir_all(&path).map_err(storage)?;
            #[cfg(unix)]
            {
                use std::os::unix::fs::PermissionsExt;
                fs::set_permissions(&path, fs::Permissions::from_mode(0o700)).map_err(storage)?;
            }
        }
        Ok(Self {
            home: fs::canonicalize(home).map_err(storage)?,
            worker: Mutex::new(None),
        })
    }
    fn with_worker(
        &self,
        store: &GlobalStore,
        installation: &str,
        action: impl FnOnce(&mut AuthWorker) -> Result<Value>,
    ) -> Result<Value> {
        self.with_worker_progress(store, installation, &|_| {}, action)
    }
    fn with_worker_progress(
        &self,
        store: &GlobalStore,
        installation: &str,
        progress: &dyn Fn(&str),
        action: impl FnOnce(&mut AuthWorker) -> Result<Value>,
    ) -> Result<Value> {
        let runtime = store
            .runtime_installations()?
            .into_iter()
            .find(|r| r.id == installation)
            .ok_or_else(|| err("not_found", "请先登记 Codex 本机安装"))?;
        require(
            runtime.key.provider == HarnessProvider::Codex
                && runtime.key.os == std::env::consts::OS
                && runtime.key.arch == std::env::consts::ARCH,
            "认证需要匹配本机平台的 Codex 安装",
        )?;
        require(
            store.runtime_integrity(installation)?.verified,
            "Codex 安装完整性未通过",
        )?;
        let RuntimeArtifact::Executable { canonical_path, .. } = runtime.artifact else {
            return Err(err("blocked", "本机认证需要原生安装"));
        };
        let mut worker = self.worker.lock().map_err(storage)?;
        if worker
            .as_ref()
            .is_some_and(|w| w.installation != installation)
        {
            require(
                !worker.as_ref().unwrap().pending,
                "原安装仍有登录流程，请先完成或取消",
            )?;
            *worker = None;
        }
        if worker.is_none() {
            let mut command = ProcessCommand::new(canonical_path);
            command
                .args([
                    "app-server",
                    "-c",
                    "cli_auth_credentials_store=\"file\"",
                    "-c",
                    "features.plugins=false",
                    "-c",
                    "features.apps=false",
                ])
                .env("CODEX_HOME", &self.home)
                .current_dir(&self.home);
            progress("启动本地认证进程");
            let mut process = JsonlProcess::spawn(command)?;
            progress("初始化登录协议");
            process.initialize_codex()?;
            *worker = Some(AuthWorker {
                installation: installation.into(),
                process,
                pending: false,
                login: None,
                restart_required: false,
            });
        }
        Self::auth_action(&mut worker, action)
    }
    fn auth_action(
        worker: &mut Option<AuthWorker>,
        action: impl FnOnce(&mut AuthWorker) -> Result<Value>,
    ) -> Result<Value> {
        let result = action(worker.as_mut().unwrap());
        if worker.as_ref().is_some_and(|w| w.restart_required) {
            // Drop reaps only the owned authentication subprocess. Engineering
            // sessions and the user's desktop Codex processes are independent.
            *worker = None;
        }
        result
    }
    pub fn status(&self, store: &GlobalStore, installation: &str) -> Result<Value> {
        self.with_worker(store, installation, |worker| {
            let response = worker
                .process
                .request("account/read", json!({"refreshToken":false}))?;
            let authenticated = response["account"].is_object();
            worker.reconcile_login();
            if authenticated {
                worker.pending = false;
                worker.login = None;
            }
            Ok(json!({"authenticated":authenticated,"type":response["account"]["type"],"pending":worker.pending}))
        })
    }
    pub fn start(&self, store: &GlobalStore, installation: &str, device: bool) -> Result<Value> {
        self.start_observed(store, installation, device, &|_| {})
    }
    pub fn start_observed(
        &self,
        store: &GlobalStore,
        installation: &str,
        device: bool,
        progress: &dyn Fn(&str),
    ) -> Result<Value> {
        self.with_worker_progress(store, installation, progress, |worker| {
            progress("请求官方授权地址");
            worker.start_login(device, std::time::Duration::from_secs(20))
        })
    }
    pub fn local_status(&self) -> Value {
        // Presence is local state, not a claim that the provider accepted the token.
        let configured = fs::read(self.home.join("auth.json"))
            .ok()
            .and_then(|b| serde_json::from_slice::<Value>(&b).ok())
            .is_some_and(|v| v["tokens"]["account_id"].is_string());
        let pending_login = self.worker.try_lock().ok().map(|mut worker| {
            if let Some(worker) = worker.as_mut() {
                worker.poll_login();
            }
            worker
                .as_ref()
                .filter(|worker| worker.pending)
                .and_then(|worker| worker.login.clone())
        });
        // A busy worker is unknown, not proof that an in-flight login ended.
        let pending = pending_login.as_ref().map(|login| login.is_some());
        json!({"authenticated":configured,"verification":"local credential presence","pending":pending,"pending_login":pending_login.flatten()})
    }
    pub fn cancel(&self, store: &GlobalStore, installation: &str, login: &str) -> Result<Value> {
        self.with_worker(store, installation, |worker| {
            require(
                worker.login.as_ref().and_then(|v| v["loginId"].as_str()) == Some(login),
                "登录请求不属于当前认证流程",
            )?;
            let response = worker
                .process
                .request("account/login/cancel", json!({"loginId":login}))?;
            worker.pending = false;
            worker.login = None;
            Ok(response)
        })
    }
}

#[cfg(all(test, unix))]
mod tests {
    use super::*;
    use std::time::Duration;

    fn fixture(mode: &str, receipt: &Path) -> AuthWorker {
        let mut command = ProcessCommand::new("python3");
        command.args(["-u", "-c", r#"
import json,os,sys,time
from pathlib import Path
receipt=Path(sys.argv[2]);count=0
for line in sys.stdin:
    frame=json.loads(line);count+=1
    receipt.write_text(json.dumps({'pid':os.getpid(),'requests':count}))
    if sys.argv[1]=='exit':sys.exit(0)
    if sys.argv[1]=='timeout':time.sleep(60)
    print(json.dumps({'id':frame['id'],'result':{'type':'chatgpt','loginId':'fixture-login','authUrl':'https://auth.openai.com/fixture'}}),flush=True)
    if sys.argv[1] in ('complete','fail'):
        while not receipt.with_suffix('.complete').exists():time.sleep(0.01)
        print(json.dumps({'method':'account/login/completed','params':{'loginId':'fixture-login','success':sys.argv[1]=='complete'}}),flush=True)
"#, mode]);
        command.arg(receipt);
        AuthWorker {
            installation: "synthetic-auth-transport".into(),
            process: JsonlProcess::spawn(command).unwrap(),
            pending: false,
            login: None,
            restart_required: false,
        }
    }

    #[test]
    fn failed_login_reaps_auth_worker_and_allows_a_fresh_explicit_attempt() {
        let directory = tempfile::tempdir().unwrap();
        for mode in ["timeout", "exit"] {
            let receipt = directory.path().join(mode);
            let mut worker = Some(fixture(mode, &receipt));
            let result = CodexAuthentication::auth_action(&mut worker, |worker| {
                worker.start_login(false, Duration::from_millis(200))
            });
            assert_eq!(result.unwrap_err().code, "needs_reconciliation");
            assert!(worker.is_none(), "failed pending state must not survive");
            let value: Value = serde_json::from_slice(&fs::read(&receipt).unwrap()).unwrap();
            let pid = value["pid"].as_i64().unwrap() as libc::pid_t;
            assert_eq!(unsafe { libc::kill(pid, 0) }, -1);
            assert_eq!(
                std::io::Error::last_os_error().raw_os_error(),
                Some(libc::ESRCH)
            );

            // This is a new explicit action, after the old listener was reaped.
            worker = Some(fixture("reply", &directory.path().join("retry")));
            let value = CodexAuthentication::auth_action(&mut worker, |worker| {
                worker.start_login(false, Duration::from_secs(2))
            })
            .unwrap();
            assert_eq!(value["loginId"], "fixture-login");
            assert!(worker.as_ref().unwrap().pending);
        }
    }

    #[test]
    fn repeated_start_recovers_the_same_pending_login_without_native_redispatch() {
        let directory = tempfile::tempdir().unwrap();
        let receipt = directory.path().join("requests");
        let mut worker = Some(fixture("reply", &receipt));
        let first = CodexAuthentication::auth_action(&mut worker, |worker| {
            worker.start_login(false, Duration::from_secs(2))
        })
        .unwrap();
        let recovered = CodexAuthentication::auth_action(&mut worker, |worker| {
            worker.start_login(false, Duration::from_secs(2))
        })
        .unwrap();
        assert_eq!(first, recovered);
        let value: Value = serde_json::from_slice(&fs::read(&receipt).unwrap()).unwrap();
        assert_eq!(value["requests"], 1);
    }

    #[test]
    fn an_explicit_login_after_completion_dispatches_a_fresh_request() {
        let directory = tempfile::tempdir().unwrap();
        let receipt = directory.path().join("requests");
        let mut worker = fixture("reply", &receipt);
        let login = worker.start_login(false, Duration::from_secs(2)).unwrap();
        worker.process.login_completed = Some((login["loginId"].as_str().unwrap().into(), true));
        worker.start_login(false, Duration::from_secs(2)).unwrap();
        worker.poll_login();
        assert!(worker.pending);
        let value: Value = serde_json::from_slice(&fs::read(&receipt).unwrap()).unwrap();
        assert_eq!(value["requests"], 2);
    }

    #[test]
    fn local_status_observes_login_completion_without_native_requests_or_stale_recovery() {
        let directory = tempfile::tempdir().unwrap();
        for mode in ["complete", "fail"] {
            let auth = CodexAuthentication::new(&directory.path().join(mode)).unwrap();
            let receipt = directory.path().join(format!("{mode}-requests"));
            let mut worker = fixture(mode, &receipt);
            worker.start_login(false, Duration::from_secs(2)).unwrap();
            *auth.worker.lock().unwrap() = Some(worker);
            assert_eq!(auth.local_status()["pending"], true);

            fs::write(receipt.with_extension("complete"), "complete").unwrap();
            let deadline = std::time::Instant::now() + Duration::from_secs(2);
            while auth.local_status()["pending"] != false {
                assert!(std::time::Instant::now() < deadline);
                std::thread::sleep(Duration::from_millis(10));
            }
            let status = auth.local_status();
            assert!(status["pending_login"].is_null());
            assert_eq!(status["authenticated"], false);
            let value: Value = serde_json::from_slice(&fs::read(&receipt).unwrap()).unwrap();
            assert_eq!(value["requests"], 1);
        }
    }

    #[test]
    fn local_status_preserves_a_pending_login_with_old_credentials_or_an_unrelated_completion() {
        let directory = tempfile::tempdir().unwrap();
        let auth = CodexAuthentication::new(directory.path()).unwrap();
        fs::write(
            auth.home.join("auth.json"),
            json!({"tokens":{"account_id":"previous-account"}}).to_string(),
        )
        .unwrap();
        let mut worker = fixture("reply", &directory.path().join("requests"));
        worker.start_login(false, Duration::from_secs(2)).unwrap();
        worker.process.login_completed = Some(("previous-login".into(), true));
        *auth.worker.lock().unwrap() = Some(worker);
        let status = auth.local_status();
        assert_eq!(status["authenticated"], true);
        assert_eq!(status["pending"], true);
        assert_eq!(status["pending_login"]["loginId"], "fixture-login");

        let _guard = auth.worker.lock().unwrap();
        assert!(auth.local_status()["pending"].is_null());
    }
}

#[cfg(test)]
mod catalog_identity_tests {
    use super::*;
    #[test]
    fn catalog_probe_is_read_only_and_account_bound_not_token_bound() {
        let dir = tempfile::tempdir().unwrap();
        let auth = CodexAuthentication::new(dir.path()).unwrap();
        assert!(auth.catalog_account_identity().unwrap().is_none());
        let write = |account: &str, token: &str| {
            fs::write(
                auth.home.join("auth.json"),
                json!({"tokens":{"account_id":account,"access_token":token}}).to_string(),
            )
            .unwrap()
        };
        write("one", "first");
        let one = auth.catalog_account_identity().unwrap();
        write("one", "refreshed");
        assert_eq!(one, auth.catalog_account_identity().unwrap());
        write("two", "next");
        assert_ne!(one, auth.catalog_account_identity().unwrap());
        assert!(!dir.path().join("credentials/codex-accounts").exists());
        assert!(auth.worker.lock().unwrap().is_none());
    }
}

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
    fn start_login(&mut self, device: bool, timeout: std::time::Duration) -> Result<Value> {
        // A page reload or a lost HTTP response must recover the same login,
        // rather than dispatch another request or leave the user blocked.
        if self.pending {
            return self
                .login
                .clone()
                .ok_or_else(|| err("needs_reconciliation", "登录状态未知，请重新开始认证"));
        }
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
            let directory = worker.process.request("model/list", json!({"limit":100}))?;
            Ok(json!({"models":directory["data"],"next_cursor":directory["nextCursor"]}))
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
                .args(["app-server", "-c", "cli_auth_credentials_store=\"file\""])
                .env("CODEX_HOME", &self.home)
                .current_dir(&self.home);
            let mut process = JsonlProcess::spawn(command)?;
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
        self.with_worker(store,installation,|worker| {
            let response=worker.process.request("account/read",json!({"refreshToken":false}))?;
            let authenticated=response["account"].is_object();
            if authenticated || worker.process.login_completed.as_ref().is_some_and(|(id,_)|worker.login.as_ref().and_then(|v|v["loginId"].as_str())==Some(id.as_str())) {worker.pending=false;worker.login=None;}
            Ok(json!({"authenticated":authenticated,"type":response["account"]["type"],"pending":worker.pending}))
        })
    }
    pub fn start(&self, store: &GlobalStore, installation: &str, device: bool) -> Result<Value> {
        self.with_worker(store, installation, |worker| {
            worker.start_login(device, std::time::Duration::from_secs(20))
        })
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
}

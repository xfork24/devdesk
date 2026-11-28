//! Serialize preparation per provider without holding database or project-selection locks.
use super::*;
use std::collections::BTreeMap;
use std::sync::RwLock;

#[derive(Default)]
pub struct RuntimePreparation {
    codex: Mutex<()>,
    dsh: Mutex<()>,
    pi: Mutex<()>,
    opencode: Mutex<()>,
    mcode: Mutex<()>,
    zcode: Mutex<()>,
    auth: Mutex<()>,
    auth_cancel: std::sync::atomic::AtomicBool,
    status: RwLock<BTreeMap<String, Value>>,
}
impl RuntimePreparation {
    pub fn run<T>(&self, key: &str, work: impl FnOnce(&dyn Fn(&str)) -> Result<T>) -> Result<T> {
        let _guard = match key {
            "codex" => &self.codex,
            "dsh" => &self.dsh,
            "pi" => &self.pi,
            "opencode" => &self.opencode,
            "mcode" => &self.mcode,
            "zcode" => &self.zcode,
            _ => &self.auth,
        }
        .lock()
        .map_err(|e| err("storage", e.to_string()))?;
        let started = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap_or_default()
            .as_millis() as u64;
        let progress = |message: &str| {
            self.status.write().unwrap().insert(
                key.into(),
                json!({"provider":key,"status":"preparing","message":message,"started":started}),
            );
        };
        progress("正在检查运行环境");
        let result = work(&progress);
        self.status.write().unwrap().insert(key.into(), match &result {
            Ok(_) => json!({"provider":key,"status":"ready","message":"运行环境已就绪","started":started}),
            Err(error) => json!({"provider":key,"status":"failed","message":error.message,"code":error.code,"started":started}),
        });
        result
    }
    pub fn status(&self) -> Value {
        json!({"preparations":self.status.read().unwrap().values().collect::<Vec<_>>()})
    }
}

pub struct SessionResourceAdapter<'a> {
    pub global: &'a GlobalStore,
    pub codex_auth: &'a codex_auth::CodexAuthentication,
    pub preparation: &'a RuntimePreparation,
}
impl RuntimePreparation {
    pub fn cancel_auth(&self) {
        self.auth_cancel
            .store(true, std::sync::atomic::Ordering::SeqCst);
    }
    pub fn auth_cancelled(&self) -> bool {
        self.auth_cancel.load(std::sync::atomic::Ordering::SeqCst)
    }
    pub fn auth_progress(&self, message: &str) {
        self.update_auth_status("preparing", message);
    }
    pub fn auth_login_result(&self, result: &Result<Value>) {
        match result {
            Ok(_) => self.update_auth_status("waiting", "等待用户完成官方网页授权"),
            Err(error) => self.update_auth_status("failed", &error.message),
        }
    }
    fn update_auth_status(&self, status: &str, message: &str) {
        let mut preparations = self.status.write().unwrap();
        let row = preparations.entry("codex-auth".into()).or_insert_with(|| {
            let started = std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap_or_default()
                .as_millis() as u64;
            json!({"provider":"codex-auth","started":started})
        });
        row["message"] = json!(message);
        row["status"] = json!(status);
    }
    pub fn prepare_auth(&self, global: &GlobalStore) -> Result<RuntimeInstallation> {
        self.run("codex-auth", |progress| {
            self.auth_cancel
                .store(false, std::sync::atomic::Ordering::SeqCst);
            global.prepare_codex_auth_cancellable(progress, &|| self.auth_cancelled())
        })
    }
}
impl ports::SessionResources for SessionResourceAdapter<'_> {
    fn shared_store(&self) -> &dyn EngineeringStore {
        self.global
    }
    fn architecture(&self) -> String {
        std::env::consts::ARCH.into()
    }
    fn prepare_harness(
        &self,
        record: &ConfigurationRecord,
        resource: &ConfigurationRecord,
    ) -> Result<RuntimeInstallation> {
        let harness: HarnessDefinition = record.decode()?;
        let selected = compose_execution_recipe(record, resource)?;
        require(
            selected.provider.as_str() == harness.provider,
            "执行安装的协议驱动不匹配",
        )?;
        self.preparation.run(harness.provider.as_str(), |progress| {
            self.global
                .prepare_execution_recipe(resource, &selected, progress)
        })
    }
    fn installations(&self) -> Result<Vec<RuntimeInstallation>> {
        self.global.runtime_installations()
    }
    fn ensure_runtime(
        &self,
        provider: HarnessProvider,
        preferred: Option<&str>,
    ) -> Result<RuntimeInstallation> {
        if let Some(id) = preferred {
            let runtime = self
                .global
                .runtime_installations()?
                .into_iter()
                .find(|r| {
                    r.id == id
                        && r.key.provider == provider
                        && r.key.os == "linux"
                        && matches!(r.artifact, RuntimeArtifact::DockerImage { .. })
                })
                .ok_or_else(|| err("blocked", "所选 Harness 安装不存在或平台不兼容"))?;
            require(
                self.global.runtime_integrity(id)?.verified,
                "所选运行时镜像不可用；固定安装不会自动替换",
            )?;
            return Ok(runtime);
        }
        self.preparation.run(provider.as_str(), |progress| {
            self.global.prepare_docker_runtime(provider, progress)
        })
    }
    fn runtime_verified(&self, id: &str) -> Result<bool> {
        Ok(self.global.runtime_integrity(id)?.verified)
    }
    fn installation_recipe(&self, i: &RuntimeInstallation) -> String {
        permissions::installation_recipe(i)
    }
    fn validate_installation(&self, i: &RuntimeInstallation) -> Result<()> {
        permissions::validate_runtime_version(i)?;
        if self.global.runtime_verification(i)?.is_none() {
            // A Host rebuild changes the driver digest, not the selected image.
            // Re-probe that exact installation without model inference or replacement.
            self.preparation.run(i.key.provider.as_str(), |progress| {
                progress("正在验证当前驱动与运行环境的协议");
                self.global.verify_runtime_protocol(i)
            })?;
        }
        self.global.require_runtime_verification(i)
    }
    fn verify_permissions(
        &self,
        s: &EngineeringState,
        l: &VersionedPermissionLock,
        i: &RuntimeInstallation,
    ) -> Result<()> {
        permission_profiles::verify_permission_installation(s, l, i)
    }
    fn compile_permissions(
        &self,
        s: &EngineeringState,
        l: &VersionedPermissionLock,
        scope: Option<ExecutionScope>,
    ) -> Result<ExecutionPermissions> {
        permission_profiles::compile_permission_profiles(s, l, scope)
    }
    fn adapter_identity(&self, p: HarnessProvider) -> String {
        harness::managed_adapter_identity(p)
    }
    fn credential_reference(
        &self,
        provider: HarnessProvider,
        connection: Option<&ModelConnection>,
        state: &EngineeringState,
    ) -> Result<Option<String>> {
        match provider {
            HarnessProvider::Codex => Ok(Some(self.codex_auth.credential_reference()?)),
            HarnessProvider::Dsh
            | HarnessProvider::Pi
            | HarnessProvider::Opencode
            | HarnessProvider::Mcode
            | HarnessProvider::Zcode => {
                let c = connection.ok_or_else(|| err("invalid_configuration", "缺少模型连接"))?;
                let facts = state.provider_facts_any(&c.provider)?;
                require(facts.harness == provider.as_str(), "服务商归属其他 Harness")?;
                if !facts.requires_key {
                    return Ok(None);
                }
                let vault = credentials::CredentialVault::new(Path::new(&self.global.root()))?;
                let status = vault.status_model_reference(
                    &facts,
                    self.global.model_credential_reference(&facts)?.as_deref(),
                )?;
                require(
                    status.available,
                    "系统凭据库不可用，请检查共享凭据助手或 Secret Service",
                )?;
                require(status.configured, format!("请先配置 {} 凭据", facts.label))?;
                // No worker or provider call exists during binding validation.
                vault
                    .verify_model_reference(&facts, &status.reference)
                    .map_err(|error| error.technical_not_applied())?;
                Ok(Some(status.reference))
            }
        }
    }
    fn model_profile_digest(
        &self,
        provider: HarnessProvider,
        c: &ModelConnection,
        m: &str,
    ) -> Result<String> {
        model_providers::model_profile_digest(provider, c, m)
    }
    fn runtime_digest(&self, a: &Definition, b: &RuntimeBinding) -> Result<String> {
        runtime_digest(a, b)
    }
}

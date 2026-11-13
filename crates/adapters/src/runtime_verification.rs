//! Observations are bound to the actual artifact, driver and permission assets.
//! Initialization probes make no model calls and do not certify inference or tool execution.
use super::*;
use std::{process::Command, time::Duration};
fn now() -> u64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap_or_default()
        .as_millis() as u64
}

impl GlobalStore {
    fn verification_key(&self, installation: &RuntimeInstallation) -> Result<String> {
        configuration_digest(&json!({"artifact":installation.artifact,"recipe":installation.recipe_digest,
            "driver":harness::managed_adapter_identity(installation.key.provider),"assets":permissions::asset_digests()}))
            .map(|digest| format!("runtime.verification.{digest}"))
    }
    pub fn runtime_verification(
        &self,
        installation: &RuntimeInstallation,
    ) -> Result<Option<Value>> {
        catalog::setting(
            &*self.connection.lock().map_err(storage)?,
            &self.verification_key(installation)?,
        )
    }
    pub fn require_runtime_verification(&self, installation: &RuntimeInstallation) -> Result<()> {
        require(
            self.runtime_integrity(&installation.id)?.verified,
            "实际运行产物已漂移",
        )?;
        require(
            self.runtime_verification(installation)?
                .is_some_and(|v| v["status"] == "verified"),
            "实际运行资源尚未通过当前驱动/权限资产的协议验证，请重新准备",
        )
    }
    pub fn verify_runtime_protocol(&self, installation: &RuntimeInstallation) -> Result<Value> {
        permissions::validate_runtime_version(installation)?;
        require(
            self.runtime_integrity(&installation.id)?.verified,
            "镜像不可用",
        )?;
        if let Some(report) = self.runtime_verification(installation)? {
            if report["status"] == "verified" {
                return Ok(report);
            }
        }
        let RuntimeArtifact::DockerImage { image_id } = &installation.artifact else {
            unreachable!()
        };
        let name = format!("devdesk-probe-{}", id());
        // Even a protocol error must remove the named isolated probe container.
        struct Cleanup(String);
        impl Drop for Cleanup {
            fn drop(&mut self) {
                let _ = process::capture(
                    "docker",
                    &["rm".into(), "--force".into(), self.0.clone()],
                    Duration::from_secs(10),
                );
            }
        }
        let _cleanup = Cleanup(name.clone());
        let mut command = Command::new("docker");
        command.args([
            "run",
            "--rm",
            "-i",
            "--name",
            &name,
            "--network",
            "none",
            "--read-only",
            "--cap-drop",
            "ALL",
            "--tmpfs",
            "/state:rw,mode=1777",
            "--tmpfs",
            "/tmp:rw,mode=1777",
            "--workdir",
            "/tmp",
            "--env",
            "CODEX_HOME=/state/codex",
            "--env",
            "HOME=/state",
            "--entrypoint",
            "/bin/sh",
            image_id,
        ]);
        match installation.key.provider {
            HarnessProvider::Codex => {
                command.args(["-c", "mkdir -p /state/codex && exec codex app-server"]);
            }
            HarnessProvider::Dsh => {
                command.args(["-c", "exec dsh --profile sdk"]);
            }
        }
        let mut wire = jsonl::JsonlProcess::spawn(command)?;
        let observations = match installation.key.provider {
            HarnessProvider::Codex => {
                wire.initialize_codex()?;
                let result = wire.request("thread/start",json!({"cwd":"/tmp","approvalPolicy":"never","sandbox":"read-only",
                    "dynamicTools":[{"type":"function","name":"devdesk_probe","description":"Registration probe; never invoked","inputSchema":{"type":"object","properties":{}}}]}))?;
                require(
                    result["thread"]["id"].as_str().is_some(),
                    "运行资源未返回有效 thread 身份",
                )?;
                vec!["initialize", "thread/start", "dynamic-tool-registration"]
            }
            HarnessProvider::Dsh => {
                wire.rpc2();
                wire.request("initialize",json!({"cwd":"/tmp","provider":"deepseek","model":"probe-no-inference","maxTokens":1024,
                    "approvalRequests":true,"capabilities":{"userQuestions":true}}))?;
                vec!["initialize"]
            }
        };
        drop(wire);
        let report = json!({"status":"verified","installation_id":installation.id,"artifact":installation.artifact,
            "recipe_digest":installation.recipe_digest,"driver":harness::managed_adapter_identity(installation.key.provider),
            "permission_assets":permissions::asset_digests(),"observations":observations,
            "inference_verified":false,"tool_execution_verified":false,"verified_at":now()});
        catalog::put_setting(
            &*self.connection.lock().map_err(storage)?,
            &self.verification_key(installation)?,
            &report,
        )?;
        Ok(report)
    }
    pub fn runtime_preparation_records(&self) -> Result<Vec<Value>> {
        let conn = self.connection.lock().map_err(storage)?;
        let mut statement = conn
            .prepare(
                "SELECT value FROM settings WHERE key LIKE 'runtime.preparation.%' ORDER BY key",
            )
            .map_err(storage)?;
        statement
            .query_map([], |row| row.get::<_, String>(0))
            .map_err(storage)?
            .map(|row| serde_json::from_str(&row.map_err(storage)?).map_err(storage))
            .collect()
    }
    pub fn cancel_runtime_preparation(&self, digest: &str) -> Result<Value> {
        require(
            digest.len() == 64 && digest.bytes().all(|b| b.is_ascii_hexdigit()),
            "需要完整配方摘要",
        )?;
        let conn = self.connection.lock().map_err(storage)?;
        let key = format!("runtime.preparation.{digest}");
        let mut value =
            catalog::setting(&conn, &key)?.ok_or_else(|| err("not_found", "准备意图不存在"))?;
        require(value["status"] == "preparing", "只有在途准备可以请求取消")?;
        value["cancel_requested"] = json!(true);
        catalog::put_setting(&conn, &key, &value)?;
        Ok(value)
    }
    pub(crate) fn preparation_checkpoint(&self, digest: &str) -> Result<()> {
        let conn = self.connection.lock().map_err(storage)?;
        let key = format!("runtime.preparation.{digest}");
        let value = catalog::setting(&conn, &key)?;
        require(
            !value.is_some_and(|v| v["cancel_requested"] == true),
            "运行资源准备已请求取消；不采用构建结果",
        )
    }
    pub(crate) fn preparation_intent(
        &self,
        source: &ConfigurationRecord,
        digest: &str,
        status: &str,
        result: Option<&Value>,
    ) -> Result<()> {
        let conn = self.connection.lock().map_err(storage)?;
        let key = format!("runtime.preparation.{digest}");
        if status == "preparing" && result.is_none() {
            if let Some(previous) = catalog::setting(&conn, &key)? {
                // On restart the result of the old build is unknown. A recovery request
                // inspects its content-addressed tag before deciding whether to rebuild.
                require(
                    previous["status"] != "preparing",
                    "准备结果未知；先核对原意图，不能直接重发构建",
                )?;
            }
        }
        catalog::put_setting(
            &conn,
            &key,
            &json!({"source_ref":source.id,"provider":source.data["provider"],"recipe_digest":digest,"status":status,"result":result,"updated_at":now()}),
        )
    }
    pub fn preparation_provider(&self, digest: &str) -> Result<String> {
        self.runtime_preparation_records()?
            .iter()
            .find(|r| r["recipe_digest"] == digest)
            .and_then(|r| r["provider"].as_str())
            .map(str::to_owned)
            .ok_or_else(|| err("not_found", "准备意图不存在"))
    }
    pub fn reconcile_runtime_preparation(&self, digest: &str) -> Result<Value> {
        require(
            digest.len() == 64 && digest.bytes().all(|b| b.is_ascii_hexdigit()),
            "需要完整配方摘要",
        )?;
        let conn = self.connection.lock().map_err(storage)?;
        let key = format!("runtime.preparation.{digest}");
        let mut value =
            catalog::setting(&conn, &key)?.ok_or_else(|| err("not_found", "准备意图不存在"))?;
        require(value["status"] == "preparing", "只核对未知/在途准备")?;
        // Caller is serialized with preparation by RuntimePreparation.run. The next
        // attempt first inspects the immutable tag and reuses a completed image.
        value["status"] = json!("reconciled");
        value["cancel_requested"] = json!(false);
        catalog::put_setting(&conn, &key, &value)?;
        Ok(value)
    }
}

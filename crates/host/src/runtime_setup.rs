//! Runtime ports are assembled here; policy and durable receipts are core use cases.
use super::*;
impl Workspace {
    pub(super) fn runtime_preparation_status(&self) -> Value {
        let mut status = self.runtime_setup.status();
        if let Some(auth) = status["preparations"]
            .as_array_mut()
            .and_then(|rows| rows.iter_mut().find(|row| row["provider"] == "codex-auth"))
            && auth["status"] == "waiting"
            && self.codex_auth.local_status()["pending"] == false
        {
            // Project the current login state without retaining another terminal
            // flag that could race with a later login or another browser tab.
            auth["status"] = json!("ready");
            auth["message"] = json!("官方网页授权流程已结束");
        }
        status
    }
    pub(super) fn ensure_runtime(
        &self,
        provider: HarnessProvider,
        preferred: Option<&str>,
    ) -> Result<RuntimeInstallation> {
        ports::SessionResources::ensure_runtime(&self.session_resources(), provider, preferred)
    }
    pub(super) fn ensure_codex_auth(&self) -> Result<RuntimeInstallation> {
        self.runtime_setup.prepare_auth(&self.global)
    }
    pub(super) fn materialize_harness(
        &self,
        e: &Engine,
        record: &ConfigurationRecord,
    ) -> Result<RuntimeInstallation> {
        e.materialize_harness(&self.session_resources(), record)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn preparation_queries_clear_finished_logins_but_preserve_preparation_and_failures() {
        let root = tempfile::tempdir().unwrap();
        let workspace =
            Workspace::open_with_adapters(root.path(), crate::test_runtime::adapters()).unwrap();
        for method in [
            "runtime.preparation_status",
            "codex.auth.preparation_status",
        ] {
            workspace.runtime_setup.auth_progress("请求官方授权地址");
            let status = workspace.dispatch(method, json!({})).unwrap();
            assert_eq!(status["preparations"][0]["status"], "preparing");

            workspace.runtime_setup.auth_login_result(&Ok(json!({})));
            let status = workspace.dispatch(method, json!({})).unwrap();
            assert_eq!(status["preparations"][0]["status"], "ready");
            assert_ne!(
                status["preparations"][0]["message"],
                "等待用户完成官方网页授权"
            );

            workspace
                .runtime_setup
                .auth_login_result(&Err(err("provider_failure", "获取官方授权地址失败")));
            let status = workspace.dispatch(method, json!({})).unwrap();
            assert_eq!(status["preparations"][0]["status"], "failed");
            assert_eq!(status["preparations"][0]["message"], "获取官方授权地址失败");
        }
    }
}

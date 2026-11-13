//! Runtime ports are assembled here; policy and durable receipts are core use cases.
use super::*;
impl Workspace {
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

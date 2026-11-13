use super::*;
pub struct ConfigurationBackend;
impl ports::ConfigurationBackend for ConfigurationBackend {
    fn prepare_permission_binding(&self, s: &EngineeringState, change: &Value) -> Result<Value> {
        let mut change = change.clone();
        let target: PermissionTargetRef =
            serde_json::from_value(change["target"].clone()).map_err(storage)?;
        let profile = s
            .configuration(change["permission_revision_id"].as_str().unwrap_or(""))?
            .permission_profile()?;
        permission_profiles::preflight_profile(s, &target, &profile)?;
        change["enforcement_contract"] =
            json!(permission_profiles::permission_contract(s, &target)?);
        Ok(change)
    }
}
impl ports::ProjectInventory for GlobalStore {
    fn runtime_facts(&self) -> Result<Value> {
        let installations = self.runtime_installations()?;
        let reports = installations
            .iter()
            .map(|i| self.runtime_verification(i))
            .collect::<Result<Vec<_>>>()?;
        Ok(
            json!({"installations":installations,"protocol_reports":reports,"permission_assets":permissions::asset_digests()}),
        )
    }
    fn snapshots(&self) -> Result<Vec<EngineeringState>> {
        let mut result = Vec::new();
        let mut offset = 0;
        loop {
            let (projects, total) = self.projects(offset, 100)?;
            for p in &projects {
                result.push(SqliteStore::read_project_snapshot(
                    Path::new(&self.root()),
                    &p.id,
                )?);
            }
            offset += projects.len();
            if offset >= total || projects.is_empty() {
                break;
            }
        }
        Ok(result)
    }
}

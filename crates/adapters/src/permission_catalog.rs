use super::*;
use rusqlite::OptionalExtension;
impl GlobalStore {
    pub fn permission_configuration(&self) -> Result<PermissionConfiguration> {
        let conn = self.connection.lock().map_err(storage)?;
        let text: String = conn
            .query_row(
                "SELECT value FROM settings WHERE key='permission_configuration'",
                [],
                |r| r.get(0),
            )
            .map_err(storage)?;
        serde_json::from_str(&text).map_err(storage)
    }
    pub fn bind_permissions(&self, c: &Command) -> Result<Value> {
        require(
            c.target.as_deref() == Some("permission-configuration"),
            "权限绑定目标必须是 permission-configuration",
        )?;
        require(!c.command_id.is_empty(), "command_id 不能为空")?;
        let fp = command_fingerprint("permission.bind", c);
        let definitions = self.snapshot()?.permissions;
        let mut next: PermissionConfiguration = serde_json::from_value(c.payload.clone())
            .map_err(|e| err("invalid_params", e.to_string()))?;
        require(
            next.harnesses.len() == 2
                && ["codex", "dsh"]
                    .iter()
                    .all(|k| next.harnesses.contains_key(*k)),
            "需要明确的 Codex 和 DSH 权限绑定",
        )?;
        for binding in next
            .harnesses
            .values()
            .chain(std::iter::once(&next.sandbox))
        {
            resolve_permission(&definitions, binding, &[])?;
        }
        let _writer = self.writer.lock().map_err(storage)?;
        let mut conn = self.connection.lock().map_err(storage)?;
        let tx = conn.transaction().map_err(storage)?;
        let previous: Option<(String, String)> = tx
            .query_row(
                "SELECT fingerprint,result FROM operations WHERE id=?1",
                [&c.command_id],
                |r| Ok((r.get(0)?, r.get(1)?)),
            )
            .optional()
            .map_err(storage)?;
        if let Some((saved, result)) = previous {
            require(saved == fp, "command_id 被不同参数复用")?;
            return serde_json::from_str(&result).map_err(storage);
        }
        require(
            !tx.query_row(
                "SELECT EXISTS(SELECT 1 FROM project_catalog WHERE command_id=?1)",
                [&c.command_id],
                |r| r.get::<_, bool>(0),
            )
            .map_err(storage)?,
            "command_id 已用于预留项目",
        )?;
        let old: String = tx
            .query_row(
                "SELECT value FROM settings WHERE key='permission_configuration'",
                [],
                |r| r.get(0),
            )
            .map_err(storage)?;
        let old: PermissionConfiguration = serde_json::from_str(&old).map_err(storage)?;
        if c.expected_revision != Some(old.revision) {
            return Err(err("revision_conflict", "权限配置版本已变化"));
        }
        for provider in [HarnessProvider::Codex, HarnessProvider::Dsh] {
            permissions::resolve_execution_permissions(
                &definitions,
                &next,
                provider,
                ExecutionScope {
                    work_type: WorkType::Discussion,
                    writable_paths: vec![],
                    read_only_paths: vec![],
                },
                None,
                None,
                None,
            )?;
        }
        next.revision = old.revision + 1;
        let result = json!(next);
        tx.execute(
            "UPDATE settings SET value=?1 WHERE key='permission_configuration'",
            [result.to_string()],
        )
        .map_err(storage)?;
        tx.execute(
            "INSERT INTO operations(id,fingerprint,result) VALUES(?1,?2,?3)",
            rusqlite::params![c.command_id, fp, result.to_string()],
        )
        .map_err(storage)?;
        tx.commit().map_err(storage)?;
        Ok(result)
    }
}

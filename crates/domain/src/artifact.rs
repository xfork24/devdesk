//! Exact identity and immutable bytes for a prepared local file seal.
use crate::*;
use sha2::{Digest, Sha256};

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ArtifactSeal {
    pub artifact: Artifact,
    pub bytes: u64,
}
impl ArtifactSeal {
    /// The application supplies the operation-bound identity. Domain rules do
    /// not allocate identities, inspect paths or observe wall time.
    pub fn prepare(
        id: Id,
        owner: &MaterialOwner,
        category: &str,
        body: &str,
        origin: Origin,
    ) -> Result<Self> {
        let (project_id, feature_id, task_id, folder) = match owner {
            MaterialOwner::Global => (None, None, None, "global".to_owned()),
            MaterialOwner::Project { id } => {
                (Some(id.clone()), None, None, format!("project-{id}"))
            }
            MaterialOwner::Feature { id } => (None, Some(id.clone()), None, id.clone()),
            MaterialOwner::Task { feature, id } => (
                None,
                Some(feature.clone()),
                Some(id.clone()),
                feature.clone(),
            ),
        };
        let task = task_id.as_deref().unwrap_or("feature");
        for part in [folder.as_str(), task, category, id.as_str()] {
            require(
                !part.is_empty()
                    && part.len() <= 256
                    && part
                        .bytes()
                        .all(|b| b.is_ascii_alphanumeric() || matches!(b, b'-' | b'_')),
                "非法 Artifact 身份或归属路径",
            )?;
        }
        require(body.len() <= 64 * 1024 * 1024, "单个 Artifact 超出封存预算")?;
        let json = ["snapshots", "workflow-outputs", "proposals"].contains(&category);
        let extension = if json { "json" } else { "md" };
        Ok(Self {
            artifact: Artifact {
                id: id.clone(),
                project_id,
                feature_id,
                task_id: task_id.clone(),
                relative_path: format!("artifacts/{folder}/{task}/{category}/{id}.{extension}"),
                sha256: format!("{:x}", Sha256::digest(body.as_bytes())),
                media_type: if json {
                    "application/json"
                } else {
                    "text/markdown; charset=utf-8"
                }
                .into(),
                origin,
            },
            bytes: body.len() as u64,
        })
    }
    pub fn validate_body(&self, body: &str) -> Result<()> {
        require(
            body.len() as u64 == self.bytes
                && format!("{:x}", Sha256::digest(body.as_bytes())) == self.artifact.sha256,
            "封存正文与原始内容身份不符",
        )
    }
}

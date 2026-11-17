//! Publish a preallocated local file without replacing an existing identity.
use crate::*;
use std::io::{Read, Write};

fn destination(root: &Path, seal: &ArtifactSeal) -> Result<PathBuf> {
    require(
        seal.bytes <= 64 * 1024 * 1024
            && seal.artifact.sha256.len() == 64
            && seal.artifact.sha256.bytes().all(|b| b.is_ascii_hexdigit()),
        "原 Artifact 内容身份无效",
    )?;
    let path = Path::new(&seal.artifact.relative_path);
    let parts = path
        .components()
        .map(|part| match part {
            std::path::Component::Normal(value) => value
                .to_str()
                .ok_or_else(|| err("invalid_params", "Artifact 路径编码无效")),
            _ => Err(err("invalid_params", "Artifact 路径越界")),
        })
        .collect::<Result<Vec<_>>>()?;
    require(
        parts.len() == 5
            && parts[0] == "artifacts"
            && parts.iter().all(|part| {
                !part.is_empty()
                    && part.len() <= 300
                    && part
                        .bytes()
                        .all(|b| b.is_ascii_alphanumeric() || matches!(b, b'-' | b'_' | b'.'))
            }),
        "Artifact 路径不属于封存目录",
    )?;
    let owner = material_owner(
        seal.artifact.project_id.as_deref(),
        seal.artifact.feature_id.as_deref(),
        seal.artifact.task_id.as_deref(),
    )?;
    let expected = ArtifactSeal::prepare(
        seal.artifact.id.clone(),
        &owner,
        parts[3],
        "",
        seal.artifact.origin,
    )?;
    require(
        expected.artifact.relative_path == seal.artifact.relative_path
            && expected.artifact.media_type == seal.artifact.media_type,
        "Artifact 路径与原身份、归属或格式不一致",
    )?;
    let root = root.canonicalize().map_err(storage)?;
    let mut current = root;
    for part in parts {
        current.push(part);
        reject_symlink(&current)?;
    }
    Ok(current)
}
pub(super) fn inspect(root: &Path, seal: &ArtifactSeal) -> Result<Option<Artifact>> {
    let path = destination(root, seal)?;
    let file = match File::open(&path) {
        Ok(file) => file,
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => return Ok(None),
        Err(error) => return Err(storage(error)),
    };
    let metadata = file.metadata().map_err(storage)?;
    require(
        metadata.is_file() && metadata.len() == seal.bytes,
        "原 Artifact 文件长度或类型不符",
    )?;
    let mut bytes = Vec::with_capacity(seal.bytes as usize);
    file.take(seal.bytes + 1)
        .read_to_end(&mut bytes)
        .map_err(storage)?;
    require(
        bytes.len() as u64 == seal.bytes
            && format!("{:x}", Sha256::digest(&bytes)) == seal.artifact.sha256,
        "原 Artifact 文件哈希不符",
    )?;
    Ok(Some(seal.artifact.clone()))
}
pub(super) fn seal(root: &Path, seal: &ArtifactSeal, body: &str) -> Result<Artifact> {
    seal.validate_body(body)?;
    if let Some(existing) = inspect(root, seal)? {
        return Ok(existing);
    }
    let destination = destination(root, seal)?;
    let relative = Path::new(&seal.artifact.relative_path);
    let parts = relative
        .components()
        .skip(1)
        .take(3)
        .map(|p| {
            p.as_os_str()
                .to_str()
                .ok_or_else(|| err("invalid_params", "Artifact 目录编码无效"))
        })
        .collect::<Result<Vec<_>>>()?;
    artifact_directory(root, &parts)?;
    let temporary = destination.with_extension(format!("{}.pending", id()));
    let result = (|| {
        let mut options = OpenOptions::new();
        options.write(true).create_new(true);
        #[cfg(unix)]
        {
            use std::os::unix::fs::OpenOptionsExt;
            options.mode(0o600);
        }
        let mut file = options.open(&temporary).map_err(storage)?;
        file.write_all(body.as_bytes()).map_err(storage)?;
        file.sync_all().map_err(storage)?;
        drop(file);
        match fs::hard_link(&temporary, &destination) {
            Ok(()) => {}
            Err(error) if error.kind() == std::io::ErrorKind::AlreadyExists => {}
            Err(error) => return Err(storage(error)),
        }
        File::open(
            destination
                .parent()
                .ok_or_else(|| err("storage", "Artifact 缺少目录"))?,
        )
        .and_then(|dir| dir.sync_all())
        .map_err(storage)?;
        inspect(root, seal)?.ok_or_else(|| err("storage", "原身份封存后文件缺失"))
    })();
    let _ = fs::remove_file(&temporary);
    result
}

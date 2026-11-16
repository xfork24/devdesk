//! Keep recipe tags discoverable and frozen artifacts named without duplicate retention tags.
use super::*;
use std::time::Duration;

pub(crate) struct PreparationLock(File);
impl Drop for PreparationLock {
    fn drop(&mut self) {
        let _ = fs2::FileExt::unlock(&self.0);
    }
}

pub(crate) fn lock(
    provider: HarnessProvider,
    digest: &str,
    checkpoint: impl Fn() -> Result<()>,
) -> Result<PreparationLock> {
    // Docker tags are shared across datasets, so a dataset-local lock is insufficient.
    let cache = std::env::var_os("DEVDESK_RUNTIME_CACHE")
        .map(PathBuf::from)
        .unwrap_or_else(|| {
            std::env::var_os("XDG_CACHE_HOME")
                .map(PathBuf::from)
                .unwrap_or_else(|| {
                    PathBuf::from(std::env::var_os("HOME").unwrap_or_default()).join(".cache")
                })
                .join("devdesk/installations")
        });
    require(cache.is_absolute(), "安装缓存必须使用绝对路径")?;
    reject_symlink(&cache)?;
    fs::create_dir_all(&cache).map_err(storage)?;
    let path = cache.join(format!("docker-{}-{digest}.lock", provider.as_str()));
    reject_symlink(&path)?;
    let file = OpenOptions::new()
        .create(true)
        .truncate(false)
        .read(true)
        .write(true)
        .open(path)
        .map_err(storage)?;
    loop {
        checkpoint()?;
        match fs2::FileExt::try_lock_exclusive(&file) {
            Ok(()) => return Ok(PreparationLock(file)),
            Err(error) if error.kind() == std::io::ErrorKind::WouldBlock => {
                std::thread::sleep(Duration::from_millis(50));
            }
            Err(error) => return Err(storage(error)),
        }
    }
}

pub(crate) fn recipe_tag(provider: HarnessProvider, digest: &str) -> String {
    format!("devdesk/managed-{}:{}", provider.as_str(), &digest[..24])
}

fn tag(image: &str, target: &str) -> Result<()> {
    let output = process::capture(
        "docker",
        &["image".into(), "tag".into(), image.into(), target.into()],
        Duration::from_secs(30),
    )?;
    require(
        output.code == 0 && runtime_registry::docker_image_info(target)?["id"] == image,
        "无法保留所选运行产物，请重新核对准备结果",
    )
}

fn other_managed_tag(info: &Value, provider: HarnessProvider, excluded: &str) -> Option<String> {
    let prefix = format!("devdesk/managed-{}:", provider.as_str());
    info["tags"].as_array()?.iter().find_map(|value| {
        value
            .as_str()
            .filter(|tag| tag.starts_with(&prefix) && *tag != excluded)
            .map(str::to_owned)
    })
}

fn archive(provider: HarnessProvider, digest: &str, info: &Value) -> Result<String> {
    let image = info["id"]
        .as_str()
        .filter(|id| id.starts_with("sha256:") && id.len() == 71)
        .ok_or_else(|| err("provider_io", "镜像未提供内容身份"))?;
    let archive = format!("{}-{}", recipe_tag(provider, digest), &image[7..]);
    tag(image, &archive)?;
    Ok(archive)
}

fn remove_legacy_tag(provider: HarnessProvider, image: &str, retained: &str) -> Result<()> {
    // Upgrade old installations only after an equivalent managed reference is verified.
    let legacy = format!("devdesk/prepared-{}:{}", provider.as_str(), &image[7..]);
    if runtime_registry::docker_image_info(&legacy).is_ok_and(|info| info["id"] == image) {
        require(
            runtime_registry::docker_image_info(retained)?["id"] == image,
            "无法核对旧运行镜像的 managed 标签",
        )?;
        let output = process::capture(
            "docker",
            &["image".into(), "rm".into(), legacy],
            Duration::from_secs(30),
        )?;
        require(
            output.code == 0 && runtime_registry::docker_image_info(retained)?["id"] == image,
            "旧运行镜像标签迁移失败，请重新核对准备结果",
        )?;
    }
    Ok(())
}

pub(crate) fn retain(
    provider: HarnessProvider,
    digest: &str,
    actual: &Value,
    frozen: bool,
) -> Result<String> {
    let image = actual["id"]
        .as_str()
        .ok_or_else(|| err("provider_io", "镜像未提供内容身份"))?;
    let canonical = recipe_tag(provider, digest);
    let current = runtime_registry::docker_image_info(&canonical).ok();
    let retained = match current {
        Some(current) if current["id"] != image => {
            if frozen {
                // Reopening an older dataset must not move the recipe's current tag backwards.
                match other_managed_tag(actual, provider, &canonical) {
                    Some(tag) => tag,
                    None => archive(provider, digest, actual)?,
                }
            } else {
                // Build without a tag first; preserve the displaced artifact before publishing.
                let previous = match other_managed_tag(&current, provider, &canonical) {
                    Some(tag) => tag,
                    None => archive(provider, digest, &current)?,
                };
                let previous_image = current["id"]
                    .as_str()
                    .ok_or_else(|| err("provider_io", "镜像未提供内容身份"))?;
                remove_legacy_tag(provider, previous_image, &previous)?;
                tag(image, &canonical)?;
                canonical
            }
        }
        Some(_) => canonical,
        None => {
            if frozen && let Some(existing) = other_managed_tag(actual, provider, &canonical) {
                existing
            } else {
                tag(image, &canonical)?;
                canonical
            }
        }
    };
    remove_legacy_tag(provider, image, &retained)?;
    Ok(retained)
}

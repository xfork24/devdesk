//! Immutable, sanitized command results for the originating worker. This is a
//! bounded projection of its protocol journal, never a platform test receipt.
use super::*;
use std::sync::Mutex;

const MAX_BYTES: u64 = 64 * 1024 * 1024;
const MAX_FILES: usize = 512;

pub(crate) fn directory(journal: &Path) -> Result<PathBuf> {
    let parent = journal
        .parent()
        .ok_or_else(|| err("storage", "原生日志缺少目录"))?;
    let stem = journal
        .file_stem()
        .and_then(|s| s.to_str())
        .ok_or_else(|| err("storage", "原生日志身份无效"))?;
    Ok(parent.join(format!("{stem}.command-output")))
}

pub(crate) struct CommandOutputs {
    root: PathBuf,
    usage: Mutex<(usize, u64)>,
}
impl CommandOutputs {
    pub(crate) fn open(journal: &Path) -> Result<Self> {
        let root = directory(journal)?;
        reject_symlink(&root)?;
        fs::create_dir_all(&root).map_err(storage)?;
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            fs::set_permissions(&root, fs::Permissions::from_mode(0o700)).map_err(storage)?;
        }
        let mut usage = (0, 0);
        for entry in fs::read_dir(&root).map_err(storage)? {
            let entry = entry.map_err(storage)?;
            require(
                entry.file_type().map_err(storage)?.is_file(),
                "命令输出目录包含非文件对象",
            )?;
            usage.0 += 1;
            usage.1 += entry.metadata().map_err(storage)?.len();
        }
        require(
            usage.0 <= MAX_FILES && usage.1 <= MAX_BYTES,
            "原生命令输出超过技术预算",
        )?;
        Ok(Self {
            root,
            usage: Mutex::new(usage),
        })
    }
    pub(crate) fn record(&self, frame: &Value) -> Result<()> {
        let item = &frame["params"]["item"];
        if frame["method"] != "item/completed" || item["type"] != "commandExecution" {
            return Ok(());
        }
        require(item["id"].is_string(), "原生命令结果缺少确切身份")?;
        let key = configuration_hash(&json!([
            frame["params"]["threadId"],
            frame["params"]["turnId"],
            item["id"]
        ]));
        let file = self.root.join(format!("{key}.json"));
        let bytes=serde_json::to_vec(&json!({"source":"sanitized_native_command_observation","thread_id":frame["params"]["threadId"],"turn_id":frame["params"]["turnId"],"item":item})).map_err(storage)?;
        let mut usage = self.usage.lock().map_err(storage)?;
        reject_symlink(&file)?;
        if file.exists() {
            require(
                fs::read(&file).map_err(storage)? == bytes,
                "同一原生命令结果不能被覆盖",
            )?;
            return Ok(());
        }
        require(
            usage.0 < MAX_FILES && usage.1 + bytes.len() as u64 <= MAX_BYTES,
            "原生命令输出超过技术预算",
        )?;
        let mut options = std::fs::OpenOptions::new();
        options.write(true).create_new(true);
        #[cfg(unix)]
        {
            use std::os::unix::fs::OpenOptionsExt;
            options.mode(0o600);
        }
        use std::io::Write;
        let pending = self
            .root
            .join(format!("{key}.pending-{}", devdesk_core::id()));
        let mut saved = options.open(&pending).map_err(storage)?;
        saved
            .write_all(&bytes)
            .and_then(|_| saved.sync_all())
            .map_err(storage)?;
        // Publish only a complete file, without replacing another observation.
        fs::hard_link(&pending, &file).map_err(storage)?;
        fs::remove_file(&pending).map_err(storage)?;
        File::open(&self.root)
            .and_then(|dir| dir.sync_all())
            .map_err(storage)?;
        usage.0 += 1;
        usage.1 += bytes.len() as u64;
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn full_command_results_are_immutable_bounded_and_reopenable() {
        let root = tempfile::tempdir().unwrap();
        let journal = root.path().join("operation.jsonl");
        let log = CommandOutputs::open(&journal).unwrap();
        let mut frame = json!({"method":"item/completed","params":{"threadId":"thread","turnId":"turn","item":{"id":"command/../one","type":"commandExecution","aggregatedOutput":"actual output\n".repeat(8000),"exitCode":0}}});
        log.record(&frame).unwrap();
        log.record(&frame).unwrap();
        let entries = fs::read_dir(directory(&journal).unwrap())
            .unwrap()
            .collect::<std::io::Result<Vec<_>>>()
            .unwrap();
        assert_eq!(entries.len(), 1);
        let saved: Value = serde_json::from_slice(&fs::read(entries[0].path()).unwrap()).unwrap();
        assert_eq!(
            saved["item"]["aggregatedOutput"],
            frame["params"]["item"]["aggregatedOutput"]
        );
        let reopened = CommandOutputs::open(&journal).unwrap();
        reopened.record(&frame).unwrap();
        frame["params"]["item"]["aggregatedOutput"] = json!("different");
        assert!(reopened.record(&frame).is_err());
        *log.usage.lock().unwrap() = (MAX_FILES, MAX_BYTES);
        frame["params"]["item"]["id"] = json!("second");
        assert!(log.record(&frame).is_err());
        assert_eq!(
            fs::read_dir(directory(&journal).unwrap()).unwrap().count(),
            1
        );
    }
    #[cfg(unix)]
    #[test]
    fn command_output_directory_cannot_redirect_to_other_worker_data() {
        let root = tempfile::tempdir().unwrap();
        let other = tempfile::tempdir().unwrap();
        let journal = root.path().join("operation.jsonl");
        std::os::unix::fs::symlink(other.path(), directory(&journal).unwrap()).unwrap();
        assert!(CommandOutputs::open(&journal).is_err());
        assert_eq!(fs::read_dir(other.path()).unwrap().count(), 0);
    }
}

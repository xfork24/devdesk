//! Materializes worker workspace leases before constructing a container.
use super::*;

pub(super) fn prepare(
    root: &Path,
    request: &SessionStart,
    policy: ToolPolicy,
    writable_paths: &[String],
    metadata: &Path,
    private_work: PathBuf,
) -> Result<PathBuf> {
    require(
        request.workspace.is_none() || request.code_view.is_none(),
        "工作区资源不能重复声明",
    )?;
    let writer = policy != ToolPolicy::ReadOnly && request.configured_task.is_some();
    let view = if let Some(lease) = &request.workspace {
        require(
            lease.operation_id == request.execution.operation_id
                && lease.session_id == request.session.id
                && request.session.project_id.as_deref() == Some(lease.handle.project_id.as_str())
                && lease.exclusive_write == writer,
            "工作区资源租约不属于此 worker 或超出权限范围",
        )?;
        match &lease.handle.source {
            FrozenResourceSource::Artifact { id } => {
                let artifact = lease
                    .artifact
                    .as_ref()
                    .ok_or_else(|| err("storage", "封存资源缺少原记录"))?;
                require(
                    artifact.id == *id
                        && artifact.sha256 == lease.handle.version
                        && writable_paths.is_empty()
                        && policy == ToolPolicy::ReadOnly,
                    "封存资源租约的原版本或只读权限不符",
                )?;
                let files = FileArtifacts::new(PathBuf::from(&request.project_root));
                let source = files.checked_path(&artifact.relative_path)?;
                require(
                    fs::metadata(source).map_err(storage)?.len() <= 1024 * 1024,
                    "封存资源超出挂载预算",
                )?;
                write_private(&private_work.join("artifact"), &files.read(artifact)?)?;
                return Ok(private_work);
            }
            FrozenResourceSource::GitWorktree { view } => {
                require(
                    view.revision == lease.handle.version
                        && view.context.source.owner == lease.handle.owner_id,
                    "代码租约与原资源不符",
                )?;
                Some(view.as_ref())
            }
        }
    } else {
        request.code_view.as_ref()
    };
    if let Some(view) = view {
        require(
            request.session.project_id.as_deref() == Some(view.context.project.as_str())
                && (view.context.source.owner == request.session.owner_id
                    || request.configured_task.as_ref().is_some_and(|task| {
                        task.task["authorization"]["workspace_input"]
                            .as_str()
                            .is_some_and(|key| {
                                task.task["inputs"][key]["owner_id"] == view.context.source.owner
                                    && task.task["inputs"][key]["revision"] == view.revision
                            })
                    })),
            "代码视图归属不符",
        )?;
        let git = CliGit::new(root)?;
        if !writer {
            let parent = metadata.join(format!("view-{}", request.execution.operation_id));
            private_dir(&parent)?;
            let work = parent.join("work");
            git.export_revision(&view.context, &view.revision, &work)?;
            return Ok(work);
        } else {
            require(
                request.configured_task.is_some(),
                "工程工作树写入需要 Workflow 步骤的冻结任务与权限范围",
            )?;
            git.validate_view(&view.context, &view.revision)
                .map_err(|error| {
                    if error.execution_failure.is_some() {
                        error
                    } else {
                        error.technical_not_applied()
                    }
                })?;
            return Ok(PathBuf::from(&view.context.source.worktree));
        }
    }
    Ok(private_work)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn artifact_workspace_requires_exact_read_only_lease_and_sealed_bytes() {
        let root = tempfile::tempdir().unwrap();
        let files = FileArtifacts::new(root.path().to_path_buf());
        let artifact = files
            .seal_scoped(
                &MaterialOwner::Project {
                    id: "project".into(),
                },
                "workflow-outputs",
                "{\"text\":\"proof\"}",
            )
            .unwrap();
        let mut request: SessionStart = serde_json::from_value(json!({
            "session":{"id":"session","owner_id":"project","project_id":"project","agent":"agent",
                "harness":"fixture","sandbox":"fixture","origin":"external"},
            "agent":{"id":"agent","family":"agent","name":"Fixture","version":1,"body":""},
            "skills":[],"documents":[],
            "execution":{"id":"execution","kind":"harness","session_id":"session",
                "operation_id":"operation","image_digest":"fixture","status":"prepared","writer_revoked":false},
            "project_root":root.path(),
            "workspace":{"operation_id":"operation","session_id":"session","artifact":artifact,
                "handle":{"project_id":"project","owner_id":"document","version":artifact.sha256,
                    "source":{"kind":"artifact","id":artifact.id}}}
        })).unwrap();
        let work = root.path().join("work");
        private_dir(&work).unwrap();
        let mounted = prepare(
            root.path(),
            &request,
            ToolPolicy::ReadOnly,
            &[],
            root.path(),
            work.clone(),
        )
        .unwrap();
        assert_eq!(
            fs::read(mounted.join("artifact")).unwrap(),
            b"{\"text\":\"proof\"}"
        );
        assert!(
            prepare(
                root.path(),
                &request,
                ToolPolicy::WorkspaceWrite,
                &[],
                root.path(),
                work.clone()
            )
            .is_err()
        );

        request.execution.operation_id = "different".into();
        assert!(
            prepare(
                root.path(),
                &request,
                ToolPolicy::ReadOnly,
                &[],
                root.path(),
                work.clone()
            )
            .is_err()
        );
        request.execution.operation_id = "operation".into();
        fs::write(
            files.checked_path(&artifact.relative_path).unwrap(),
            b"tampered",
        )
        .unwrap();
        assert!(
            prepare(
                root.path(),
                &request,
                ToolPolicy::ReadOnly,
                &[],
                root.path(),
                work
            )
            .is_err()
        );
    }
}

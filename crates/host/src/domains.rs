//! Client-neutral domain directory, commands, and update notifications.
use super::*;
use base64::{Engine as _, engine::general_purpose::STANDARD};
use devdesk_adapters::domain_packages::{DomainPackages, MAX_ARCHIVE, is_digest};
use devdesk_core::{
    Command, EngineeringStore, InvocationContext, concurrency::*, ports::DomainReader,
};

pub(super) struct ProjectReader {
    pub packages: Arc<DomainPackages>,
}

impl DomainReader for ProjectReader {
    fn read(&self, project: &str, handler: &str, input: Value) -> devdesk_domain::Result<Value> {
        self.packages.read_for_project(project, handler, input)
    }

    fn has_hook(&self, project: &str, group: &str, hook: &str) -> devdesk_domain::Result<bool> {
        self.packages.has_hook(project, group, hook)
    }

    fn hook(
        &self,
        project: &str,
        group: &str,
        hook: &str,
        input: Value,
    ) -> devdesk_domain::Result<Option<Value>> {
        self.packages.hook(project, group, hook, input)
    }

    fn resource(
        &self,
        project: &str,
        group: &str,
        collection: &str,
        key: &str,
    ) -> devdesk_domain::Result<Option<(String, String)>> {
        self.packages.resource(project, group, collection, key)
    }
}

pub(super) fn method(name: &str) -> bool {
    name.starts_with("domain.")
        || name.starts_with("query.domain.")
        || name.starts_with("query.core.git.")
}

fn command_identity(params: &Value) -> devdesk_domain::Result<&str> {
    params["command_id"]
        .as_str()
        .map(str::trim)
        .filter(|command| !command.is_empty() && command.len() <= 256)
        .ok_or_else(|| err("invalid_params", "需要有效的命令身份"))
}

fn project_directory_change(
    workspace: &Workspace,
    command_id: String,
    fingerprint: String,
    project: &str,
    removed: bool,
    result: Value,
) -> devdesk_domain::Result<Value> {
    let key = devdesk_adapters::GlobalStore::project_removal_key(project);
    let current = workspace
        .global
        .read_resources(std::slice::from_ref(&key))?
        .remove(0);
    workspace
        .global
        .commit_changes(&PreparedChangeSet {
            command_id,
            fingerprint,
            read_set: ReadSet::new(vec![ResourceVersion {
                resource: key.clone(),
                revision: current.revision,
            }])?,
            changes: vec![ResourceChange {
                resource: key,
                value: json!(removed),
            }],
            result,
            time: std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap_or_default()
                .as_millis() as i64,
        })
        .map(|receipt| receipt.result)
}

pub(super) fn dispatch(app: &App, name: &str, params: Value) -> devdesk_domain::Result<Value> {
    let workspace = app.workspace()?;
    if name != "domain.project.remove"
        && let Some(project) = params["project_id"].as_str()
    {
        workspace.require_listed_project(project)?;
    }
    let run = |method: &str, payload: Value| {
        workspace
            .dispatch_with_receipt(method, payload, None)
            .map(|reply| reply.value)
    };
    match name {
        "query.core.git.head" => {
            return devdesk_adapters::git::local_git(&params, false);
        }
        "query.core.git.diff" => {
            return devdesk_adapters::git::local_git(&params, true);
        }
        _ => {}
    }
    let packages = workspace.domain_packages()?;
    let text = |key: &str| {
        params[key]
            .as_str()
            .ok_or_else(|| err("invalid_params", format!("缺少 {key}")))
    };
    let epoch = || {
        params["epoch"]
            .as_u64()
            .ok_or_else(|| err("invalid_params", "缺少活动领域代次"))
    };
    let digest = || {
        text("digest").and_then(|id| {
            if is_digest(id) {
                Ok(id)
            } else {
                Err(err("invalid_params", "领域包摘要无效"))
            }
        })
    };
    let entity = || {
        let id = text("entity_id")?;
        if uuid::Uuid::parse_str(id).is_ok_and(|parsed| parsed.to_string() == id) {
            Ok(id)
        } else {
            Err(err("invalid_params", "领域对象 ID 需要规范 UUID"))
        }
    };
    match name {
        "query.domain.active" => Ok(packages.status()),
        "query.domain.installed" => packages.installed(),
        "query.domain.storage" => packages.storage(epoch()?),
        "domain.storage.configure" => {
            let expected = params["expected_revision"]
                .as_u64()
                .ok_or_else(|| err("invalid_params", "缺少领域数据目录版本"))?;
            let path = match params.get("data_root") {
                Some(Value::Null) => None,
                Some(Value::String(path)) if !path.trim().is_empty() => {
                    Some(std::path::Path::new(path))
                }
                _ => return Err(err("invalid_params", "需要领域数据目录或 null")),
            };
            let epoch = epoch()?;
            let (_, projects) =
                packages.configure_storage(&workspace.global, epoch, expected, path)?;
            workspace.reopen_available_domain_projects(projects)?;
            workspace.discover_domain_projects(epoch)?;
            app.domain_epoch.send_modify(|_| {});
            packages.storage(epoch)
        }
        "domain.install" => {
            let archive = text("archive")?;
            if archive.len() > MAX_ARCHIVE.div_ceil(3) * 4 {
                return Err(err("invalid_params", "领域 ZIP 超出大小限制"));
            }
            let bytes = STANDARD
                .decode(archive)
                .map_err(|_| err("invalid_params", "领域 ZIP 编码无效"))?;
            packages.install(&bytes)
        }
        "domain.activate" | "domain.deactivate" | "domain.uninstall" => {
            let uninstall = name == "domain.uninstall";
            let quiescent = |projects: Vec<String>| {
                for project in projects {
                    if uninstall {
                        if workspace.domain_project_has_running_workflow(&project)? {
                            return Err(err(
                                "domain_busy",
                                format!("项目 {project} 中仍有工作流正在执行"),
                            ));
                        }
                    } else if workspace.domain_project_busy(&project)? {
                        return Err(err(
                            "domain_busy",
                            format!("请先结束项目 {project} 的在途领域工作流或核对仓库操作"),
                        ));
                    }
                }
                Ok(())
            };
            let epoch = if name == "domain.activate" {
                let prepare = |projects| workspace.reopen_available_domain_projects(projects);
                packages.activate(digest()?, epoch()?, prepare, quiescent)?
            } else {
                packages.deactivate(digest()?, epoch()?, uninstall, quiescent)?
            };
            let discovered = if name == "domain.activate" {
                workspace.discover_domain_projects(epoch)
            } else {
                Ok(())
            };
            publish(app, epoch);
            discovered?;
            Ok(packages.status())
        }
        "domain.project.create" => {
            let title = text("title")?;
            let command = command_identity(&params)?;
            let expected_epoch = epoch()?;
            packages.with_sources(expected_epoch, params.get("connection"), |digest, sources, plan| {
                let mut payload = json!({"title":title});
                let mut existing_project = None;
                let reuse = plan.is_some() && params["reuse_connection"] == true;
                let _open = reuse
                    .then(|| workspace.domain_project_open.lock().map_err(|e| err("storage", e.to_string())))
                    .transpose()?;
                let open_id = format!("domain-project-open:{}", devdesk_domain::configuration_hash(&(digest, command)));
                let fingerprint = devdesk_domain::configuration_hash(&(digest, title, params.get("connection")));
                if reuse && let Some(previous) = workspace.global.replay_commit(&open_id, &fingerprint)? {
                    return Ok(previous.result);
                }
                if let Some(plan) = &plan {
                    let repository = plan["repository"].as_str().ok_or_else(|| err("domain_contract", "连接缺少仓库"))?;
                    let mainline = plan["mainline"].as_str().ok_or_else(|| err("domain_contract", "连接缺少目标分支"))?;
                    let repository = crate::workspace::github_projects::repository_name(repository)?;
                    if reuse {
                        let visibility = packages.project_visibility(Some(expected_epoch))?;
                        existing_project = workspace.global.domain_github_project(&repository, mainline, &visibility)?
                            .map(|project| json!(project));
                        if existing_project.is_none() {
                            let root = packages.project_import_root(digest)?;
                            if let Some(project) = workspace.global.copied_github_project(&root, &repository, mainline)? {
                                packages.bind_copied_project(&project.id, digest, &root)?;
                                existing_project = Some(workspace.load_copied_project(&project)?);
                            }
                        }
                    }
                    payload["repository"] = json!(repository);
                    payload["mainline"] = json!(mainline);
                }
                let created = if let Some(existing) = existing_project {
                    existing
                } else {
                    packages.project_import_root(digest)?;
                    workspace.create_project(
                    Command {
                        command_id: format!(
                            "domain-project:{}",
                            devdesk_domain::configuration_hash(&(digest, command))
                        ),
                        target: None,
                        expected_revision: None,
                        payload,
                    },
                        |project| packages.bind_project(project, digest),
                    )?
                };
                let project = created["id"]
                    .as_str()
                    .ok_or_else(|| err("storage", "项目回执缺少 ID"))?;
                let (engine, reopened) = match workspace.project_engine(Some(project)) {
                    Err(error) if reuse && error.code == "not_found" => {
                        packages.recover_project_from_storage(digest)?;
                        (workspace.open_registered_domain_project(project)?, true)
                    }
                    result => (result?, false),
                };
                engine.adopt_domain_sources(sources)?;
                if let Some(plan) = plan {
                    let state = engine.store.read_resources(&[devdesk_core::concurrency::ResourceKey::new(
                        "repository_project_state", project,
                    )])?.remove(0);
                    if state.value.as_ref().is_some_and(|row| row["connect_exists"] == false) {
                        engine.execute("repository.connect", Command {
                            command_id: format!("domain-project-connect:{project}"),
                            target: Some(project.into()),
                            expected_revision: Some(engine.store.project_metadata()?.revision),
                            payload: json!({"repository":plan["repository"],"mainline":plan["mainline"]}),
                        })?;
                    }
                }
                if reuse {
                    let was_removed = workspace.global.project_removed(project)?;
                    let current = json!(engine.store.project_metadata()?);
                    let result = project_directory_change(&workspace, open_id, fingerprint, project, false, current)?;
                    if was_removed || reopened {
                        app.domain_epoch.send_modify(|_| {});
                    }
                    Ok(result)
                } else {
                    Ok(created)
                }
            })
        }
        "domain.project.remove" => {
            let project = text("project_id")?;
            let command = command_identity(&params)?;
            packages.with_project(project, epoch()?, |digest| {
                let _open = workspace
                    .domain_project_open
                    .lock()
                    .map_err(|e| err("storage", e.to_string()))?;
                let existing = workspace
                    .global
                    .project(project)?
                    .ok_or_else(|| err("not_found", "项目不存在"))?;
                if existing
                    .workspace_definition
                    .as_ref()
                    .is_none_or(|workspace| workspace.repository.is_empty())
                {
                    return Err(err(
                        "invalid_params",
                        "只有已登记 GitHub 仓库的项目可以移除",
                    ));
                }
                let id = format!(
                    "domain-project-remove:{}",
                    devdesk_domain::configuration_hash(&(digest, command))
                );
                let fingerprint = devdesk_domain::configuration_hash(&(digest, project));
                if let Some(previous) = workspace.global.replay_commit(&id, &fingerprint)? {
                    return Ok(previous.result);
                }
                let result = project_directory_change(
                    &workspace,
                    id,
                    fingerprint,
                    project,
                    true,
                    json!({"id":project,"removed":true}),
                )?;
                app.domain_epoch.send_modify(|_| {});
                Ok(result)
            })
        }
        "domain.workflow.start" => {
            let reference = text("workflow_ref")?;
            let command = command_identity(&params)?;
            let mode = if params.get("mode").is_some() {
                text("mode")?
            } else {
                "rust"
            };
            if !matches!(mode, "manual" | "rust" | "agent") {
                return Err(err(
                    "invalid_params",
                    "领域工作流需要 manual、rust 或 agent 模式",
                ));
            }
            let inputs = params.get("inputs").cloned().unwrap_or(json!({}));
            if !inputs.is_object() {
                return Err(err("invalid_params", "工作流输入必须是 JSON 对象"));
            }
            let owner = if params.get("entity_id").is_some() || params.get("project_id").is_some() {
                let revision = params["expected_revision"]
                    .as_u64()
                    .ok_or_else(|| err("invalid_params", "缺少工作归属原修订"))?;
                Some((text("project_id")?, entity()?, revision))
            } else {
                None
            };
            packages.with_workflow(epoch()?, (reference, mode, owner.map(|owner| owner.0)), |package| {
                let identity = devdesk_domain::configuration_hash(&(package.digest, command));
                let payload = json!({"workflow_ref":reference,"orchestration":{"mode":mode},
                    "role_agent_bindings":package.role_agent_bindings,"inputs":inputs});
                if let Some((project, entity, revision)) = owner {
                    let engine = workspace.project_engine(Some(project))?;
                    let entity_record = engine.read_domain_entity(entity)?;
                    let kind = entity_record["type_ref"].as_str().ok_or_else(|| err("storage", "对象缺少类型"))?;
                    let binding = package.entity_bindings.get(kind)
                        .ok_or_else(|| err("blocked", "此领域对象类型未声明该工作流"))?;
                    let result = engine.start_domain_workflow(&Command {
                        command_id: format!("domain-entity-start:{identity}"),
                        target: Some(entity.into()), expected_revision: Some(revision), payload,
                    }, binding)?;
                    return Ok(json!({"project_id":project,"execution_id":result["id"],"owner_id":entity}));
                }
                if !package.entity_bindings.is_empty() {
                    return Err(err("invalid_params", "此工作流需要包声明的领域对象作为归属"));
                }
                let created = workspace.create_project(
                    Command {
                        command_id: format!("domain-project:{identity}"), target: None,
                        expected_revision: None, payload: json!({"title":format!("Domain workflow {reference}")}),
                    },
                    |project| packages.bind_project(project, package.digest),
                )?;
                let project = created["id"].as_str().ok_or_else(|| err("storage", "项目回执缺少 ID"))?;
                workspace.project_engine(Some(project))?.adopt_domain_sources(package.sources)?;
                let result = run("workflow.start", json!({
                    "project_id":project,"command_id":format!("domain-start:{identity}"), "target":project,
                    "expected_revision":devdesk_core::project_creation::INITIAL_PROJECT_REVISION,
                    "payload":payload,
                }))?;
                Ok(json!({"project_id":project,"execution_id":result["id"]}))
            })
        }
        "query.domain.workflow" => {
            let project = text("project_id")?;
            let id = text("execution_id")?;
            packages.with_project(project, epoch()?, |_| {
                let mut result = run(
                    "query.workflow_instance",
                    json!({"project_id":project,"id":id}),
                )?;
                if result["record"]["data"]["status"] == "completed"
                    && let Some(submission) = workspace
                        .project_engine(Some(project))?
                        .read_domain_submission(id)?
                {
                    result["domain_submission"] = submission;
                }
                Ok(result)
            })
        }
        "domain.entity.put" => {
            let project = text("project_id")?;
            let id = entity()?;
            let reference = text("type_ref")?;
            let command_id = text("command_id")?.trim();
            if command_id.is_empty() || command_id.len() > 256 {
                return Err(err("invalid_params", "需要有效的领域对象命令身份"));
            }
            let expected = params["expected_revision"]
                .as_u64()
                .ok_or_else(|| err("invalid_params", "缺少领域对象原修订"))?;
            let data = &params["data"];
            packages.validate_entity(project, epoch()?, (reference, data), |digest, relations| {
                workspace
                    .project_engine(Some(project))?
                    .write_domain_entity(
                        digest,
                        relations,
                        &Command {
                            command_id: command_id.into(),
                            target: Some(id.into()),
                            expected_revision: Some(expected),
                            payload: json!({
                                "type_ref":reference, "data":data,
                                "relations":params.get("relations").cloned().unwrap_or_else(|| json!({}))
                            }),
                        },
                        &InvocationContext::human(Some(project.into())),
                    )
            })
        }
        "query.domain.entity" => {
            let project = text("project_id")?;
            let id = entity()?;
            packages.with_project(project, epoch()?, |_| {
                workspace
                    .project_engine(Some(project))?
                    .read_domain_entity(id)
            })
        }
        "query.domain.contribution" => {
            let epoch = epoch()?;
            let method = packages.contribution_projection(epoch, text("contribution")?)?;
            let mut input = params["input"]
                .as_object()
                .filter(|fields| fields.get("project_id").and_then(Value::as_str).is_some())
                .cloned()
                .ok_or_else(|| err("invalid_params", "领域贡献需要项目作用域"))?;
            input.insert("domain_epoch".into(), json!(epoch));
            workspace
                .dispatch_public_rpc(&method, Value::Object(input))
                .map(|reply| reply.value)
        }
        "query.domain.invoke" => {
            packages.invoke(epoch()?, text("handler")?, params["input"].clone())
        }
        _ => Err(err("method_not_found", "领域 RPC 未注册")),
    }
}

fn publish(app: &App, epoch: u64) {
    app.domain_epoch.send_if_modified(|seen| {
        if epoch <= *seen {
            return false;
        }
        *seen = epoch;
        true
    });
}

pub(super) fn until_epoch_changes(
    app: &App,
    pinned: Option<u64>,
) -> impl Future<Output = ()> + use<> {
    let mut updates = app.domain_epoch.subscribe();
    if let Some(epoch) = pinned {
        publish(app, epoch);
    }
    async move {
        let Some(epoch) = pinned else {
            return std::future::pending().await;
        };
        let _ = updates.wait_for(|seen| *seen != epoch).await;
    }
}

pub(super) async fn events(State(app): State<Arc<App>>) -> Response {
    let load = app.clone();
    let current = tokio::task::spawn_blocking(move || {
        load.workspace()
            .and_then(|workspace| Ok(workspace.domain_packages()?.epoch()))
    })
    .await;
    if let Ok(Ok(epoch)) = current {
        publish(&app, epoch);
    }
    let rx = app.domain_epoch.subscribe();
    let updates = stream::unfold((app, rx, true), |(app, mut rx, first)| async move {
        if !first {
            tokio::select! {
                result = rx.changed() => { result.ok()?; }
                _ = app.shutdown.notified() => {}
            }
        }
        if app.stopping.load(Ordering::Relaxed) {
            return None;
        }
        let epoch = *rx.borrow_and_update();
        let event = Event::default()
            .event("domain")
            .id(epoch.to_string())
            .data(epoch.to_string());
        Some((Ok::<_, std::convert::Infallible>(event), (app, rx, false)))
    });
    Sse::new(updates)
        .keep_alive(KeepAlive::default())
        .into_response()
}

pub(super) async fn asset(
    State(app): State<Arc<App>>,
    Path((epoch, digest, path)): Path<(u64, String, String)>,
) -> Response {
    let result = app
        .workspace()
        .and_then(|workspace| Ok(workspace.domain_packages()?.asset(epoch, &digest, &path)));
    match result {
        Ok(Some(bytes)) => (
            [(
                header::CONTENT_TYPE,
                mime_guess::from_path(&path)
                    .first_or_octet_stream()
                    .to_string(),
            )],
            bytes,
        )
            .into_response(),
        _ => StatusCode::NOT_FOUND.into_response(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use devdesk_core::id;
    use devdesk_domain::{
        RepositoryAction, RepositoryOperation, RepositoryOperationStatus, WorktreeKind,
        configuration_hash,
    };

    #[test]
    fn removed_project_only_ignores_parked_repository_connections() {
        let root = tempfile::tempdir().unwrap();
        let workspace =
            Workspace::open_with_adapters(root.path(), crate::test_runtime::adapters()).unwrap();
        let opened = workspace
            .dispatch(
                "project.open_github",
                json!({"command_id":id(),"payload":{"repository":"fixture/failed-connect"}}),
            )
            .unwrap();
        let project = opened["id"].as_str().unwrap();
        let set_removed = |removed| {
            project_directory_change(
                &workspace,
                id(),
                configuration_hash(&(project, removed)),
                project,
                removed,
                Value::Null,
            )
            .unwrap();
        };
        set_removed(true);
        assert!(
            workspace.domain_project_busy(project).unwrap(),
            "prepared connection"
        );
        assert!(
            !workspace
                .domain_project_has_running_workflow(project)
                .unwrap()
        );

        // The fixture Git adapter fails without external I/O.
        workspace.tick_fixture(|| false).unwrap();
        let engine = workspace.project_engine(Some(project)).unwrap();
        let operation = engine.store.snapshot().unwrap().repository_operations[0].clone();
        assert_eq!(
            operation.status,
            RepositoryOperationStatus::NeedsReconciliation
        );
        assert!(!engine.store.has_pending_work().unwrap());
        assert!(
            !workspace.domain_project_busy(project).unwrap(),
            "removed project with an idle connection"
        );

        set_removed(false);
        assert!(
            workspace.domain_project_busy(project).unwrap(),
            "visible project awaiting review"
        );
        workspace
            .dispatch(
                "repository.reconcile",
                json!({"project_id":project,"command_id":id(),"target":operation.id,
                    "expected_revision":operation.revision,"payload":{}}),
            )
            .unwrap();
        set_removed(true);
        assert!(
            workspace.domain_project_busy(project).unwrap(),
            "requested reconciliation"
        );
        workspace.tick_fixture(|| false).unwrap();
        assert!(
            !workspace.domain_project_busy(project).unwrap(),
            "connection is idle again"
        );

        engine
            .store
            .seed_fixture_state(None, &mut |state| {
                state.repository_operations.push(RepositoryOperation {
                    id: id(),
                    project_id: project.to_owned(),
                    action: RepositoryAction::CreateWorktree {
                        owner: id(),
                        worktree_kind: WorktreeKind::Feature,
                        base: "a".repeat(40),
                        branch: None,
                    },
                    status: RepositoryOperationStatus::NeedsReconciliation,
                    receipt: None,
                    reason: "原仓库操作结果未知".into(),
                    revision: 1,
                });
                Ok(Value::Null)
            })
            .unwrap();
        assert!(
            workspace.domain_project_busy(project).unwrap(),
            "unresolved worktree effect"
        );
    }
}

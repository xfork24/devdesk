//! Client-neutral domain directory, commands, and update notifications.
use super::*;
use base64::{Engine as _, engine::general_purpose::STANDARD};
use devdesk_adapters::domain_packages::{DomainPackages, MAX_ARCHIVE, is_digest};
use devdesk_core::{Command, InvocationContext, ports::DomainReader};

pub(super) struct ProjectReader {
    pub packages: Arc<DomainPackages>,
    pub trusted: Arc<std::collections::BTreeSet<String>>,
}

impl DomainReader for ProjectReader {
    fn read(&self, project: &str, handler: &str, input: Value) -> devdesk_domain::Result<Value> {
        self.packages
            .read_for_project(project, handler, input, &self.trusted)
    }

    fn worktree_branch(
        &self,
        project: &str,
        owner: &str,
        kind: devdesk_domain::WorktreeKind,
    ) -> devdesk_domain::Result<Option<String>> {
        self.packages
            .worktree_branch(project, owner, kind, &self.trusted)
    }

    fn project_test_discovery(
        &self,
        project: &str,
        mainline: &str,
    ) -> devdesk_domain::Result<Option<devdesk_domain::ProjectTestDiscovery>> {
        self.packages
            .project_test_discovery(project, mainline, &self.trusted)
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

pub(super) fn dispatch(app: &App, name: &str, params: Value) -> devdesk_domain::Result<Value> {
    let workspace = app.workspace()?;
    let run = |method: &str, payload: Value| {
        workspace
            .dispatch_with_receipt(method, payload)
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
    let trusted = &app.trusted_domains;
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
        "query.domain.active" => Ok(packages.status(trusted)),
        "query.domain.installed" => packages.installed(),
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
            let quiescent = |projects: Vec<String>| {
                for project in projects {
                    if workspace.domain_project_busy(&project)? {
                        return Err(err(
                            "domain_busy",
                            format!("请先结束项目 {project} 的在途领域工作流或核对仓库操作"),
                        ));
                    }
                }
                Ok(())
            };
            let epoch = if name == "domain.activate" {
                packages.activate(digest()?, epoch()?, trusted, quiescent)?
            } else {
                packages.deactivate(digest()?, epoch()?, name == "domain.uninstall", quiescent)?
            };
            publish(app, epoch);
            Ok(packages.status(trusted))
        }
        "domain.project.create" => {
            let title = text("title")?;
            let command = command_identity(&params)?;
            packages.with_sources(epoch()?, trusted, |digest, sources| {
                let created = workspace.create_project(
                    Command {
                        command_id: format!(
                            "domain-project:{}",
                            devdesk_domain::configuration_hash(&(digest, command))
                        ),
                        target: None,
                        expected_revision: None,
                        payload: json!({"title":title}),
                    },
                    |project| packages.bind_project(project, digest),
                )?;
                let project = created["id"]
                    .as_str()
                    .ok_or_else(|| err("storage", "项目回执缺少 ID"))?;
                workspace
                    .project_engine(Some(project))?
                    .adopt_domain_sources(sources)?;
                Ok(created)
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
            packages.with_workflow(epoch()?, (reference, mode, owner.map(|owner| owner.0)), trusted, |package| {
                let identity = devdesk_domain::configuration_hash(&(package.digest, command));
                let payload = json!({"workflow_ref":reference,"orchestration":{"mode":mode},
                    "role_agent_bindings":package.role_agent_bindings,"inputs":inputs});
                if let Some((project, entity, revision)) = owner {
                    let engine = workspace.project_engine(Some(project))?;
                    let entity_record = engine.read_domain_entity(package.digest, entity)?;
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
            packages.with_project(project, epoch()?, trusted, |_| {
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
            packages.validate_entity(project, epoch()?, trusted, (reference, data), |digest, relations| {
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
            packages.with_project(project, epoch()?, trusted, |digest| {
                workspace
                    .project_engine(Some(project))?
                    .read_domain_entity(digest, id)
            })
        }
        "query.domain.invoke" => {
            packages.invoke(epoch()?, text("handler")?, params["input"].clone(), trusted)
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
    let result = app.workspace().and_then(|workspace| {
        Ok(workspace
            .domain_packages()?
            .asset(epoch, &digest, &path, &app.trusted_domains))
    });
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

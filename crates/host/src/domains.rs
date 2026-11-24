//! Client-neutral domain directory, commands, and update notifications.
use super::*;
use base64::{Engine as _, engine::general_purpose::STANDARD};
use devdesk_adapters::domain_packages::{DomainPackages, MAX_ARCHIVE, is_digest};
use devdesk_core::{concurrency::ResourceKey, ports::DomainReader};

pub(super) struct ProjectReader {
    pub packages: Arc<DomainPackages>,
    pub trusted: Arc<std::collections::BTreeSet<String>>,
}

impl DomainReader for ProjectReader {
    fn read(&self, project: &str, handler: &str, input: Value) -> devdesk_domain::Result<Value> {
        self.packages
            .read_for_project(project, handler, input, &self.trusted)
    }
}

pub(super) fn method(name: &str) -> bool {
    name.starts_with("domain.")
        || name.starts_with("query.domain.")
        || name.starts_with("query.core.git.")
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
                    let engine = workspace.project_engine(Some(&project))?;
                    let key = ResourceKey::new("workflow_active_owner", &project);
                    let active = engine.store.read_resources(&[key])?.into_iter().next();
                    if active
                        .and_then(|row| row.value)
                        .is_some_and(|row| row["active"] == true)
                    {
                        return Err(err(
                            "domain_busy",
                            format!("请先结束项目 {project} 的在途领域工作流"),
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
        "domain.workflow.start" => {
            let reference = text("workflow_ref")?;
            let command = text("command_id")?.trim();
            if command.is_empty() || command.len() > 256 {
                return Err(err("invalid_params", "需要有效的工作流命令身份"));
            }
            let mode = params["mode"].as_str().unwrap_or("rust");
            if !matches!(mode, "manual" | "rust") {
                return Err(err("invalid_params", "当前领域入口支持 manual 和 rust"));
            }
            packages.with_workflow(epoch()?, reference, trusted, |digest, sources| {
                let identity = devdesk_domain::configuration_hash(&(digest, command));
                let project = run(
                    "project.create",
                    json!({
                        "command_id":format!("domain-project:{identity}"),
                        "payload":{"title":format!("Domain workflow {reference}")},
                    }),
                )?;
                let project = project["id"]
                    .as_str()
                    .ok_or_else(|| err("storage", "项目回执缺少 ID"))?;
                packages.bind_project(project, digest)?;
                let engine = workspace.project_engine(Some(project))?;
                engine.adopt_domain_sources(sources)?;
                let result = run("workflow.start", json!({
                    "project_id":project,"command_id":format!("domain-start:{identity}"),
                    "target":project,"expected_revision":engine.store.project_metadata()?.revision,
                    "payload":{"workflow_ref":reference,"orchestration":{"mode":mode},
                        "inputs":params.get("inputs").cloned().unwrap_or(json!({}))},
                }))?;
                Ok(json!({"project_id":project,"execution_id":result["id"]}))
            })
        }
        "query.domain.workflow" => {
            let project = text("project_id")?;
            packages.with_project(project, epoch()?, trusted, || {
                run(
                    "query.workflow_instance",
                    json!({
                        "project_id":project,"id":text("execution_id")?,
                    }),
                )
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

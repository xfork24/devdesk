//! Discussion submission freezes a bounded committed context and two exact
//! local files before one Session/Turn admission. Formal task inputs use their
//! separate original workflow/controller authority.
use crate::{concurrency::*, conversation_admission::NativeConversationInput, *};

fn version(row: &ResourceSnapshot) -> ResourceVersion {
    ResourceVersion {
        resource: row.resource.clone(),
        revision: row.revision,
    }
}

impl Engine {
    pub(crate) fn native_conversation_submission(
        &self,
        method: &str,
        c: &Command,
        fingerprint: &str,
        context: &InvocationContext,
    ) -> Result<Option<Value>> {
        if method != "conversation.send" {
            return Ok(None);
        }
        if let Some(result) = self.replay_sealed_command(&c.command_id, fingerprint)? {
            return Ok(Some(result));
        }
        let sid = c
            .target
            .as_deref()
            .ok_or_else(|| err("invalid_params", "缺少会话目标"))?;
        let client = text(&c.payload, "client_message_id")?;
        let input = text(&c.payload, "input")?;
        let keys = [
            ResourceKey::new("session", sid),
            ResourceKey::new("session_input_state", sid),
            ResourceKey::new("conversation_client", json!([sid, client]).to_string()),
        ];
        let rows = self.store.read_resources(&keys)?;
        require(
            rows.len() == 3 && rows.iter().zip(&keys).all(|(r, k)| &r.resource == k),
            "输入原会话事实读取不完整",
        )?;
        let session: Session = serde_json::from_value(
            self.store.restore_runtime_value(
                rows[0]
                    .value
                    .as_ref()
                    .ok_or_else(|| err("not_found", "会话不存在"))?,
            )?,
        )
        .map_err(|e| err("storage", e.to_string()))?;
        let state = rows[1]
            .value
            .as_ref()
            .ok_or_else(|| err("storage", "输入准入谓词缺失"))?;
        if state["configured"] == true || state["controller"] == true {
            return self
                .native_formal_conversation_submission(c, fingerprint, context, session, &rows)
                .map(Some);
        }
        require(
            session
                .runtime_binding
                .as_ref()
                .is_some_and(|b| b.work_type == WorkType::Discussion),
            "会话缺少实际正式来源或讨论绑定",
        )?;
        require(
            matches!(context.actor(), WorkflowActor::Human),
            "自由讨论输入需要实际 Human 来源",
        )?;
        let project = session
            .project_id
            .as_deref()
            .ok_or_else(|| err("invalid_scope", "会话缺少项目"))?;
        context.require_scope(if session.scope == Some(SessionScope::Global) {
            None
        } else {
            Some(project)
        })?;
        require(
            self.store.project_id().is_none_or(|id| id == project),
            "输入不能跨项目",
        )?;
        require(
            ![
                "task",
                "frozen_task",
                "task_instructions",
                "task_source",
                "scenario",
            ]
            .iter()
            .any(|key| c.payload.get(key).is_some()),
            "自由讨论不能提交或替换正式任务来源",
        )?;
        let references = c
            .payload
            .get("experience_refs")
            .cloned()
            .unwrap_or(json!([]));
        let refs = references
            .as_array()
            .ok_or_else(|| err("invalid_params", "experience_refs 必须为列表"))?;
        require(
            input.len() <= 65536 && client.len() <= 160 && refs.len() <= 20,
            "讨论输入、消息 ID 或经验引用超出限制",
        )?;
        for reference in refs {
            text(reference, "source_project_id")?;
            text(reference, "snapshot_id")?;
        }
        let provenance = context.prepare_record(method, c);
        if let Some(previous) = &rows[2].value {
            let previous = self.store.restore_runtime_value(previous)?;
            require(
                previous["input"] == input && previous["experience_refs"] == references,
                "客户端消息 ID 被不同输入或经验引用复用",
            )?;
            let tid = text(&previous, "id")?;
            return self
                .store
                .commit_changes(&PreparedChangeSet {
                    command_id: c.command_id.clone(),
                    fingerprint: fingerprint.into(),
                    read_set: ReadSet::new(vec![
                        version(&rows[2]),
                        ResourceVersion {
                            resource: provenance.resource.clone(),
                            revision: 0,
                        },
                    ])?,
                    changes: vec![provenance],
                    result: json!({"id":tid,"operation_id":tid,"replayed":true}),
                    time: now() as i64,
                })
                .map(|r| Some(r.result));
        }
        if c.expected_revision != Some(rows[0].revision) {
            return Err(err(
                "revision_conflict",
                "会话原版本已改变，请保留输入并刷新",
            ));
        }
        require(
            !matches!(
                session.lifecycle,
                Some(
                    SessionLifecycle::RequiresSuccessor
                        | SessionLifecycle::Closed
                        | SessionLifecycle::NeedsReconciliation
                )
            ),
            "原会话已关闭或需要带快照的后继会话",
        )?;
        if state["permission_switch_pending"] == true {
            return Err(err(
                "permission_switch_pending",
                "原会话正在切换，请恢复已有请求",
            ));
        }
        require(state["active"] == false, "会话正忙或上轮等待核对")?;
        let worker_key = ResourceKey::new("session_live_workers", sid);
        let worker = self
            .store
            .read_resources(std::slice::from_ref(&worker_key))?
            .into_iter()
            .next()
            .ok_or_else(|| err("storage", "原 worker 谓词缺失"))?;
        require(
            worker
                .value
                .as_ref()
                .and_then(Value::as_array)
                .is_some_and(Vec::is_empty),
            "原会话仍有未撤销的 worker，需要先核对",
        )?;
        let (code_view, code_guards) = self.native_discussion_code_view(&session)?;
        let read_set = ReadSet::new(rows.iter().map(version).collect())?
            .merge([version(&worker)])?
            .merge(code_guards.versions().iter().cloned())?;
        self.admit_native_conversation_input(
            c,
            fingerprint,
            NativeConversationInput {
                session,
                original: rows[0].clone(),
                read_set,
                changes: vec![],
                provenance,
                input,
                client,
                references,
                task_source: None,
                code_view,
                workspace: None,
                origin: Origin::Human,
                capture: None,
                extra_files: vec![],
            },
        )
        .map(Some)
    }

    pub(crate) fn native_discussion_code_view(
        &self,
        session: &Session,
    ) -> Result<(Option<FrozenCode>, ReadSet)> {
        if session.scope == Some(SessionScope::Global) {
            return Ok((None, ReadSet::default()));
        }
        let project = session
            .project_id
            .as_deref()
            .ok_or_else(|| err("invalid_scope", "代码视图缺少项目"))?;
        let (owner, owner_version) = self.session_work_owner(session)?;
        if owner != session.owner_id {
            return Ok((None, ReadSet::new(vec![owner_version])?));
        }
        let owner_key = ResourceKey::new(
            "repository_worktree_owner",
            json!([project, session.owner_id]).to_string(),
        );
        let head_id = if session.scope == Some(SessionScope::Project) {
            format!("main:{project}")
        } else {
            session.owner_id.clone()
        };
        let keys = [
            owner_key,
            ResourceKey::new("head", head_id),
            ResourceKey::new("project", project),
        ];
        let rows = self.store.read_resources(&keys)?;
        require(
            rows.len() == 3 && rows.iter().zip(&keys).all(|(r, k)| &r.resource == k),
            "讨论代码事实读取不完整",
        )?;
        let guards = ReadSet::new(rows.iter().map(version).collect())?;
        let Some(work) = &rows[0].value else {
            return Ok((None, guards));
        };
        let work: RepositoryWorktree =
            serde_json::from_value(work.clone()).map_err(|e| err("storage", e.to_string()))?;
        let head: GitHead = serde_json::from_value(
            rows[1]
                .value
                .clone()
                .ok_or_else(|| err("blocked", "代码对象尚未核对"))?,
        )
        .map_err(|e| err("storage", e.to_string()))?;
        let metadata: Project = serde_json::from_value(
            rows[2]
                .value
                .clone()
                .ok_or_else(|| err("not_found", "项目不存在"))?,
        )
        .map_err(|e| err("storage", e.to_string()))?;
        require(
            work.project_id == project
                && work.owner_id == session.owner_id
                && head.project_id == project
                && metadata.execution_mode == "live",
            "讨论代码视图跨范围",
        )?;
        let location = CodeLocation {
            owner: session.owner_id.clone(),
            branch: work.branch,
            worktree: work.path,
        };
        Ok((
            Some(FrozenCode {
                context: GitContext {
                    project: project.into(),
                    workspace: metadata
                        .workspace_definition
                        .ok_or_else(|| err("blocked", "仓库定义缺失"))?,
                    source: location.clone(),
                    target: location,
                    origin: Origin::External,
                },
                revision: head.revision,
            }),
            guards,
        ))
    }
}

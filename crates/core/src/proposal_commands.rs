//! Proposals freeze a real completed discussion and an exact target revision.
//! Extraction and adoption never grant workflow or platform evidence.
use crate::{
    concurrency::*,
    material_commands::{material_parts, stable_object_id},
    work_owners::WorkOwnerFacts,
    *,
};

fn version(row: &ResourceSnapshot) -> ResourceVersion {
    ResourceVersion {
        resource: row.resource.clone(),
        revision: row.revision,
    }
}
fn read_proposal(
    store: &dyn EngineeringStore,
    id: &str,
) -> Result<(ResourceSnapshot, Proposal, ResourceSnapshot)> {
    let keys = [
        ResourceKey::new("proposal", id),
        ResourceKey::new("proposal_successor", id),
    ];
    let mut rows = store.read_resources(&keys)?;
    require(
        rows.len() == 2 && rows.iter().zip(&keys).all(|(r, k)| &r.resource == k),
        "提案原事实读取不完整",
    )?;
    let successor = rows.pop().unwrap();
    let original = rows.pop().unwrap();
    let proposal = serde_json::from_value(
        store.restore_runtime_value(
            original
                .value
                .as_ref()
                .ok_or_else(|| err("not_found", "提案不存在"))?,
        )?,
    )
    .map_err(|e| err("storage", e.to_string()))?;
    require(successor.revision == 0, "提案已有后继版本")?;
    Ok((original, proposal, successor))
}

impl Engine {
    pub(crate) fn native_proposal_command(
        &self,
        method: &str,
        c: &Command,
        fingerprint: &str,
        context: &InvocationContext,
    ) -> Result<Option<Value>> {
        if !method.starts_with("proposal.") {
            return Ok(None);
        }
        require(
            matches!(context.actor(), WorkflowActor::Human),
            "提案人工编辑和采用需要实际 Human 来源",
        )?;
        if let Some(result) = self.replay_sealed_command(&c.command_id, fingerprint)? {
            return Ok(Some(result));
        }
        match method {
            "proposal.reject" | "proposal.adopt" => self
                .native_proposal_decision(method, c, fingerprint, context)
                .map(Some),
            "proposal.create" | "proposal.extract" | "proposal.revise" => self
                .native_proposal_draft(method, c, fingerprint, context)
                .map(Some),
            _ => Err(err("method_not_found", "未知提案命令")),
        }
    }
    fn native_proposal_draft(
        &self,
        method: &str,
        c: &Command,
        fingerprint: &str,
        context: &InvocationContext,
    ) -> Result<Value> {
        let previous = if method == "proposal.revise" {
            Some(read_proposal(
                self.store.as_ref(),
                c.target
                    .as_deref()
                    .ok_or_else(|| err("invalid_params", "缺少原提案"))?,
            )?)
        } else {
            None
        };
        let mut read_set = ReadSet::default();
        if let Some((original, proposal, successor)) = &previous {
            require(
                proposal.status == ProposalStatus::Draft
                    && c.expected_revision == Some(original.revision),
                "提案版本或状态已变化",
            )?;
            read_set = read_set.merge([version(original), version(successor)])?;
        }
        let tid = previous
            .as_ref()
            .map(|(_, p, _)| p.turn_id.as_str())
            .or_else(|| c.payload["turn_id"].as_str())
            .ok_or_else(|| err("invalid_params", "需要来源 turn_id"))?;
        let source_key = ResourceKey::new(
            "proposal_turn",
            json!([tid, method == "proposal.extract"]).to_string(),
        );
        let source = self
            .store
            .read_resources(std::slice::from_ref(&source_key))?
            .into_iter()
            .next()
            .ok_or_else(|| err("storage", "提案原轮次读取不完整"))?;
        let turn = source
            .value
            .as_ref()
            .ok_or_else(|| err("not_found", "提案来源轮次不存在"))?;
        require(turn["status"] == "completed", "来源轮次尚未真实完成")?;
        let sid = text(turn, "session_id")?;
        let project = text(turn, "project_id")?;
        context.require_project(&project)?;
        let keys = [
            ResourceKey::new("session", &sid),
            ResourceKey::new("session_formal", &sid),
        ];
        let rows = self.store.read_resources(&keys)?;
        require(
            rows.len() == 2 && rows.iter().zip(&keys).all(|(r, k)| &r.resource == k),
            "提案来源会话读取不完整",
        )?;
        require(
            rows[1].revision == 0,
            "工作流产物由步骤输出合同接收，请在工作流中处理",
        )?;
        let session: Session = serde_json::from_value(
            self.store.restore_runtime_value(
                rows[0]
                    .value
                    .as_ref()
                    .ok_or_else(|| err("not_found", "原讨论会话缺失"))?,
            )?,
        )
        .map_err(|e| err("storage", e.to_string()))?;
        require(
            session.project_id.as_deref() == Some(project.as_str()) && session.scope.is_some(),
            "提案来源跨项目或不属于讨论",
        )?;
        read_set =
            read_set.merge(std::iter::once(version(&source)).chain(rows.iter().map(version)))?;
        let content: ProposalContent = if method == "proposal.extract" {
            serde_json::from_str(turn["assistant"].as_str().unwrap_or("").trim())
        } else {
            serde_json::from_value(c.payload["content"].clone())
        }
        .map_err(|e| err("invalid_proposal", e.to_string()))?;
        require(!content.title.trim().is_empty(), "材料标题不能为空")?;
        let catalog = self.store.configuration_snapshot()?;
        let delivery = crate::governance::reference(
            &catalog.state,
            &content.delivery_ref,
            "delivery_type",
            &Scope::project(&project),
        )?;
        let definition: DeliveryTypeDefinition = delivery.decode()?;
        require(
            !matches!(
                definition.producer,
                Some(DeliveryProducer::Platform { .. } | DeliveryProducer::Workflow)
            ),
            "提案不能冒充平台或子流程产出",
        )?;
        self.validate_configured_value(&definition, &content.value)?;
        let (owner_id, owner_guard) = self.session_work_owner(&session)?;
        let mut owner = WorkOwnerFacts::read(self.store.as_ref(), &owner_id)?;
        owner.read_set = owner.read_set.merge([owner_guard])?;
        require(owner.project == project, "提案目标不属于来源项目")?;
        read_set = read_set
            .merge(owner.read_set.versions().iter().cloned())?
            .merge(
                self.material_input_witnesses(&project, &content.input_refs)?
                    .versions()
                    .iter()
                    .cloned(),
            )?
            .merge([ResourceVersion {
                resource: ResourceKey::new("catalog_clock", "configuration"),
                revision: catalog.revision,
            }])?;
        let body =
            serde_json::to_string(&content).map_err(|e| err("serialization", e.to_string()))?;
        let seal = ArtifactSeal::prepare(
            stable_object_id(self.store.as_ref(), &c.command_id, "proposal-artifact")?,
            &owner.material,
            "proposals",
            &body,
            Origin::Human,
        )?;
        let pid = stable_object_id(self.store.as_ref(), &c.command_id, "proposal")?;
        let proposal = Proposal {
            id: pid.clone(),
            project_id: project.clone(),
            session_id: sid,
            turn_id: tid.into(),
            schema_version: 2,
            dependencies: content.input_refs.clone(),
            content,
            target: Some(owner_id),
            expected_revision: Some(owner.owner.revision),
            status: ProposalStatus::Draft,
            artifact_id: Some(seal.artifact.id.clone()),
            adopted_objects: vec![],
            supersedes: previous.map(|(_, p, _)| p.id),
            revision: 1,
        };
        let key = ResourceKey::new("proposal", &pid);
        let provenance = context.prepare_record(method, c);
        read_set = read_set.merge([
            ResourceVersion {
                resource: key.clone(),
                revision: 0,
            },
            ResourceVersion {
                resource: provenance.resource.clone(),
                revision: 0,
            },
        ])?;
        self.seal_command_changes(
            PreparedChangeSet {
                command_id: c.command_id.clone(),
                fingerprint: fingerprint.into(),
                read_set,
                changes: vec![
                    ResourceChange {
                        resource: key,
                        value: self.store.encode_runtime_value(&json!(proposal))?,
                    },
                    provenance,
                ],
                result: json!({"id":pid}),
                time: now() as i64,
            },
            seal,
            &body,
            Scope::project(&project),
        )
    }
    fn native_proposal_decision(
        &self,
        method: &str,
        c: &Command,
        fingerprint: &str,
        context: &InvocationContext,
    ) -> Result<Value> {
        let target = c
            .target
            .as_deref()
            .ok_or_else(|| err("invalid_params", "缺少原提案"))?;
        let (original, mut proposal, successor) = read_proposal(self.store.as_ref(), target)?;
        context.require_project(&proposal.project_id)?;
        require(
            proposal.status == ProposalStatus::Draft
                && c.expected_revision == Some(original.revision),
            "提案状态或原版本已变化",
        )?;
        let provenance = context.prepare_record(method, c);
        let mut read_set = ReadSet::new(vec![
            version(&original),
            version(&successor),
            ResourceVersion {
                resource: provenance.resource.clone(),
                revision: 0,
            },
        ])?;
        proposal.revision = original.revision + 1;
        if method == "proposal.reject" {
            proposal.status = ProposalStatus::Rejected;
            return self
                .store
                .commit_changes(&PreparedChangeSet {
                    command_id: c.command_id.clone(),
                    fingerprint: fingerprint.into(),
                    read_set,
                    changes: vec![
                        ResourceChange {
                            resource: original.resource,
                            value: self.store.encode_runtime_value(&json!(proposal))?,
                        },
                        provenance,
                    ],
                    result: json!({"id":target}),
                    time: now() as i64,
                })
                .map(|r| r.result);
        }
        let owner = WorkOwnerFacts::read(
            self.store.as_ref(),
            proposal
                .target
                .as_deref()
                .ok_or_else(|| err("scope_violation", "提案目标归属缺失"))?,
        )?;
        require(owner.project == proposal.project_id, "提案目标跨项目")?;
        owner.require_revision(proposal.expected_revision)?;
        owner.require_editable()?;
        let catalog = self.store.configuration_snapshot()?;
        let definition: DeliveryTypeDefinition = catalog
            .state
            .configuration(&proposal.content.delivery_ref)?
            .decode()?;
        self.validate_configured_value(&definition, &proposal.content.value)?;
        read_set = read_set
            .merge(owner.read_set.versions().iter().cloned())?
            .merge(
                self.material_input_witnesses(&owner.project, &proposal.content.input_refs)?
                    .versions()
                    .iter()
                    .cloned(),
            )?
            .merge([ResourceVersion {
                resource: ResourceKey::new("source", &proposal.content.delivery_ref),
                revision: 1,
            }])?;
        let body = proposal
            .content
            .value
            .as_str()
            .map(str::to_owned)
            .unwrap_or(
                serde_json::to_string_pretty(&proposal.content.value)
                    .map_err(|e| err("serialization", e.to_string()))?,
            );
        let seal = ArtifactSeal::prepare(
            stable_object_id(self.store.as_ref(), &c.command_id, "adoption-artifact")?,
            &owner.material,
            "materials",
            &body,
            Origin::Human,
        )?;
        let did = stable_object_id(self.store.as_ref(), &c.command_id, "adopted-document")?;
        let key = ResourceKey::new("document", &did);
        let (project_id, feature_id, task_id) = material_parts(&owner.material);
        let document = Document {
            id: did.clone(),
            project_id,
            feature_id,
            task_id,
            kind: proposal.content.delivery_ref.clone(),
            title: proposal.content.title.clone(),
            body: body.clone(),
            version: 1,
            supersedes: None,
            input_refs: proposal.content.input_refs.clone(),
            origin: Origin::Human,
            artifact_id: Some(seal.artifact.id.clone()),
        };
        proposal.status = ProposalStatus::Accepted;
        proposal.adopted_objects = vec![did.clone()];
        read_set = read_set.merge([ResourceVersion {
            resource: key.clone(),
            revision: 0,
        }])?;
        let changes = vec![
            owner.advance()?,
            ResourceChange {
                resource: key,
                value: self.store.encode_runtime_value(&json!(document))?,
            },
            ResourceChange {
                resource: original.resource,
                value: self.store.encode_runtime_value(&json!(proposal))?,
            },
            provenance,
        ];
        self.seal_command_changes(
            PreparedChangeSet {
                command_id: c.command_id.clone(),
                fingerprint: fingerprint.into(),
                read_set,
                changes,
                result: json!({"id":target,"objects":[did]}),
                time: now() as i64,
            },
            seal,
            &body,
            Scope::project(&owner.project),
        )
    }
}

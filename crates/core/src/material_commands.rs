//! Human material and manual context edits use explicit ownership facts. File
//! sealing is outside SQL and retains the original finite admission plan.
use crate::{concurrency::*, work_owners::WorkOwnerFacts, *};

pub(crate) fn stable_object_id(
    store: &dyn EngineeringStore,
    command: &str,
    kind: &str,
) -> Result<Id> {
    uuid::Uuid::parse_str(
        &configuration_hash(&(
            "material-object/v1",
            store.namespace(),
            store.root(),
            command,
            kind,
        ))[..32],
    )
    .map(|id| id.to_string())
    .map_err(|e| err("storage", e.to_string()))
}
pub(crate) fn material_parts(owner: &MaterialOwner) -> (Option<Id>, Option<Id>, Option<Id>) {
    match owner {
        MaterialOwner::Global => (None, None, None),
        MaterialOwner::Project { id } => (Some(id.clone()), None, None),
        MaterialOwner::Feature { id } => (None, Some(id.clone()), None),
        MaterialOwner::Task { feature, id } => (None, Some(feature.clone()), Some(id.clone())),
    }
}

impl Engine {
    pub(crate) fn native_material_command(
        &self,
        method: &str,
        c: &Command,
        fingerprint: &str,
        context: &InvocationContext,
    ) -> Result<Option<Value>> {
        if !matches!(
            method,
            "document.revise" | "context.update" | "snapshot.save"
        ) {
            return Ok(None);
        }
        require(
            matches!(context.actor(), WorkflowActor::Human),
            "人工材料编辑需要实际 Human 来源",
        )?;
        if matches!(method, "document.revise" | "snapshot.save") {
            if let Some(result) = self.replay_sealed_command(&c.command_id, fingerprint)? {
                return Ok(Some(result));
            }
        } else if let Some(receipt) = self.store.replay_commit(&c.command_id, fingerprint)? {
            return Ok(Some(receipt.result));
        }
        let target = c
            .target
            .as_deref()
            .ok_or_else(|| err("invalid_params", "缺少材料或背景目标"))?;
        if method == "context.update" {
            let key = ResourceKey::new("context", target);
            let mut rows = self.store.read_resources(std::slice::from_ref(&key))?;
            require(
                rows.len() == 1 && rows[0].resource == key,
                "背景原事实读取不完整",
            )?;
            let original = rows.remove(0);
            if c.expected_revision != Some(original.revision) {
                return Err(err(
                    "revision_conflict",
                    "背景原版本已变化，请保留草稿并刷新",
                ));
            }
            let raw = original
                .value
                .as_ref()
                .ok_or_else(|| err("not_found", "背景不存在"))?;
            let owner = WorkOwnerFacts::read(self.store.as_ref(), &text(raw, "owner_id")?)?;
            context.require_project(&owner.project)?;
            owner.require_editable()?;
            let content = text(&c.payload, "content")?;
            let mut next = raw.clone();
            next["content"] = json!(content);
            next["revision"] = json!(original.revision + 1);
            let event = ResourceKey::new(
                "event_entry",
                stable_object_id(self.store.as_ref(), &c.command_id, "context-event")?,
            );
            let audit = ResourceChange {
                resource: event.clone(),
                value: json!({"target":target,"kind":"context.revised","detail":json!({"previous_revision":original.revision,"revision":original.revision+1,"previous":raw["content"],"content":content}).to_string(),"time":now()}),
            };
            let provenance = context.prepare_record(method, c);
            let read_set = owner.read_set.merge([
                ResourceVersion {
                    resource: key.clone(),
                    revision: original.revision,
                },
                ResourceVersion {
                    resource: event,
                    revision: 0,
                },
                ResourceVersion {
                    resource: provenance.resource.clone(),
                    revision: 0,
                },
            ])?;
            return self
                .store
                .commit_changes(&PreparedChangeSet {
                    command_id: c.command_id.clone(),
                    fingerprint: fingerprint.into(),
                    read_set,
                    changes: vec![
                        ResourceChange {
                            resource: key,
                            value: next,
                        },
                        audit,
                        provenance,
                    ],
                    result: json!({"id":target}),
                    time: now() as i64,
                })
                .map(|r| Some(r.result));
        }
        let owner = WorkOwnerFacts::read(self.store.as_ref(), target)?;
        context.require_project(&owner.project)?;
        owner.require_revision(c.expected_revision)?;
        owner.require_editable()?;
        if method == "snapshot.save" {
            return self
                .native_manual_snapshot(c, fingerprint, context, &owner)
                .map(Some);
        }
        let kind = text(&c.payload, "kind")?;
        let body = text(&c.payload, "body")?;
        let title = text(&c.payload, "title")?;
        let inputs: Vec<Id> =
            serde_json::from_value(c.payload.get("input_refs").cloned().unwrap_or(json!([])))
                .map_err(|e| err("invalid_params", e.to_string()))?;
        let mut read_set = owner.read_set.merge(
            self.material_input_witnesses(&owner.project, &inputs)?
                .versions()
                .iter()
                .cloned(),
        )?;
        let (project, feature, task) = material_parts(&owner.material);
        let previous = opt(&c.payload, "supersedes");
        let version = if let Some(previous) = &previous {
            let keys = [
                ResourceKey::new("document", previous),
                ResourceKey::new("document_successor", previous),
            ];
            let rows = self.store.read_resources(&keys)?;
            require(
                rows.len() == 2 && rows.iter().zip(&keys).all(|(r, k)| &r.resource == k),
                "前版材料事实读取不完整",
            )?;
            let old: Document = serde_json::from_value(
                self.store.restore_runtime_value(
                    rows[0]
                        .value
                        .as_ref()
                        .ok_or_else(|| err("not_found", "前版材料缺失"))?,
                )?,
            )
            .map_err(|e| err("storage", e.to_string()))?;
            require(
                old.project_id == project
                    && old.feature_id == feature
                    && old.task_id == task
                    && old.kind == kind
                    && rows[1].revision == 0,
                "前版材料归属、类型不符或已有后继",
            )?;
            read_set = read_set.merge(rows.iter().map(|r| ResourceVersion {
                resource: r.resource.clone(),
                revision: r.revision,
            }))?;
            old.version
                .checked_add(1)
                .ok_or_else(|| err("resource_exhausted", "材料版本已用尽"))?
        } else {
            1
        };
        let did = stable_object_id(self.store.as_ref(), &c.command_id, "document")?;
        let seal = ArtifactSeal::prepare(
            stable_object_id(self.store.as_ref(), &c.command_id, "artifact")?,
            &owner.material,
            "materials",
            &body,
            Origin::Human,
        )?;
        let document = Document {
            id: did.clone(),
            project_id: project,
            feature_id: feature,
            task_id: task,
            kind,
            title,
            body: body.clone(),
            version,
            supersedes: previous,
            input_refs: inputs,
            origin: Origin::Human,
            artifact_id: Some(seal.artifact.id.clone()),
        };
        let key = ResourceKey::new("document", &did);
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
        let changes = vec![
            owner.advance()?,
            ResourceChange {
                resource: key,
                value: self.store.encode_runtime_value(&json!(document))?,
            },
            provenance,
        ];
        self.seal_command_changes(
            PreparedChangeSet {
                command_id: c.command_id.clone(),
                fingerprint: fingerprint.into(),
                read_set,
                changes,
                result: json!({"id":did}),
                time: now() as i64,
            },
            seal,
            &body,
            owner.scope,
        )
        .map(Some)
    }

    fn native_manual_snapshot(
        &self,
        c: &Command,
        fingerprint: &str,
        context: &InvocationContext,
        owner: &WorkOwnerFacts,
    ) -> Result<Value> {
        let context_key = ResourceKey::new("owner_context", &owner.id);
        let mut keys = vec![context_key.clone()];
        if c.payload.get("documents").is_none() {
            keys.push(ResourceKey::new(
                "owner_materials",
                serde_json::to_string(&owner.material)
                    .map_err(|e| err("serialization", e.to_string()))?,
            ));
        }
        let rows = self.store.read_resources(&keys)?;
        require(
            rows.len() == keys.len() && rows.iter().zip(&keys).all(|(r, k)| &r.resource == k),
            "快照原背景和材料读取不完整",
        )?;
        let mut read_set = owner.read_set.merge(rows.iter().map(|r| ResourceVersion {
            resource: r.resource.clone(),
            revision: r.revision,
        }))?;
        let documents: Vec<Id> = serde_json::from_value(
            c.payload
                .get("documents")
                .cloned()
                .unwrap_or_else(|| rows[1].value.clone().unwrap_or(json!([]))),
        )
        .map_err(|e| err("invalid_params", e.to_string()))?;
        require(documents.len() <= 2048, "一次快照最多选择 2048 个材料")?;
        let selected = documents
            .iter()
            .map(|id| ResourceKey::new("document_ownership", id))
            .collect::<Vec<_>>();
        require(
            selected
                .iter()
                .collect::<std::collections::BTreeSet<_>>()
                .len()
                == selected.len(),
            "快照材料重复",
        )?;
        let inputs = self.store.read_resources(&selected)?;
        require(
            inputs.len() == selected.len()
                && inputs.iter().zip(&selected).all(|(r, k)| &r.resource == k),
            "快照所选材料读取不完整",
        )?;
        let (project_id, feature_id, task_id) = material_parts(&owner.material);
        for row in &inputs {
            let value = row
                .value
                .as_ref()
                .ok_or_else(|| err("not_found", "快照材料不存在"))?;
            require(
                value["project_id"] == json!(project_id)
                    && value["feature_id"] == json!(feature_id)
                    && (value["task_id"].is_null() || value["task_id"] == json!(task_id)),
                "快照材料不属于目标工作范围",
            )?;
        }
        read_set = read_set.merge(inputs.iter().map(|r| ResourceVersion {
            resource: r.resource.clone(),
            revision: r.revision,
        }))?;
        let sid = stable_object_id(self.store.as_ref(), &c.command_id, "manual-snapshot")?;
        let snapshot = Snapshot {
            id: sid.clone(),
            project_id,
            feature_id,
            task_id,
            kind: "manual".into(),
            content: rows[0]
                .value
                .as_ref()
                .and_then(|v| v["content"].as_str())
                .unwrap_or_default()
                .into(),
            documents,
            inherited_from: None,
            capture: SnapshotCapture {
                step_execution_ids: vec![],
                sessions: vec![],
                captured_at: Some(now()),
                reason: opt(&c.payload, "reason")
                    .unwrap_or_else(|| "User selected materials".into()),
            },
            runtime_bindings: vec![],
            execution_instances: vec![],
        };
        let body = serde_json::to_string_pretty(&snapshot)
            .map_err(|e| err("serialization", e.to_string()))?;
        let seal = ArtifactSeal::prepare(
            stable_object_id(self.store.as_ref(), &c.command_id, "snapshot-artifact")?,
            &owner.material,
            "snapshots",
            &body,
            Origin::Human,
        )?;
        let key = ResourceKey::new("snapshot", &sid);
        let provenance = context.prepare_record("snapshot.save", c);
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
                        value: self.store.encode_runtime_value(&json!(snapshot))?,
                    },
                    provenance,
                ],
                result: json!({"id":sid}),
                time: now() as i64,
            },
            seal,
            &body,
            owner.scope.clone(),
        )
    }
}

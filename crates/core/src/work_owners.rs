//! Explicit ownership facts shared by material and conversation use cases.
use crate::{concurrency::*, *};

pub(crate) struct WorkOwnerFacts {
    pub owner: ResourceSnapshot,
    pub project: Id,
    pub material: MaterialOwner,
    pub scope: Scope,
    pub read_set: ReadSet,
    pub parent: Option<ResourceSnapshot>,
}
impl WorkOwnerFacts {
    pub fn read(store: &dyn EngineeringStore, id: &str) -> Result<Self> {
        let keys = [
            ResourceKey::new("project", id),
            ResourceKey::new("feature", id),
            ResourceKey::new("task", id),
        ];
        let rows = store.read_resources(&keys)?;
        require(
            rows.len() == 3 && rows.iter().zip(&keys).all(|(r, k)| &r.resource == k),
            "工作归属事实读取不完整",
        )?;
        let present = rows
            .iter()
            .filter(|r| r.value.is_some())
            .collect::<Vec<_>>();
        require(present.len() == 1, "工作对象不存在或身份有歧义")?;
        let owner = (*present[0]).clone();
        let raw = owner.value.as_ref().unwrap();
        let mut guards = rows
            .iter()
            .map(|r| ResourceVersion {
                resource: r.resource.clone(),
                revision: r.revision,
            })
            .collect::<Vec<_>>();
        let (project, material, scope, parent) = match owner.resource.kind.as_str() {
            "project" => (
                id.to_owned(),
                MaterialOwner::Project { id: id.into() },
                Scope::project(id),
                None,
            ),
            "feature" => {
                let project = text(raw, "project_id")?;
                (
                    project.clone(),
                    MaterialOwner::Feature { id: id.into() },
                    Scope::feature(&project, id),
                    None,
                )
            }
            "task" => {
                let feature = text(raw, "feature_id")?;
                let parent_key = ResourceKey::new("feature", &feature);
                let mut rows = store.read_resources(std::slice::from_ref(&parent_key))?;
                require(
                    rows.len() == 1 && rows[0].resource == parent_key,
                    "任务父对象读取不完整",
                )?;
                let parent = rows.remove(0);
                let project = text(
                    parent
                        .value
                        .as_ref()
                        .ok_or_else(|| err("not_found", "任务父对象缺失"))?,
                    "project_id",
                )?;
                guards.push(ResourceVersion {
                    resource: parent.resource.clone(),
                    revision: parent.revision,
                });
                (
                    project.clone(),
                    MaterialOwner::Task {
                        feature: feature.clone(),
                        id: id.into(),
                    },
                    Scope::feature(&project, &feature),
                    Some(parent),
                )
            }
            _ => return Err(err("invalid_scope", "无效工作归属")),
        };
        require(
            store.project_id().is_none_or(|id| id == project),
            "工作对象不属于当前项目库",
        )?;
        let live_key = ResourceKey::new("project_live", &project);
        let live = store
            .read_resources(std::slice::from_ref(&live_key))?
            .into_iter()
            .next()
            .ok_or_else(|| err("storage", "项目运行模式读取不完整"))?;
        require(live.revision == 1, "历史项目只读")?;
        guards.push(ResourceVersion {
            resource: live_key,
            revision: live.revision,
        });
        Ok(Self {
            owner,
            project,
            material,
            scope,
            read_set: ReadSet::new(guards)?,
            parent,
        })
    }
    pub fn require_revision(&self, expected: Option<u64>) -> Result<()> {
        if expected == Some(self.owner.revision) {
            Ok(())
        } else {
            Err(err(
                "revision_conflict",
                "工作原版本已改变，请保留草稿并刷新",
            ))
        }
    }
    pub fn require_editable(&self) -> Result<()> {
        require(
            self.owner
                .value
                .as_ref()
                .is_some_and(|v| v["status"] != "completed")
                && self
                    .parent
                    .as_ref()
                    .is_none_or(|p| p.value.as_ref().is_some_and(|v| v["status"] != "completed")),
            "已完成工作已冻结",
        )
    }
    pub fn advance(&self) -> Result<ResourceChange> {
        let mut value = self
            .owner
            .value
            .clone()
            .ok_or_else(|| err("not_found", "原工作对象缺失"))?;
        value["revision"] = json!(self.owner.revision + 1);
        Ok(ResourceChange {
            resource: self.owner.resource.clone(),
            value,
        })
    }
}

impl Engine {
    pub(crate) fn material_input_witnesses(&self, project: &str, ids: &[Id]) -> Result<ReadSet> {
        require(ids.len() <= 256, "材料输入引用超出预算")?;
        let mut keys = Vec::new();
        let mut unique = std::collections::BTreeSet::new();
        for id in ids {
            require(unique.insert(id), "材料输入引用重复")?;
            keys.push(ResourceKey::new("document_ownership", id));
            keys.push(ResourceKey::new("workflow_output_scope", id));
        }
        let rows = self.store.read_resources(&keys)?;
        require(
            rows.len() == keys.len() && rows.iter().zip(&keys).all(|(r, k)| &r.resource == k),
            "材料输入来源读取不完整",
        )?;
        let mut guards = Vec::new();
        for pair in rows.as_chunks::<2>().0 {
            let valid = (if let Some(doc) = &pair[0].value {
                if doc["project_id"] == project {
                    true
                } else if let Some(feature) = doc["feature_id"].as_str() {
                    let parent = self
                        .store
                        .read_resources(&[ResourceKey::new("feature", feature)])?
                        .into_iter()
                        .next()
                        .ok_or_else(|| err("storage", "材料归属父对象读取不完整"))?;
                    let allowed = parent
                        .value
                        .as_ref()
                        .is_some_and(|f| f["project_id"] == project);
                    guards.push(ResourceVersion {
                        resource: parent.resource,
                        revision: parent.revision,
                    });
                    allowed
                } else {
                    false
                }
            } else {
                false
            }) || pair[1].value.as_ref().is_some_and(|r| {
                serde_json::from_value::<Scope>(r["scope"].clone())
                    .is_ok_and(|s| s.project_id() == Some(project))
            });
            require(valid, "材料输入必须属于同一项目")?;
            guards.extend(pair.iter().map(|r| ResourceVersion {
                resource: r.resource.clone(),
                revision: r.revision,
            }));
        }
        ReadSet::default().merge(guards)
    }
}

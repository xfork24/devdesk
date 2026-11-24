//! Durable progress for a committed full template reset.
use crate::{
    EngineeringStore,
    concurrency::{
        PreparedChangeSet, ReadSet, ResourceChange, ResourceKey, ResourceSnapshot, ResourceVersion,
    },
    now,
    platform_facts::one,
};
use devdesk_domain::{Result, configuration_hash, err, require};
use serde::{Deserialize, Serialize};
use serde_json::{Value, json};

pub const ACTIVE_SETTING: &str = "template.reset.active";

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct TemplateResetJob {
    pub command_id: String,
    pub generation: u64,
    pub accepted_at: u64,
    pub completed_at: Option<u64>,
    pub targets: Vec<TemplateResetTarget>,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct TemplateResetTarget {
    pub id: String,
    pub applied_at: Option<u64>,
    pub error_code: Option<String>,
}

fn key(command_id: &str) -> ResourceKey {
    ResourceKey::new(
        "setting",
        format!("template.reset.job.{}", configuration_hash(&command_id)),
    )
}

fn identity(store: &dyn EngineeringStore) -> Result<ResourceSnapshot> {
    one(store, "setting", ACTIVE_SETTING)
}

fn decode(snapshot: &ResourceSnapshot) -> Result<Option<TemplateResetJob>> {
    snapshot
        .value
        .clone()
        .map(|value| serde_json::from_value(value).map_err(|e| err("storage", e.to_string())))
        .transpose()
}

impl TemplateResetJob {
    pub fn read(store: &dyn EngineeringStore, command_id: Option<&str>) -> Result<Option<Self>> {
        let key = match command_id {
            Some(command) => key(command),
            None => {
                let active = identity(store)?;
                let Some(id) = active.value.as_ref().and_then(Value::as_str) else {
                    return Ok(None);
                };
                ResourceKey::new("setting", id)
            }
        };
        decode(&one(store, "setting", &key.id)?)
    }

    pub(crate) fn active_guard(store: &dyn EngineeringStore) -> Result<ResourceSnapshot> {
        let active = identity(store)?;
        if let Some(id) = active.value.as_ref().and_then(Value::as_str) {
            let running = decode(&one(store, "setting", id)?)?;
            if running.is_some_and(|job| job.completed_at.is_none()) {
                return Err(err("operation_in_progress", "原模板重置仍有项目等待应用"));
            }
        }
        Ok(active)
    }

    pub(crate) fn start(
        store: &dyn EngineeringStore,
        active: ResourceSnapshot,
        command: &str,
        generation: u64,
        targets: Vec<String>,
    ) -> Result<([ResourceVersion; 2], [ResourceChange; 2])> {
        let accepted_at = now();
        let job = Self {
            command_id: command.into(),
            generation,
            accepted_at,
            completed_at: targets.is_empty().then_some(accepted_at),
            targets: targets
                .into_iter()
                .map(|id| TemplateResetTarget {
                    id,
                    applied_at: None,
                    error_code: None,
                })
                .collect(),
        };
        let job_key = key(command);
        require(
            one(store, "setting", &job_key.id)?.value.is_none(),
            "模板重置已经受理",
        )?;
        Ok((
            [
                ResourceVersion {
                    resource: active.resource.clone(),
                    revision: active.revision,
                },
                ResourceVersion {
                    resource: job_key.clone(),
                    revision: 0,
                },
            ],
            [
                ResourceChange {
                    resource: active.resource,
                    value: json!(job_key.id),
                },
                ResourceChange {
                    resource: job_key,
                    value: json!(job),
                },
            ],
        ))
    }

    pub fn report(&self) -> Value {
        let applied = self
            .targets
            .iter()
            .filter(|target| target.applied_at.is_some())
            .count();
        let retryable = self
            .targets
            .iter()
            .filter(|target| target.error_code.is_some())
            .count();
        let phase = if self.completed_at.is_some() {
            "completed"
        } else if retryable > 0 {
            "retryable_failure"
        } else if applied > 0 {
            "applying"
        } else {
            "global_committed"
        };
        json!({
            "command_id":self.command_id,"generation":self.generation,"phase":phase,
            "accepted_at":self.accepted_at,"completed_at":self.completed_at,
            "applied":applied,"total":self.targets.len(),"targets":self.targets
        })
    }

    pub fn record_target(
        &self,
        store: &dyn EngineeringStore,
        target: &str,
        error_code: Option<&str>,
    ) -> Result<()> {
        let resource = key(&self.command_id);
        for attempt in 0..5 {
            let snapshot = one(store, "setting", &resource.id)?;
            let mut job = decode(&snapshot)?.ok_or_else(|| err("storage", "模板重置工作项缺失"))?;
            require(job.generation == self.generation, "模板重置工作项代次改变")?;
            let target = job
                .targets
                .iter_mut()
                .find(|entry| entry.id == target)
                .ok_or_else(|| err("storage", "模板重置工作项范围改变"))?;
            if target.applied_at.is_some()
                || (error_code.is_some() && target.error_code.as_deref() == error_code)
            {
                return Ok(());
            }
            let updated_at = now();
            target.error_code = error_code.map(str::to_owned);
            if error_code.is_none() {
                target.applied_at = Some(updated_at);
            }
            if job.targets.iter().all(|item| item.applied_at.is_some()) {
                job.completed_at = Some(updated_at);
            }
            let fingerprint = configuration_hash(&(
                "template-reset-target/v1",
                &resource,
                snapshot.revision,
                &job,
            ));
            let changes = PreparedChangeSet {
                command_id: format!("template-reset-progress:{fingerprint}"),
                fingerprint,
                read_set: ReadSet::new(vec![ResourceVersion {
                    resource: resource.clone(),
                    revision: snapshot.revision,
                }])?,
                changes: vec![ResourceChange {
                    resource: resource.clone(),
                    value: json!(job),
                }],
                result: json!({"ok":true}),
                time: updated_at as i64,
            };
            match store.commit_changes(&changes) {
                Err(error) if error.code == "revision_conflict" && attempt < 4 => {}
                result => return result.map(|_| ()),
            }
        }
        unreachable!()
    }
}

//! Storage-independent, resource-level optimistic commit contracts.
use devdesk_domain::{Result, err};
use serde::{Deserialize, Serialize};
use serde_json::Value;
use std::collections::BTreeMap;

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum StoreNamespace {
    Configuration,
    Engineering { project_id: Option<String> },
}

#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ResourceKey {
    pub kind: String,
    pub id: String,
}

impl ResourceKey {
    pub fn new(kind: impl Into<String>, id: impl Into<String>) -> Self {
        Self {
            kind: kind.into(),
            id: id.into(),
        }
    }

    pub fn validate(&self) -> Result<()> {
        if self.kind.is_empty()
            || self.id.is_empty()
            || self.kind.len() > 128
            || self.id.len() > 1024
            || !self
                .kind
                .bytes()
                .all(|b| b.is_ascii_alphanumeric() || matches!(b, b'_' | b'.' | b'-'))
        {
            return Err(err("invalid_params", "无效的资源身份"));
        }
        Ok(())
    }
}

/// Revision zero is an absence assertion, never an existing object's revision.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ResourceVersion {
    pub resource: ResourceKey,
    pub revision: u64,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ResourceSnapshot {
    pub resource: ResourceKey,
    pub revision: u64,
    pub value: Option<Value>,
}

/// Every object or predicate used to authorize a change belongs in this set.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(transparent)]
pub struct ReadSet(Vec<ResourceVersion>);

impl ReadSet {
    pub fn new(versions: Vec<ResourceVersion>) -> Result<Self> {
        let mut unique = BTreeMap::new();
        for version in versions {
            version.resource.validate()?;
            if version.revision > i64::MAX as u64 {
                return Err(err("invalid_params", "资源版本超出存储范围"));
            }
            if unique.insert(version.resource, version.revision).is_some() {
                return Err(err("invalid_params", "资源前置条件重复"));
            }
        }
        Ok(Self(
            unique
                .into_iter()
                .map(|(resource, revision)| ResourceVersion { resource, revision })
                .collect(),
        ))
    }

    pub fn versions(&self) -> &[ResourceVersion] {
        &self.0
    }

    pub fn validate(&self) -> Result<()> {
        Self::new(self.0.clone()).map(|_| ())
    }

    pub fn require(&self, key: &ResourceKey) -> Result<u64> {
        self.0
            .iter()
            .find(|version| &version.resource == key)
            .map(|version| version.revision)
            .ok_or_else(|| err("precondition_required", "缺少资源版本前置条件"))
    }
}

/// Storage-maintained tokens for collection predicates, never a mutable project lock.
/// Callers still read the actual selected facts from the same committed snapshot.
pub fn repository_collection(project: &str) -> ResourceKey {
    ResourceKey::new("collection", format!("repository:{project}"))
}
pub fn feature_collection(project: &str) -> ResourceKey {
    ResourceKey::new("collection", format!("features:{project}"))
}

/// Values are prepared by trusted core use cases; this is not an RPC envelope.
#[derive(Debug, Clone)]
pub struct ResourceChange {
    pub resource: ResourceKey,
    pub value: Value,
}

#[derive(Debug, Clone)]
pub struct PreparedChangeSet {
    pub command_id: String,
    pub fingerprint: String,
    pub read_set: ReadSet,
    pub changes: Vec<ResourceChange>,
    pub result: Value,
    pub time: i64,
}

impl PreparedChangeSet {
    pub fn validate(&self) -> Result<()> {
        if self.command_id.is_empty() || self.fingerprint.is_empty() {
            return Err(err("invalid_params", "提交需要明确的命令身份与指纹"));
        }
        self.read_set.validate()?;
        let mut changed = BTreeMap::new();
        for change in &self.changes {
            change.resource.validate()?;
            if self.read_set.require(&change.resource)? == i64::MAX as u64 {
                return Err(err("resource_exhausted", "资源版本已用尽，禁止溢出更新"));
            }
            if changed.insert(&change.resource, ()).is_some() {
                return Err(err("invalid_params", "同一资源不能重复提交变化"));
            }
        }
        if self.changes.is_empty() {
            return Err(err("invalid_params", "提交变化不能为空"));
        }
        Ok(())
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CommitReceipt {
    pub result: Value,
    pub versions: Vec<ResourceVersion>,
    pub cursor: u64,
}

/// Internal admission result. A replay returns the original durable receipt but
/// must not grant another attempt to perform an external side effect.
#[derive(Debug, Clone)]
pub struct CommitOutcome {
    pub receipt: CommitReceipt,
    pub applied: bool,
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn missing_write_guard_and_duplicate_assertions_are_rejected() {
        let resource = ResourceKey::new("settings", "global");
        let condition = ResourceVersion {
            resource: resource.clone(),
            revision: 0,
        };
        assert!(ReadSet::new(vec![condition.clone(), condition]).is_err());
        let prepared = PreparedChangeSet {
            command_id: "save".into(),
            fingerprint: "original-actor-and-parameters".into(),
            read_set: ReadSet::default(),
            changes: vec![ResourceChange {
                resource,
                value: json!({"limit":4}),
            }],
            result: json!({}),
            time: 1,
        };
        assert_eq!(
            prepared.validate().unwrap_err().code,
            "precondition_required"
        );
    }

    #[test]
    fn deserialized_read_sets_must_still_pass_validation() {
        let read_set: ReadSet = serde_json::from_value(json!([
            {"resource":{"kind":"settings","id":"global"},"revision":1},
            {"resource":{"kind":"settings","id":"global"},"revision":2}
        ]))
        .unwrap();
        assert!(read_set.validate().is_err());
        assert!(
            ResourceKey::new("settings; DROP TABLE settings", "global")
                .validate()
                .is_err()
        );
    }

    #[test]
    fn unrelated_resources_keep_independent_versions_and_absence_guards() {
        let first = ResourceKey::new("feature", "a");
        let second = ResourceKey::new("feature", "b");
        let read_set = ReadSet::new(vec![
            ResourceVersion {
                resource: first.clone(),
                revision: 3,
            },
            ResourceVersion {
                resource: second.clone(),
                revision: 0,
            },
        ])
        .unwrap();
        assert_eq!(read_set.require(&first).unwrap(), 3);
        assert_eq!(read_set.require(&second).unwrap(), 0);
        assert_eq!(
            read_set
                .require(&ResourceKey::new("feature", "c"))
                .unwrap_err()
                .code,
            "precondition_required"
        );
    }
}

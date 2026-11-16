//! Disposable query projections. Exact source revisions remain the authority for editing and execution.
use crate::*;
use std::{
    collections::{BTreeMap, VecDeque},
    sync::atomic::{AtomicUsize, Ordering},
};

static MEMO_BYTES: AtomicUsize = AtomicUsize::new(0);
const PROCESS_BYTES: usize = 32 * 1024 * 1024;
const SNAPSHOT_BYTES: usize = 4 * 1024 * 1024;
struct MemoValue {
    value: Value,
    bytes: usize,
}
impl Drop for MemoValue {
    fn drop(&mut self) {
        MEMO_BYTES.fetch_sub(self.bytes, Ordering::Relaxed);
    }
}
#[derive(Default)]
pub(crate) struct QueryMemo {
    values: BTreeMap<String, MemoValue>,
    order: VecDeque<String>,
    bytes: usize,
}
impl QueryMemo {
    pub fn get(&mut self, key: &str) -> Option<Value> {
        let value = self.values.get(key)?.value.clone();
        self.order.retain(|k| k != key);
        self.order.push_back(key.to_owned());
        Some(value)
    }
    pub fn insert(&mut self, key: String, value: &Value) {
        let Ok(encoded) = serde_json::to_vec(value) else {
            return;
        };
        let bytes = encoded.len().saturating_mul(2);
        if bytes > SNAPSHOT_BYTES || self.values.contains_key(&key) {
            return;
        }
        while self.values.len() >= 16 || self.bytes + bytes > SNAPSHOT_BYTES {
            let Some(old) = self.order.pop_front() else {
                break;
            };
            if let Some(value) = self.values.remove(&old) {
                self.bytes -= value.bytes;
            }
        }
        if MEMO_BYTES
            .try_update(Ordering::Relaxed, Ordering::Relaxed, |n| {
                n.checked_add(bytes).filter(|n| *n <= PROCESS_BYTES)
            })
            .is_err()
        {
            return;
        }
        self.bytes += bytes;
        self.order.push_back(key.clone());
        self.values.insert(
            key,
            MemoValue {
                value: value.clone(),
                bytes,
            },
        );
    }
}

/// Selection metadata only: no Markdown, workflow steps, schemas, policies, or installation files.
pub fn configuration_summary(
    catalog: &ConfigurationCatalogSnapshot,
    r: &ConfigurationRecord,
) -> Value {
    let mut row = json!({"id":r.id,"type":r.r#type,"name":r.name,"scope":r.scope,"version":r.version,"revision":r.revision,"hash":r.content_hash});
    if r.id != format!("{}:{}", r.definition_id, r.version) && r.id != r.definition_id {
        row["identity"] = json!(r.definition_id);
    }
    if !catalog.is_current_head(r) {
        row["head"] = json!(false);
    }
    if catalog.is_archived(r) {
        row["archived"] = json!(true);
    }
    if catalog.is_retired(r) {
        row["retired"] = json!(true);
    }
    row["origin"] = catalog
        .origin(&r.id)
        .map_or(Value::Null, |o| o["origin"].clone());
    let fields = configuration_selection_record(r);
    row.as_object_mut()
        .unwrap()
        .extend(fields.as_object().unwrap().clone());
    row.as_object_mut().unwrap().remove("definition_id");
    row
}

/// Shared thin fields for discussion catalogs, whose visibility is resolved by their source port.
pub fn configuration_selection_record(r: &ConfigurationRecord) -> Value {
    let mut row = json!({"id":r.id,"type":r.r#type,"definition_id":r.definition_id,"name":r.name,"scope":r.scope,"version":r.version,"revision":r.revision,"hash":r.content_hash});
    let data: serde_json::Map<String, Value> = [
        "provider",
        "harness_provider",
        "model",
        "runtime_version",
        "target_kind",
        "implementation",
        "integration_ref",
        "agent_revision_id",
        "default_workflow_ref",
    ]
    .into_iter()
    .filter_map(|key| r.data.get(key).map(|v| (key.into(), v.clone())))
    .collect();
    if !data.is_empty() {
        row["data"] = json!(data);
    }
    for (key, value) in &r.refs {
        row[key] = value.clone();
    }
    row
}

pub fn configuration_index(
    catalog: &ConfigurationCatalogSnapshot,
    p: &Value,
    project: Option<&str>,
) -> Result<Value> {
    let state = &catalog.state;
    let mut rows: Vec<_> = state
        .governance
        .iter()
        .filter(|r| {
            r.is_definition() || matches!(r.r#type.as_str(), "agent_instance" | "workflow_draft")
        })
        .filter(|r| p["kind"].as_str().is_none_or(|kind| r.r#type == kind))
        .filter(|r| configuration_visible(state, r, project))
        .filter(|r| p.get("scope").is_none_or(|scope| json!(r.scope) == *scope))
        .filter(|r| p["include_archived"] == true || !catalog.is_archived(r))
        .filter(|r| p["include_deleted"] == true || !catalog.is_retired(r))
        .filter(|r| {
            p["include_history"] == true
                || if p["include_archived"] == true {
                    catalog.is_head(r)
                } else {
                    catalog.is_current_head(r)
                }
        })
        .collect();
    rows.sort_by_key(|r| (std::cmp::Reverse(r.created_at), r.id.as_str()));
    let offset = p["offset"].as_u64().unwrap_or(0) as usize;
    let limit = p["limit"].as_u64().unwrap_or(1000).clamp(1, 1000) as usize;
    let version = configuration_hash(&(
        &catalog.fingerprint,
        project,
        p["kind"].clone(),
        p["scope"].clone(),
        p["include_history"].clone(),
        p["include_archived"].clone(),
        p["include_deleted"].clone(),
    ));
    if offset > 0 && p["index_version"].as_str() != Some(&version) {
        return Err(err("catalog_changed", "目录分页版本已变化，请重新读取"));
    }
    Ok(
        json!({"records":rows.iter().skip(offset).take(limit).map(|r|configuration_summary(catalog,r)).collect::<Vec<_>>(),"total":rows.len(),"index_version":version}),
    )
}

pub fn configuration_records(
    catalog: &ConfigurationCatalogSnapshot,
    p: &Value,
    project: Option<&str>,
) -> Result<Value> {
    let refs: Vec<String> = serde_json::from_value(p["refs"].clone())
        .map_err(|e| err("invalid_params", e.to_string()))?;
    require(refs.len() <= 64, "一次最多读取 64 个确切配置修订")?;
    let records = refs
        .iter()
        .map(|id| {
            let r = catalog
                .record(id)
                .ok_or_else(|| err("not_found", "配置修订不存在"))?;
            require(
                catalog_record(r) && configuration_visible(&catalog.state, r, project),
                "配置修订不属于当前作用域",
            )?;
            let mut row = json!(r);
            row["template_source"] = catalog.origin(id).cloned().unwrap_or(Value::Null);
            Ok(row)
        })
        .collect::<Result<Vec<_>>>()?;
    Ok(json!({"records":records}))
}

pub fn project_configuration_context(
    catalog: &ConfigurationCatalogSnapshot,
    project: Option<&str>,
) -> Result<Value> {
    let s = &catalog.state;
    let configuration = s
        .governance
        .iter()
        .filter(|r| {
            r.r#type == "project_configuration" && project.is_none_or(|id| r.definition_id == id)
        })
        .max_by_key(|r| r.version);
    let pinned = crate::templates::pinned_template_records(s, configuration.map(|r| &r.data));
    let standard=configuration.and_then(|c|c.data["standard_ref"].as_str()).and_then(|id|catalog.record(id)).map(|r| {
        let provenance=s.governance.iter().find(|d|d.r#type=="standard_derivation" && d.definition_id==r.id).map(|d|json!({"base_standard_ref":d.data["base_standard_ref"],"overrides":d.data["overrides"]}));
        json!({"record":{"id":r.id,"name":r.name,"version":r.version},"provenance":provenance,"default_workflow_ref":configuration.map(|c|&c.data["workflow_ref"])})
    });
    Ok(
        json!({"configuration":configuration,"standard":standard,"pinned_records":pinned.iter().map(|r|configuration_summary(catalog,r)).collect::<Vec<_>>(),"uses_archived_templates":pinned.iter().any(|r|!template_record_current(s,r))}),
    )
}

fn snapshot_summary(value: &mut Value) {
    if let Some(nodes) = value["nodes"].as_array_mut() {
        for node in nodes {
            if let Some(node) = node.as_object_mut() {
                node.remove("content");
            }
        }
    }
    if let Some(record) = value["record"].as_object_mut() {
        record.remove("data");
    }
}
pub fn summarize_configuration_result(method: &str, value: &mut Value) {
    if method == "query.standard" {
        snapshot_summary(value);
        value["summary"] = json!(true);
    }
    if method == "query.project_standard_preview" {
        snapshot_summary(&mut value["snapshot"]);
        if let Some(roles) = value["requirements"]["roles"].as_array_mut() {
            for role in roles {
                *role = json!({"role_id":role["role_id"],"role_ref":role["role_ref"],"name":role["name"],"role":{"name":role["role"]["name"],"id":role["role"]["id"]}});
            }
        }
        if let Some(bindings) = value["requirements"]["specification_view"].as_array_mut() {
            for binding in bindings {
                *binding = json!({"path":binding["path"],"spec_refs":binding["spec_refs"]});
            }
        }
        value["summary"] = json!(true);
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn query_memo_is_bounded_and_evicts_old_keys() {
        let mut memo = QueryMemo::default();
        let payload = json!({"body":"x".repeat(100*1024)});
        for n in 0..30 {
            memo.insert(n.to_string(), &payload);
        }
        assert!(memo.values.len() <= 16);
        assert!(memo.bytes <= SNAPSHOT_BYTES);
        assert!(memo.get("0").is_none());
        assert!(memo.get("29").is_some());
        memo.insert("oversize".into(), &json!("x".repeat(SNAPSHOT_BYTES)));
        assert!(memo.get("oversize").is_none());
    }
}

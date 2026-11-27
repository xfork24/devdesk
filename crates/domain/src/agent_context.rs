//! Versioned, read-only context access shared by every Agent identity.
use crate::*;
use serde_json::json;

pub const AGENT_CONTEXT_PROTOCOL: &str = "devdesk-agent-context/v1";

pub fn agent_context_tools() -> Vec<ToolDefinition> {
    let list = json!({"type":"object","description":"List this Agent instance's own historical experience, including project, Workflow and Feature sources. No current project is required. Read individual snapshots with agent.experience.read using each row's project_id as source_project_id. Follow next_offset for additional pages.","additionalProperties":false,"properties":{
        "project_id":{"type":"string"},"project_name":{"type":"string"},
        "workflow_instance_id":{"type":"string"},"feature_id":{"type":"string"},
        "feature_name":{"type":"string"},"task_id":{"type":"string"},
        "keyword":{"type":"string"},"from":{"type":"integer","minimum":0},
        "to":{"type":"integer","minimum":0},"offset":{"type":"integer","minimum":0},
        "limit":{"type":"integer","minimum":1,"maximum":100}}});
    let read = json!({"type":"object","description":"Read an experience returned by agent.experience.list. Use its project_id as source_project_id and snapshot_id. content_chunk is a fragment of the JSON snapshot, documents and evidence index; follow next_offset until null and verify content_digest. Set document_id or artifact_id to read an indexed document or original evidence.","additionalProperties":false,
        "required":["snapshot_id","source_project_id"],"properties":{
        "snapshot_id":{"type":"string","minLength":1},
        "source_project_id":{"type":"string","minLength":1},
        "document_id":{"type":"string","minLength":1},
        "artifact_id":{"type":"string","minLength":1},
        "offset":{"type":"integer","minimum":0},
        "max_bytes":{"type":"integer","minimum":1024,"maximum":65536}}});
    [
        ("agent.experience.list", list),
        ("agent.experience.read", read),
    ]
    .into_iter()
    .map(|(name, input_schema)| ToolDefinition {
        runtime: None,
        harness_provider: None,
        qualified_name: name.into(),
        implementation: "platform".into(),
        handler: None,
        input_schema,
        output_schema: json!({"type":"object"}),
        capabilities: vec![],
    })
    .collect()
}

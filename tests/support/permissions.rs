//! Complete v2 grants for offline contract tests; no v1 defaults or override chain.
#![allow(dead_code)]
use devdesk_domain::*;
#[path = "defaults.rs"]
mod defaults;
use defaults::*;
use serde_json::json;

pub fn prepare(
    s: &mut EngineeringState,
    provider: HarnessProvider,
    policy: PermissionPolicy,
) -> VersionedPermissionLock {
    let harness = s
        .governance
        .iter()
        .find(|r| r.r#type == "harness" && r.data["provider"] == provider.as_str())
        .unwrap()
        .id
        .clone();
    let mut bounds = Vec::new();
    for (kind, target, profile_id) in [
        (
            PermissionTargetKind::Harness,
            harness,
            "builtin-permission-harness-full-access:1",
        ),
        (
            PermissionTargetKind::Sandbox,
            DEFAULT_PERMISSION_SANDBOX.into(),
            "builtin-permission-sandbox-full-access:1",
        ),
    ] {
        let mut profile: PermissionProfile = s
            .configuration(profile_id)
            .unwrap()
            .permission_profile()
            .unwrap();
        if kind == PermissionTargetKind::Harness {
            profile.policy.filesystem = policy.filesystem.clone();
            profile.policy.network = policy.network.clone();
            profile.policy.tools = Some(policy.tools.clone());
        }
        profile.normalize().unwrap();
        let data = permission_profile_data(&profile);
        let r = s
            .governance
            .iter_mut()
            .find(|r| r.id == profile_id)
            .unwrap();
        r.content_hash = content_digest(&r.refs, &data);
        r.data = data;
        let target = permission_target(s, kind, &target).unwrap();
        let binding = VersionedPermissionBinding {
            enforcement_contract: devdesk_adapters::permission_profiles::permission_contract(
                s, &target,
            )
            .unwrap(),
            target,
            permission_revision_id: profile_id.into(),
        };
        let hash = permission_content_hash(&binding);
        let identity = format!("fixture-binding-{hash}");
        let id = format!("{identity}:1");
        let data = json!(binding);
        s.governance.push(ConfigurationRecord {
            id: id.clone(),
            r#type: "permission_binding".into(),
            definition_id: identity,
            scope: Scope::global(),
            name: profile.name.clone(),
            version: 1,
            supersedes: None,
            content_hash: content_digest(&References::new(), &data),
            refs: References::new(),
            data,
            immutable: true,
            revision: 1,
            created_at: 0,
        });
        let record = s.governance.last().unwrap();
        bounds.push(BoundPermission {
            binding_revision_id: id,
            binding_hash: record.content_hash.clone(),
            binding,
            profile_hash: s.configuration(profile_id).unwrap().content_hash.clone(),
            profile,
        });
    }
    VersionedPermissionLock {
        harness: bounds.remove(0),
        sandbox: bounds.remove(0),
    }
}
pub fn compile(
    provider: HarnessProvider,
    scope: ExecutionScope,
    policy: PermissionPolicy,
) -> Result<ExecutionPermissions> {
    let mut s = EngineeringState::default();
    devdesk_adapters::seed_factory_configuration(&mut s);
    let lock = prepare(&mut s, provider, policy);
    devdesk_adapters::permission_profiles::compile_permission_profiles(&s, &lock, Some(scope))
}

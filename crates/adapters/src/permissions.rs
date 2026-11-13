//! Compile portable access policies into the fixed provider/container contracts.
use super::*;
use std::collections::BTreeMap;

pub fn capabilities(provider: HarnessProvider) -> PermissionCapabilities {
    PermissionCapabilities {
        backend: provider.as_str().into(),
        version: match provider {
            HarnessProvider::Codex => runtime_preparation::CODEX_VERSION,
            HarnessProvider::Dsh => runtime_preparation::DSH_VERSION,
        }
        .into(),
        recipe_digest: match provider {
            HarnessProvider::Codex => runtime_preparation::default_codex_recipe_digest(),
            HarnessProvider::Dsh => runtime_preparation::default_dsh_recipe_digest(),
        },
        filesystem: vec![
            "full_access".into(),
            "read_only".into(),
            "scoped_worktree_writes".into(),
        ],
        network: vec!["enabled".into(), "disabled_tools".into()],
        tools: if provider == HarnessProvider::Dsh {
            vec!["all_available".into(), "restricted".into()]
        } else {
            vec!["all_available".into()]
        },
        limitations: vec![
            "network allowlist requires an enforcing proxy and is unsupported".into(),
            "scope is constrained by the registered worktree and Docker mounts".into(),
            "Full access trusts every process inside this container".into(),
            if provider == HarnessProvider::Dsh {
                "scoped writes must be enforceable by the frozen execution-scope mounts".into()
            } else {
                "scoped writes use the native named permissions profile".into()
            },
        ],
    }
}
pub fn asset_digests() -> BTreeMap<String, String> {
    [
        (
            "docker/seccomp/nested-v1.json",
            include_bytes!("../../../permissions/backends/docker/seccomp/nested-v1.json")
                .as_slice(),
        ),
        (
            "docker/apparmor/devdesk-harness-v1.apparmor",
            include_bytes!(
                "../../../permissions/backends/docker/apparmor/devdesk-harness-v1.apparmor"
            )
            .as_slice(),
        ),
        (
            "dsh/bwrap-runner.sh",
            include_bytes!("../../../permissions/backends/dsh/bwrap-runner.sh").as_slice(),
        ),
        (
            "dsh/tool-policy.mjs",
            include_bytes!("../../../permissions/backends/dsh/tool-policy.mjs").as_slice(),
        ),
    ]
    .into_iter()
    .map(|(n, b)| (n.into(), format!("{:x}", Sha256::digest(b))))
    .collect()
}
pub fn compile_resolved_permissions(
    provider: HarnessProvider,
    scope: ExecutionScope,
    harness: ResolvedPermission,
    sandbox: ResolvedPermission,
    approval: ApprovalPolicy,
) -> Result<ExecutionPermissions> {
    let manifest: Value =
        serde_json::from_str(include_str!("../../../permissions/backends/manifest.json"))
            .map_err(storage)?;
    for (path, digest) in asset_digests() {
        if manifest["assets"][&path]["sha256"] != digest {
            return Err(err(
                "permission_asset_mismatch",
                format!("权限后端摘要漂移: {path}"),
            ));
        }
    }
    let nested = harness.policy.filesystem != FilesystemPermission::FullAccess
        || harness.policy.network != NetworkPermission::Enabled;
    let mut result = ExecutionPermissions {
        binding_lock: None,
        harness: harness.clone(),
        sandbox,
        approval,
        scope,
        isolation: SandboxIsolation {
            protected_metadata: vec![".git".into(), ".codex".into(), ".agents".into()],
            read_only_root: false,
            executable_tmp: true,
            nested_sandbox: nested,
            backend: if nested {
                "docker-nested-v1"
            } else {
                "docker-default-v1"
            }
            .into(),
            credential_boundary: "container-trust".into(),
        },
        limits: ResourceLimits::default(),
        enforcement: PermissionEnforcementPlan {
            native_options: Some(NativePermissionOptions {
                provider,
                version: capabilities(provider).version,
                sandbox_mode: match (&harness.policy.filesystem, provider) {
                    (FilesystemPermission::FullAccess, HarnessProvider::Codex)
                        if harness.policy.network == NetworkPermission::Enabled =>
                    {
                        "externalSandbox"
                    }
                    (_, HarnessProvider::Codex) => "namedProfile",
                    (FilesystemPermission::FullAccess, HarnessProvider::Dsh) => {
                        "danger-full-access"
                    }
                    (FilesystemPermission::ReadOnly, HarnessProvider::Dsh) => "read-only",
                    (_, HarnessProvider::Dsh) => "workspace-write",
                }
                .into(),
                profile: "devdesk".into(),
                approval_policy: match (approval, provider) {
                    (ApprovalPolicy::Never, _) => "never",
                    (_, HarnessProvider::Codex) => "on-request",
                    _ => "ask",
                }
                .into(),
            }),
            compiler_version: "permission-compiler/v2".into(),
            harness_backend: format!("{}-native-v1", provider.as_str()),
            sandbox_backend: if nested {
                "docker-nested-v1"
            } else {
                "docker-default-v1"
            }
            .into(),
            rules: [
                ("harness.filesystem".into(), "provider-native".into()),
                (
                    "harness.network".into(),
                    "provider-native-or-confined-runner".into(),
                ),
                ("sandbox.filesystem".into(), "docker-mounts-and-user".into()),
                ("sandbox.network".into(), "docker-network".into()),
                (
                    "execution_scope".into(),
                    "coordinator-and-docker-mounts".into(),
                ),
            ]
            .into(),
            asset_digests: asset_digests(),
        },
    };
    result.harness.policy.canonicalize();
    result.sandbox.policy.canonicalize();
    validate_execution_permissions(&result, provider)?;
    Ok(result)
}
pub fn validate_execution_permissions(
    p: &ExecutionPermissions,
    provider: HarnessProvider,
) -> Result<()> {
    p.harness.policy.validate()?;
    p.sandbox.policy.validate()?;
    for (layer, policy) in [
        ("harness", &p.harness.policy),
        ("sandbox", &p.sandbox.policy),
    ] {
        if matches!(policy.network, NetworkPermission::Allowlist { .. }) {
            return Err(err(
                "permission_unsupported",
                format!("{layer}.network.allowlist 需要受控代理，当前后端不支持"),
            ));
        }
        if let FilesystemPermission::Scoped { rules } = &policy.filesystem {
            for r in rules {
                if r.access == FileAccess::Deny
                    || (r.access == FileAccess::ReadWrite
                        && r.path != "/work"
                        && !r.path.starts_with("/work/"))
                {
                    return Err(err(
                        "permission_unsupported",
                        format!(
                            "{layer}.filesystem: 当前后端只支持 /work 内的限定写，拒绝读取需要独立执行后端"
                        ),
                    ));
                }
            }
            if layer == "sandbox" {
                return Err(err(
                    "permission_unsupported",
                    "Sandbox scoped 策略请使用执行范围挂载；当前只支持 FullAccess / ReadOnly",
                ));
            }
        }
    }
    if p.sandbox.policy.tools != ToolPermission::AllAvailable {
        return Err(err(
            "permission_unsupported",
            "Docker 不理解 Harness 工具名；请在 Harness 上配置工具规则",
        ));
    }
    if p.sandbox.policy.network == NetworkPermission::Disabled {
        return Err(err(
            "permission_conflict",
            "Sandbox 网络关闭会阻止模型连接；工具禁网请修改 Harness 权限",
        ));
    }
    match provider {
        HarnessProvider::Codex if p.harness.policy.tools != ToolPermission::AllAvailable => {
            return Err(err(
                "permission_unsupported",
                "Codex 固定版本没有通用工具白名单接口",
            ));
        }
        HarnessProvider::Dsh if p.harness.policy.network == NetworkPermission::Disabled => {
            if p.harness.policy.filesystem == FilesystemPermission::FullAccess {
                return Err(err(
                    "permission_unsupported",
                    "DSH 工具禁网需要 read_only 或 scoped 文件模式以启用原生沙箱执行器",
                ));
            }
            if !matches!(&p.harness.policy.tools,ToolPermission::Restricted{allow,deny:_} if allow.iter().all(|n|["bash","ask_user_question"].contains(&n.as_str())))
            {
                return Err(err(
                    "permission_unsupported",
                    "DSH 工具禁网需要显式限制为受控 bash 和用户问答，其他网络入口尚无执行器",
                ));
            }
        }
        _ => {}
    }
    if provider == HarnessProvider::Dsh {
        if let FilesystemPermission::Scoped { rules } = &p.harness.policy.filesystem {
            // DSH's immutable session cwd always grants the whole workspace in
            // workspace-write mode. Additional roots cannot narrow that grant.
            // Accept only restrictions already enforced by the outer scope mounts.
            let absolute = |path: &str| {
                if path == "." {
                    "/work".to_string()
                } else {
                    format!("/work/{path}")
                }
            };
            let within = |path: &str, parent: &str| {
                path == parent
                    || path
                        .strip_prefix(parent)
                        .is_some_and(|s| s.starts_with('/'))
            };
            let writable: Vec<_> = p.scope.writable_paths.iter().map(|v| absolute(v)).collect();
            let readonly: Vec<_> = p
                .scope
                .read_only_paths
                .iter()
                .map(|v| absolute(v))
                .collect();
            let enforced = p.harness.policy.file_access("/") == FileAccess::Read
                && writable
                    .iter()
                    .all(|v| p.harness.policy.file_access(v) == FileAccess::ReadWrite)
                && rules
                    .iter()
                    .filter(|r| r.access == FileAccess::Read)
                    .all(|r| {
                        !writable.iter().any(|v| within(&r.path, v))
                            || readonly.iter().any(|v| within(&r.path, v))
                    });
            if !enforced {
                return Err(err(
                    "permission_unsupported",
                    "DSH 原生 workspace-write 不能缩小会话 cwd；scoped 限制须由冻结 ExecutionScope 挂载落实",
                ));
            }
        }
    }
    if !p.scope.writable_paths.is_empty()
        && (p.sandbox.policy.filesystem == FilesystemPermission::ReadOnly
            || p.harness.policy.filesystem == FilesystemPermission::ReadOnly)
    {
        return Err(err(
            "permission_conflict",
            "正式写入阶段的执行范围与只读 Permission 冲突",
        ));
    }
    Ok(())
}
pub fn codex_sandbox_policy(p: &ExecutionPermissions) -> Value {
    if p.harness.policy.filesystem == FilesystemPermission::FullAccess
        && p.harness.policy.network == NetworkPermission::Enabled
    {
        json!({"type":"externalSandbox","networkAccess":"enabled"})
    } else if p.harness.policy.filesystem == FilesystemPermission::ReadOnly {
        json!({"type":"readOnly","networkAccess":p.harness.policy.network==NetworkPermission::Enabled})
    } else {
        let roots = match &p.harness.policy.filesystem {
            FilesystemPermission::Scoped { rules } => rules
                .iter()
                .filter(|r| r.access == FileAccess::ReadWrite)
                .map(|r| r.path.clone())
                .collect::<Vec<_>>(),
            _ => vec!["/work".into()],
        };
        json!({"type":"workspaceWrite","writableRoots":roots,"networkAccess":p.harness.policy.network==NetworkPermission::Enabled})
    }
}
pub fn codex_config(p: &ExecutionPermissions) -> String {
    let approval = if p.approval == ApprovalPolicy::Never {
        "never"
    } else {
        "on-request"
    };
    if p.harness.policy.filesystem == FilesystemPermission::FullAccess
        && p.harness.policy.network == NetworkPermission::Enabled
    {
        return format!(
            "cli_auth_credentials_store=\"file\"\nsandbox_mode=\"danger-full-access\"\napproval_policy=\"{approval}\"\n[projects.\"/work\"]\ntrust_level=\"untrusted\"\n"
        );
    }
    let mut rules = BTreeMap::new();
    let root = match &p.harness.policy.filesystem {
        FilesystemPermission::FullAccess => "write",
        FilesystemPermission::ReadOnly => "read",
        FilesystemPermission::Scoped { .. } => "deny",
    };
    rules.insert(":root".to_string(), root);
    rules.insert(":minimal".to_string(), "read");
    if let FilesystemPermission::Scoped { rules: paths } = &p.harness.policy.filesystem {
        for r in paths {
            rules.insert(
                if r.path == "/" {
                    ":root".into()
                } else {
                    r.path.clone()
                },
                match r.access {
                    FileAccess::Deny => "deny",
                    FileAccess::Read => "read",
                    FileAccess::ReadWrite => "write",
                },
            );
        }
    }
    if p.isolation.credential_boundary == "confined-tools" {
        rules.insert("/state".into(), "deny");
    }
    let mut out = format!(
        "cli_auth_credentials_store=\"file\"\ndefault_permissions=\"devdesk\"\napproval_policy=\"{approval}\"\n[permissions.devdesk.filesystem]\n"
    );
    for (path, access) in rules {
        out.push_str(&format!(
            "{}=\"{access}\"\n",
            serde_json::to_string(&path).unwrap()
        ));
    }
    out.push_str(&format!("[permissions.devdesk.network]\nenabled={}\n[projects.\"/work\"]\ntrust_level=\"untrusted\"\n",p.harness.policy.network==NetworkPermission::Enabled));
    if p.harness.policy.network == NetworkPermission::Disabled {
        out = format!("web_search=\"disabled\"\n{out}");
    }
    if p.isolation.credential_boundary == "confined-tools" {
        out.push_str("[features]\nhooks=false\nplugins=false\napps=false\nmulti_agent=false\ncode_mode_host=true\nworkspace_dependencies=false\nskill_mcp_dependency_install=false\nbrowser_use=false\ncomputer_use=false\n");
    }
    out
}

pub fn dsh_profile(p: &ExecutionPermissions, instructions: &str, cwd: &str) -> Result<String> {
    let mode = match p.harness.policy.filesystem {
        FilesystemPermission::FullAccess => "danger-full-access",
        FilesystemPermission::ReadOnly => "read-only",
        FilesystemPermission::Scoped { .. } => "workspace-write",
    };
    let approval = if p.approval == ApprovalPolicy::Never {
        "never"
    } else {
        "ask"
    };
    let roots = match &p.harness.policy.filesystem {
        FilesystemPermission::Scoped { rules } => rules
            .iter()
            .filter(|r| r.access == FileAccess::ReadWrite)
            .map(|r| r.path.clone())
            .collect::<Vec<_>>(),
        _ => vec![],
    };
    let persona = serde_json::to_string(instructions).map_err(storage)?;
    let cwd_yaml = serde_json::to_string(cwd).map_err(storage)?;
    let mut patch = format!(
        "- id: agent-instructions\n  disabled: true\n- id: session-telemetry-otel\n  disabled: true\n- id: session-log-deepseek\n  disabled: true\n- id: system-prompt\n  config:\n    personaPrefix: {persona}\n- id: sandbox-policy\n  config:\n    mode: {mode}\n    workspaceRoot: {cwd_yaml}\n    additionalWritableRoots: {}\n- id: approval\n  config:\n    policy: {approval}\n- id: permission\n  config:\n    defaultPreset: devdesk\n    presets:\n      devdesk:\n        sandbox: {mode}\n        approval: {approval}\n",
        serde_json::to_string(&roots).map_err(storage)?
    );
    if matches!(p.harness.policy.tools, ToolPermission::Restricted { .. }) {
        patch.push_str(
            "- insert:\n  - id: devdesk-tool-boundary\n    name: /run/devdesk/tool-policy.mjs\n",
        );
    }
    if p.harness.policy.network == NetworkPermission::Disabled {
        patch.push_str("- id: sandbox\n  config:\n    runnerCommand: [/bin/bash, /run/devdesk/runner]\n    runnerFailureSignatures: ['bwrap:']\n");
    }
    Ok(patch)
}

pub fn validate_runtime_version(installation: &RuntimeInstallation) -> Result<()> {
    // Acceptance is content-anchored: the installation must originate from the exact
    // build recipe whose permission contract this binary verified. The recipe identity
    // is embedded in the installation key at registration time.
    let expected = capabilities(installation.key.provider).recipe_digest;
    let actual = installation_recipe(installation);
    if actual != expected {
        return Err(err(
            "permission_unsupported",
            format!(
                "权限后端只验证了配方 {}，当前安装为 {}",
                &expected[..16],
                if actual.is_empty() {
                    "未登记配方".into()
                } else {
                    actual
                }
            ),
        ));
    }
    Ok(())
}

/// Recipe identity embedded in a runtime key (`...+recipe.<64 hex>`), if any.
pub fn installation_recipe(installation: &RuntimeInstallation) -> String {
    installation
        .key
        .version
        .split_once("+recipe.")
        .map(|(_, recipe)| recipe.to_owned())
        .unwrap_or_default()
}

#[cfg(test)]
mod recipe_tests {
    use super::*;

    #[test]
    fn permission_acceptance_uses_the_complete_registered_recipe_digest() {
        let digest = runtime_preparation::default_codex_recipe_digest();
        let mut installation = RuntimeInstallation {
            id: "recipe-regression".into(),
            key: RuntimeKey {
                provider: HarnessProvider::Codex,
                version: format!("display-metadata+recipe.{digest}"),
                os: "linux".into(),
                arch: "aarch64".into(),
            },
            artifact: RuntimeArtifact::DockerImage {
                image_id: format!("sha256:{}", "a".repeat(64)),
            },
            source: "test fixture".into(),
            license: "test".into(),
        };
        assert!(validate_runtime_version(&installation).is_ok());
        installation.key.version = format!("display-metadata+recipe.{}", &digest[..16]);
        assert!(validate_runtime_version(&installation).is_err());
        installation.key.version = format!(
            "display-metadata+recipe.{}{}",
            &digest[..63],
            if digest.ends_with('0') { '1' } else { '0' }
        );
        assert!(validate_runtime_version(&installation).is_err());
    }
}

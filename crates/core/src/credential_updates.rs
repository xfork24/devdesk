//! Credential publication is a versioned binding to a verified immutable vault candidate.
use crate::{concurrency::*, *};

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ConfigureCredential {
    pub command_id: String,
    pub expected_revision: u64,
    pub provider: String,
    pub model_provider: String,
    pub provider_ref: String,
    pub api_key: String,
    pub project_id: Option<String>,
}
pub use crate::ports::{CredentialCandidate, CredentialCandidates};

pub fn parse(params: &Value) -> Result<ConfigureCredential> {
    if params.get("expected_revision").is_none() {
        return Err(err("precondition_required", "缺少凭据引用的原始版本"));
    }
    let command: ConfigureCredential =
        serde_json::from_value(params.clone()).map_err(|e| err("invalid_params", e.to_string()))?;
    validate(&command)?;
    Ok(command)
}
fn validate(command: &ConfigureCredential) -> Result<()> {
    require(
        !command.command_id.is_empty() && command.command_id.len() <= 1024,
        "需要有效的命令身份",
    )?;
    require(command.provider == "dsh", "Codex 使用独立 OAuth 接口")?;
    require(
        !command.model_provider.is_empty() && command.model_provider.len() <= 128,
        "需要有效的模型服务商",
    )?;
    require(
        !command.provider_ref.is_empty() && command.provider_ref.len() <= 1024,
        "需要原始服务商源修订",
    )?;
    require(command.expected_revision < i64::MAX as u64, "凭据版本越界")?;
    require(
        (8..=4096).contains(&command.api_key.len())
            && command.api_key.bytes().all(|b| b.is_ascii_graphic()),
        "API key 格式无效",
    )?;
    require(
        command
            .project_id
            .as_ref()
            .is_none_or(|id| !id.is_empty() && id.len() <= 1024),
        "无效的项目上下文",
    )?;
    Ok(())
}
pub fn binding_key(provider: &str) -> ResourceKey {
    ResourceKey::new("setting", format!("credentials.dsh.{provider}"))
}
fn version(resource: ResourceKey, revision: u64) -> ResourceVersion {
    ResourceVersion { resource, revision }
}
fn namespace(store: &dyn EngineeringStore, context: &InvocationContext) -> Result<()> {
    if store.namespace() != StoreNamespace::Configuration {
        return Err(err("invalid_scope", "凭据引用只属于全局配置库"));
    }
    if !matches!(context.actor(), WorkflowActor::Human) {
        return Err(err("permission_denied", "凭据更新需要可信人类入口"));
    }
    Ok(())
}
pub fn configure(
    store: &dyn EngineeringStore,
    vault: &dyn CredentialCandidates,
    command: &ConfigureCredential,
    context: &InvocationContext,
) -> Result<CommitReceipt> {
    validate(command)?;
    namespace(store, context)?;
    // Persist only a digest of the secret. Replay needs no vault access, current
    // provider selection, catalog expansion, or replacement client versions.
    let fingerprint = configuration_hash(&(
        "credentials.configure",
        &command.command_id,
        command.expected_revision,
        &command.provider,
        &command.model_provider,
        &command.provider_ref,
        configuration_hash(&command.api_key),
        &command.project_id,
        context,
    ));
    if let Some(receipt) = store.replay_commit(&command.command_id, &fingerprint)? {
        return Ok(receipt);
    }
    let outcome = (|| {
        let (identity, revision) = command
            .provider_ref
            .rsplit_once(':')
            .ok_or_else(|| err("invalid_params", "服务商需要确切源引用"))?;
        let source_revision = revision
            .parse::<u64>()
            .ok()
            .filter(|n| *n > 0)
            .ok_or_else(|| err("invalid_params", "服务商源版本无效"))?;
        let binding = binding_key(&command.model_provider);
        let intent = ResourceKey::new(
            "setting",
            format!(
                "credential.intent.{}",
                configuration_hash(&command.command_id)
            ),
        );
        let source = ResourceKey::new("source", &command.provider_ref);
        let head = ResourceKey::new("source_head", identity);
        let rows = store.read_resources(&[
            binding.clone(),
            intent.clone(),
            source.clone(),
            head.clone(),
        ])?;
        require(
            rows.len() == 4
                && rows
                    .iter()
                    .zip([&binding, &intent, &source, &head])
                    .all(|(row, key)| &row.resource == key),
            "凭据上下文读取不完整",
        )?;
        if rows[0].revision != command.expected_revision {
            return Err(err("revision_conflict", "凭据引用已被修改"));
        }
        if rows[3].revision != source_revision
            || rows[3]
                .value
                .as_ref()
                .and_then(|value| value["reference"].as_str())
                != Some(command.provider_ref.as_str())
        {
            return Err(err("revision_conflict", "模型服务商源已变化"));
        }
        let definition: SourceDefinition = serde_json::from_value(
            rows[2]
                .value
                .clone()
                .ok_or_else(|| err("not_found", "模型服务商源不存在"))?,
        )
        .map_err(|e| err("storage", e.to_string()))?;
        require(definition.scope.is_global(), "凭据服务商源需要全局归属")?;
        require(
            definition.r#type == "model_provider",
            "所选源不是模型服务商",
        )?;
        let provider: ProviderFacts = serde_json::from_value(definition.data)
            .map_err(|e| err("invalid_params", e.to_string()))?;
        provider.validate()?;
        require(
            provider.id == command.model_provider
                && provider.requires_key
                && provider.harness == "dsh",
            "服务商与凭据请求不符或无需密钥",
        )?;
        let token = configuration_hash(&("credential-candidate", store.root(), &fingerprint));
        let candidate_id = format!(
            "{}-{}-{}-{}-{}",
            &token[..8],
            &token[8..12],
            &token[12..16],
            &token[16..20],
            &token[20..32]
        );
        let prepared = json!({"status":"prepared","fingerprint":fingerprint,"candidate_id":candidate_id,"provider_ref":command.provider_ref,"expected_revision":command.expected_revision});
        if let Some(previous) = &rows[1].value {
            require(previous == &prepared, "凭据意图与原始命令不符")?;
            require(rows[1].revision == 1, "凭据意图状态无效")?;
        }
        let dependencies = vec![
            version(binding.clone(), command.expected_revision),
            version(source, rows[2].revision),
            version(head, source_revision),
        ];
        if rows[1].value.is_none() {
            let mut guards = dependencies.clone();
            guards.push(version(intent.clone(), 0));
            store.commit_changes(&PreparedChangeSet {
                command_id: format!(
                    "credential-prepare:{}",
                    configuration_hash(&command.command_id)
                ),
                fingerprint: configuration_hash(&("credential-intent", &fingerprint)),
                read_set: ReadSet::new(guards)?,
                changes: vec![ResourceChange {
                    resource: intent.clone(),
                    value: prepared.clone(),
                }],
                result: prepared.clone(),
                time: now() as i64,
            })?;
        }
        // Durable intent precedes external I/O. The vault reconciles the stable
        // immutable candidate; a failed publication never changes the old binding.
        let candidate = vault.ensure(&provider, &candidate_id, &command.api_key)?;
        require(
            !candidate.reference.is_empty()
                && candidate.status.is_object()
                && candidate.status["reference"].as_str() == Some(candidate.reference.as_str())
                && candidate.status["configured"] == true,
            "凭据端口没有返回核验回执",
        )?;
        let mut result = candidate.status;
        result["credential_version"] =
            json!(version(binding.clone(), command.expected_revision + 1));
        result["provider_ref"] = json!(command.provider_ref);
        let mut guards = dependencies;
        guards.push(version(intent.clone(), 1));
        let mut completed = prepared;
        completed["status"] = json!("completed");
        completed["reference"] = json!(candidate.reference);
        store.commit_changes(&PreparedChangeSet {
            command_id: command.command_id.clone(),
            fingerprint: fingerprint.clone(),
            read_set: ReadSet::new(guards)?,
            changes: vec![
                ResourceChange {
                    resource: binding,
                    value: json!(candidate.reference),
                },
                ResourceChange {
                    resource: intent,
                    value: completed,
                },
            ],
            result,
            time: now() as i64,
        })
    })();
    match outcome {
        Ok(receipt) => Ok(receipt),
        Err(error) => store
            .replay_commit(&command.command_id, &fingerprint)?
            .ok_or(error),
    }
}

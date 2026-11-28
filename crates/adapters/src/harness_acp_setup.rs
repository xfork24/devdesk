//! Provider-specific model and policy inputs for the shared ACP client.
use super::*;
use std::time::Duration;

pub(super) struct Setup {
    pub command: Vec<String>,
    pub servers: Vec<Value>,
    pub model: String,
    pub profile: Option<PathBuf>,
    pub profile_target: Option<String>,
    pub marker: Option<PathBuf>,
    pub acp: acp::Acp,
}

pub(crate) fn model_config(
    provider: HarnessProvider,
    connection: &ModelConnection,
    model: &str,
) -> Result<(Value, String)> {
    require(!connection.native, "ACP 需要冻结 Host 模型路由")?;
    match provider {
        HarnessProvider::Opencode => {
            let package = match connection.protocol.as_str() {
                "openai-completions" => "@opencode/ai/providers/openai-compatible",
                "anthropic-messages" => "@opencode/ai/providers/anthropic",
                _ => return Err(err("invalid_configuration", "OpenCode 模型协议未验证")),
            };
            let frozen = format!("devdesk/{model}");
            let endpoint = connection.worker_endpoint.trim_end_matches('/');
            // OpenCode's Anthropic package appends /messages to its base URL.
            let base_url =
                if connection.protocol == "anthropic-messages" && !endpoint.ends_with("/v1") {
                    format!("{endpoint}/v1")
                } else {
                    connection.worker_endpoint.clone()
                };
            let mut provider_config = json!({
                "package": package,
                "settings": {"baseURL": base_url},
                "models": {(model): {
                    "modelID": model,
                    "limit": {"context": connection.context_window, "output": connection.max_output_tokens},
                }},
            });
            if !connection.api_key_env.is_empty() {
                provider_config["env"] = json!([connection.api_key_env]);
            }
            Ok((
                json!({"model":frozen,"providers":{"devdesk":provider_config}}),
                frozen,
            ))
        }
        HarnessProvider::Mcode => {
            require(
                matches!(
                    connection.protocol.as_str(),
                    "openai-completions" | "anthropic-messages"
                ),
                "mcode 模型协议未验证",
            )?;
            let frozen = format!("custom_provider:devdesk/{model}");
            let key = if connection.api_key_env.is_empty() {
                "devdesk-local-model".into()
            } else {
                format!("${{{}}}", connection.api_key_env)
            };
            Ok((
                json!({"defaultModel":frozen,
                "custom_provider":{"devdesk":{"api":connection.protocol,"name":"DevDesk",
                    "options":{"baseURL":connection.worker_endpoint,"apiKey":key},
                    "models":{(model):{"name":model,"contextWindow":connection.context_window,"maxTokens":connection.max_output_tokens}}}}}),
                format!(
                    "m:custom_provider%3Adevdesk:{}:v:",
                    model
                        .bytes()
                        .map(|byte| {
                            if byte.is_ascii_alphanumeric() || b"-_.~".contains(&byte) {
                                (byte as char).to_string()
                            } else {
                                format!("%{byte:02X}")
                            }
                        })
                        .collect::<String>()
                ),
            ))
        }
        HarnessProvider::Zcode => {
            require(
                connection.protocol == "openai-completions" && !connection.api_key_env.is_empty(),
                "ZCode 当前需要已绑定的 OpenAI 兼容服务商",
            )?;
            let selection = json!({"providerId":"devdesk","modelId":model});
            Ok((
                json!({
                    "credential":connection.api_key_env, "model":model,
                    "providers":{"schemaVersion":1,"config":{
                        "providerConfigRules":{"providerRules":[{"providerId":"devdesk","providerName":"DevDesk",
                            "enabled":true,"config":{"group":"standard-personal","visibility":"visible",
                            "personalModelIds":[model],"modelOrder":[model],"access":{"type":"api-key"},
                            "api":{"type":"openai-chat-completions","baseUrl":connection.worker_endpoint,"headers":{}}}}]},
                        "modelConfigRules":{"providerModelRules":[],"manualProviderModelRules":[{
                            "providerId":"devdesk","modelId":model,"config":{"enabled":true,
                                "properties":{"contextWindow":connection.context_window,
                                    "inputFormat":{"supportsImage":false,"supportsVideo":false,"supportsPdf":false},
                                    "supportsJsonSchemaOutput":false,"supportsNativeWebSearch":false,"supportsMidConversationSystem":false},
                                "optionSpecs":{"maxOutputTokens":{"max":connection.max_output_tokens},
                                    "reasoningLevel":{"values":["disabled"],"map":"{}"}}}}]},
                        "providerOrder":["devdesk"],
                        "defaultModelSelection":{"providerId":selection["providerId"],"modelId":selection["modelId"],
                            "options":{"reasoningLevel":"disabled"}}
                    }}
                }),
                model.into(),
            ))
        }
        _ => unreachable!("ACP model configuration"),
    }
}

pub(super) struct Inputs<'a> {
    pub binding: &'a RuntimeBinding,
    pub connection: &'a ModelConnection,
    pub state: &'a Path,
    pub meta: &'a Path,
    pub operation: &'a str,
    pub instructions: &'a str,
}

pub(super) fn prepare(
    input: Inputs<'_>,
    wakeup: Arc<dyn Fn() + Send + Sync>,
    secrets: &mut Vec<String>,
) -> Result<Setup> {
    let Inputs {
        binding,
        connection,
        state,
        meta,
        operation,
        instructions,
    } = input;
    let provider = binding.installation.key.provider;
    let permissions = binding
        .permissions
        .as_ref()
        .ok_or_else(|| err("policy_not_enforced", "ACP 需要冻结权限"))?;
    permissions::validate_execution_permissions(permissions, provider)?;
    let tools = pi::tool_definitions(binding)?;
    let (bridge, url, token) = bridge::Bridge::start(
        bridge::listen_address()?,
        tools,
        wakeup,
        Duration::from_secs(110),
    )?;
    secrets.push(token.clone());
    let authorization = format!("Bearer {token}");
    let (mut config, model) = model_config(provider, connection, &binding.model)?;
    // Isolate executable configuration from the previous runtime process, and
    // keep provider history in the separately pinned session data directory.
    let component = format!("acp-{operation}");
    let private = state.join(&component);
    private_dir(&private)?;
    let in_container = format!("/state/{component}");
    write_private(&private.join("instructions"), instructions.as_bytes())?;
    let mut profile = None;
    let mut profile_target = None;
    let mut marker = None;
    let mut servers = Vec::new();
    let mut command = vec!["/usr/bin/env".into()];
    let config_path = match provider {
        HarnessProvider::Opencode => {
            let plugins = private.join("plugins");
            private_dir(&plugins)?;
            write_private(
                &plugins.join("devdesk.js"),
                include_bytes!("../../../permissions/backends/opencode/plugin.mjs"),
            )?;
            let data = state.join("opencode-data");
            private_dir(&data)?;
            marker = Some(private.join("policy-loaded"));
            config["instructions"] = json!([format!("{in_container}/instructions")]);
            config["mcp"] = json!({"servers":{"devdesk":{"type":"remote","url":url,
                "headers":{"Authorization":authorization},"oauth":false,"codemode":false,"protocol":"legacy","pool":false,"timeout":120000}}});
            command.extend([
                format!("OPENCODE_CONFIG_DIR={in_container}"),
                format!("XDG_CONFIG_HOME={in_container}/xdg"),
                format!("XDG_CACHE_HOME={in_container}/cache"),
                format!("XDG_STATE_HOME={in_container}/state"),
                "XDG_DATA_HOME=/state/opencode-data".into(),
                "OPENCODE_DISABLE_AUTOUPDATE=1".into(),
                "OPENCODE_DISABLE_MODELS_FETCH=1".into(),
                "OPENCODE_DISABLE_PROJECT_CONFIG=1".into(),
                "OPENCODE_DISABLE_CLAUDE_CODE=1".into(),
                "OPENCODE_DISABLE_FREE_MODELS=1".into(),
                format!("DEVDESK_POLICY_MARKER={in_container}/policy-loaded"),
                "opencode".into(),
                "acp".into(),
            ]);
            private.join("opencode.json")
        }
        HarnessProvider::Mcode => {
            let home = state.join("mcode");
            private_dir(&home)?;
            let path = meta.join("acp-hooks.json");
            write_private(&path, br#"{"enabled":true,"hooks":{"PreToolUse":[{"hooks":[{"type":"command","command":"node /run/devdesk/tool-policy.mjs"}]}]}}"#)?;
            profile = Some(path);
            profile_target = Some("/state/mcode/hooks.json".into());
            servers.push(json!({"name":"devdesk","type":"http","url":url,
                "headers":[{"name":"Authorization","value":authorization}]}));
            command.extend([
                "MINIMAX_DATA_DIR=/state/mcode".into(), "NODE_ENV=production".into(),
                "MAVIS_BUILD_ENV=production".into(), "NO_UPDATE_NOTIFIER=1".into(), "CI=1".into(),
                "/bin/sh".into(), "-ec".into(),
                "node /run/devdesk/tool-policy.mjs --probe; exec mcode acp --append-system-prompt-file \"$1\"".into(),
                "mcode-acp".into(), format!("{in_container}/instructions"),
            ]);
            home.join("config.yaml")
        }
        HarnessProvider::Zcode => {
            let home = state.join("zcode");
            private_dir(&home)?;
            let zcode_settings = home.join(".zcode");
            private_dir(&zcode_settings)?;
            let cli_settings = zcode_settings.join("cli");
            private_dir(&cli_settings)?;
            write_private(&private.join("bridge.mjs"), permissions::ZCODE_BRIDGE)?;
            let loaded =
                format!("node /run/devdesk/tool-policy.mjs --loaded {in_container}/policy-loaded");
            let settings = json!({
                "permission":{"mode":"build","allowedTools":["Read","Glob","Grep","LS"],
                    "disallowedTools":["Bash","Shell","Write","Edit","MultiEdit","NotebookEdit","Agent","Task","WebFetch","WebSearch"]},
                "mcp":{"servers":{"devdesk":{"type":"http","url":url,"protocolVersion":"legacy",
                    "headers":{"Authorization":authorization}}}},
                "hooks":{"enabled":true,"events":{
                    "SessionStart":[{"hooks":[{"type":"command","command":loaded,"onError":"deny"}]}],
                    "PreToolUse":[{"hooks":[{"type":"command","command":"node /run/devdesk/tool-policy.mjs","onError":"deny"}]}]
                }}
            });
            let settings = serde_json::to_vec(&settings).map_err(storage)?;
            write_private(&cli_settings.join("config.json"), &settings)?;
            write_private(&private.join("settings.json"), &settings)?;
            command.extend([
                "HOME=/state/zcode".into(),
                "ZCODE_DATA_BASE_DIR=/state/zcode".into(),
                "ZCODE_SESSION_DB_PATH=/state/zcode/session.sqlite".into(),
                format!("ZCODE_CLI_SETTINGS_FILE={in_container}/settings.json"),
                "node".into(),
                format!("{in_container}/bridge.mjs"),
                format!("{in_container}/config.json"),
                format!("{in_container}/instructions"),
                format!("{in_container}/policy-loaded"),
            ]);
            private.join("config.json")
        }
        _ => unreachable!("ACP runtime setup"),
    };
    write_private(&config_path, &serde_json::to_vec(&config).map_err(storage)?)?;
    Ok(Setup {
        command,
        servers,
        model,
        profile,
        profile_target,
        marker,
        acp: acp::Acp::new(bridge, private),
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn opencode_anthropic_base_includes_v1_once() {
        let mut connection: ModelConnection = serde_json::from_value(json!({
            "provider":"opencode-minimax-cn","kind":"cloud_subscription","route":"minimax",
            "protocol":"anthropic-messages","endpoint":"https://api.minimax.cn/anthropic",
            "worker_endpoint":"https://api.minimax.cn/anthropic","host_gateway":false,
            "api_key_env":"OPENCODE_MINIMAX_CN_API_KEY","context_window":32768,"max_output_tokens":4096,
        }))
        .unwrap();
        let base = |connection: &ModelConnection| {
            model_config(HarnessProvider::Opencode, connection, "MiniMax-M3.1-Flash")
                .unwrap()
                .0["providers"]["devdesk"]["settings"]["baseURL"]
                .clone()
        };
        assert_eq!(base(&connection), "https://api.minimax.cn/anthropic/v1");
        connection.worker_endpoint.push_str("/v1");
        assert_eq!(base(&connection), "https://api.minimax.cn/anthropic/v1");
        connection.protocol = "openai-completions".into();
        connection.worker_endpoint = "http://host.docker.internal:11434/v1/".into();
        assert_eq!(base(&connection), "http://host.docker.internal:11434/v1/");
    }
}

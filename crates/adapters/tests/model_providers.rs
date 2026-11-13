use devdesk_adapters::{configuration_digest, model_providers::*};
use devdesk_core::id;
use devdesk_domain::*;
use serde_json::json;
use std::{
    io::Write,
    process::{Command, Stdio},
};

/// The published provider facts, mirrored from resources/configuration sources.
fn facts(provider: &str) -> ProviderFacts {
    let (kind, endpoint, route, protocol, api_key_env, key, native, min, max, account) =
        match provider {
            "deepseek" => (
                "cloud_subscription",
                "https://api.deepseek.com",
                "deepseek-official",
                "deepseek-native",
                "DEEPSEEK_API_KEY",
                true,
                true,
                1024u64,
                Some(131072u64),
                "deepseek-api-key-",
            ),
            "zai-coding-cn" => (
                "cloud_subscription",
                "https://open.bigmodel.cn/api/coding/paas/v4",
                "zai-coding-cn",
                "openai-completions",
                "ZAI_CODING_CN_API_KEY",
                true,
                false,
                1,
                None,
                "dsh-zai-coding-cn-api-key-",
            ),
            "minimax-cn" => (
                "cloud_subscription",
                "https://api.minimax.cn/anthropic",
                "minimax-cn",
                "anthropic-messages",
                "MINIMAX_CN_API_KEY",
                true,
                false,
                1,
                None,
                "dsh-minimax-cn-api-key-",
            ),
            "ollama" => (
                "local_server",
                "http://127.0.0.1:11434/v1",
                "ollama",
                "openai-completions",
                "",
                false,
                false,
                1,
                None,
                "",
            ),
            _ => panic!("unknown test provider"),
        };
    serde_json::from_value(json!({
        "id": provider, "label": provider, "kind": kind, "harness": "dsh",
        "endpoint": endpoint, "route": route, "protocol": protocol,
        "api_key_env": api_key_env, "requires_key": key, "native": native,
        "capacity_required": provider != "deepseek",
        "min_output_request": min, "max_output_request": max,
        "credential_account": account
    }))
    .unwrap()
}
fn connection(provider: &str) -> ModelConnection {
    let m: ModelDefinition = serde_json::from_value(json!({"provider":provider,"model":"devdesk-mock-model","context_window":32768,"max_output_tokens":4096})).unwrap();
    resolve_model_connection(&m, &facts(provider))
        .unwrap()
        .unwrap()
}

#[test]
fn generated_profiles_select_only_the_frozen_route_without_secret_values() {
    for provider in ["deepseek", "zai-coding-cn", "minimax-cn", "ollama"] {
        let c = connection(provider);
        let patch = dsh_model_patch(&c, "devdesk-mock-model").unwrap();
        if !c.native {
            assert!(patch.contains(&c.api_key_env));
            assert!(patch.contains(&c.worker_endpoint));
            assert!(patch.contains("llm-deepseek\n  disabled: true"));
        }
        assert_eq!(configuration_digest(&json!(patch)).unwrap().len(), 64);
        assert!(!patch.contains("FAKE_DSH_TEST_VALUE"));
    }
}

#[test]
fn new_providers_require_a_complete_snapshot_and_matching_profile_digest() {
    let mut b: RuntimeBinding=serde_json::from_value(json!({"installation":{"id":id(),"key":{"provider":"dsh","version":devdesk_adapters::runtime_preparation::default_version(HarnessProvider::Dsh),"os":"linux","arch":"aarch64"},"artifact":{"kind":"docker_image","image_id":format!("sha256:{}","a".repeat(64))},"source":"fixture","license":"MIT"},"adapter_version":"fixture","model":"devdesk-mock-model","tool_policy":"read_only","work_type":"discussion","configuration_digest":"fixture","agent_configuration":{"agent_instance_id":id(),"agent_revision_id":id(),"composition_hash":"fixture","model_revision_id":id(),"harness_revision_id":id(),"model_provider":"ollama","model_parameters":{},"tool_revisions":[]}})).unwrap();
    assert!(binding_model_connection(&b).is_err());
    let c = connection("ollama");
    b.model_profile_digest =
        Some(configuration_digest(&json!(dsh_model_patch(&c, &b.model).unwrap())).unwrap());
    b.model_connection = Some(c);
    assert_eq!(binding_model_connection(&b).unwrap().provider, "ollama");
    b.model_profile_digest = Some("changed".into());
    assert!(binding_model_connection(&b).is_err());
    // The legacy fallback (resolving deepseek without a frozen snapshot) is gone:
    // bindings must carry the connection resolved at freeze time.
    b.model_connection = None;
    b.model_profile_digest = None;
    assert!(binding_model_connection(&b).is_err());
}

#[test]
fn ollama_directory_discovery_reads_host_metadata_without_inference() {
    use std::{io::Read, net::TcpListener};
    let server = TcpListener::bind("127.0.0.1:0").unwrap();
    let port = server.local_addr().unwrap().port();
    let thread = std::thread::spawn(move || {
        let (mut stream, _) = server.accept().unwrap();
        stream
            .set_read_timeout(Some(std::time::Duration::from_secs(5)))
            .unwrap();
        let mut bytes = [0u8; 8192];
        let n = stream.read(&mut bytes).unwrap();
        assert!(String::from_utf8_lossy(&bytes[..n]).starts_with("GET /v1/models "));
        let body = r#"{"data":[{"id":"devdesk-mock-model"}]}"#;
        write!(stream,"HTTP/1.1 200 OK\r\nContent-Type: application/json\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{body}",body.len()).unwrap();
    });
    let endpoint = format!("http://127.0.0.1:{port}/v1");
    let catalog = discover_ollama_models(&endpoint).unwrap();
    assert_eq!(catalog["source"], "host_endpoint");
    assert_eq!(catalog["models"], json!([{"id":"devdesk-mock-model"}]));
    assert_eq!(catalog["inference_verified"], false);
    assert_eq!(catalog["container_verified"], false);
    thread.join().unwrap();
    assert!(discover_ollama_models(&endpoint).is_err());
}

#[test]
#[ignore = "requires the pinned DSH Docker image; all inference is served by fixtures in --network=none"]
fn pinned_dsh_uses_local_fixture_streams_tools_and_native_resume() {
    let image =
        std::env::var("DEVDESK_DSH_MOCK_IMAGE").expect("set immutable DEVDESK_DSH_MOCK_IMAGE");
    assert!(image.starts_with("sha256:") && image.len() == 71);
    let cases: Vec<_> = ["zai-coding-cn","minimax-cn","ollama"].into_iter().map(|p| {
        let c=connection(p);
        json!({"provider":p,"patch":dsh_model_patch(&c,"devdesk-mock-model").unwrap(),"endpoint":c.worker_endpoint,"env":c.api_key_env})
    }).collect();
    let fixture = std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("../../tests/fixtures/protocols/dsh-model-providers.mjs")
        .canonicalize()
        .unwrap();
    let mut child = Command::new("docker")
        .args([
            "run",
            "--rm",
            "-i",
            "--network=none",
            "--read-only",
            "--cap-drop=ALL",
            "--security-opt=no-new-privileges",
            "--tmpfs",
            "/tmp:rw,nosuid,size=256m",
            "--mount",
        ])
        .arg(format!(
            "type=bind,source={},target=/probe.mjs,readonly",
            fixture.display()
        ))
        .args(["--entrypoint", "node", &image, "/probe.mjs"])
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .unwrap();
    child
        .stdin
        .take()
        .unwrap()
        .write_all(&serde_json::to_vec(&cases).unwrap())
        .unwrap();
    let output = child.wait_with_output().unwrap();
    assert!(
        output.status.success(),
        "{}\n{}",
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr)
    );
    let report: serde_json::Value = serde_json::from_slice(&output.stdout).unwrap();
    assert_eq!(report["reports"].as_array().unwrap().len(), 3);
    println!("{report}");
}

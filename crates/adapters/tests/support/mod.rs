//! Resolve a test installation on this Docker platform, then freeze its content ID.
fn metadata(provider: &str) -> &(String, String) {
    use std::sync::OnceLock;
    static CODEX: OnceLock<(String, String)> = OnceLock::new();
    static DSH: OnceLock<(String, String)> = OnceLock::new();
    let cache = match provider {
        "codex" => &CODEX,
        "dsh" => &DSH,
        _ => panic!("unknown test provider"),
    };
    cache.get_or_init(|| load(provider))
}
pub fn image(provider: &str) -> String {
    metadata(provider).0.clone()
}
#[allow(dead_code)]
pub fn platform(provider: &str) -> String {
    metadata(provider).1.clone()
}
fn load(provider: &str) -> (String, String) {
    let variable = format!("DEVDESK_TEST_{}_IMAGE", provider.to_ascii_uppercase());
    let reference = std::env::var(&variable).unwrap_or_else(|_| match provider {
        "codex" => "devdesk/codex:0.167.0".into(),
        "dsh" => "devdesk/dsh:0.2.2-rc.1".into(),
        _ => panic!("unknown test provider"),
    });
    let output = std::process::Command::new("docker")
        .args(["image", "inspect", &reference])
        .output()
        .expect("Docker must be installed");
    assert!(
        output.status.success(),
        "build the fixed {provider} image or set {variable}; images are never silently pulled"
    );
    let rows: serde_json::Value = serde_json::from_slice(&output.stdout).unwrap();
    assert_eq!(rows[0]["Os"], "linux");
    let identity = rows[0]["Id"].as_str().unwrap();
    assert!(identity.starts_with("sha256:") && identity.len() == 71);
    let arch = match rows[0]["Architecture"].as_str().unwrap() {
        "arm64" => "aarch64",
        "amd64" => "x86_64",
        _ => panic!("unsupported Docker architecture"),
    };
    (identity.into(), format!("linux/{arch}"))
}

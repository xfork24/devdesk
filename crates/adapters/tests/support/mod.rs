//! Resolve a test installation on this Docker platform, then freeze its content ID.
fn metadata(provider: &str) -> &(String, String) {
    use std::sync::OnceLock;
    static CODEX: OnceLock<(String, String)> = OnceLock::new();
    static PROJECT: OnceLock<(String, String)> = OnceLock::new();
    static DSH: OnceLock<(String, String)> = OnceLock::new();
    let cache = match provider {
        "codex" => &CODEX,
        "dsh" => &DSH,
        "project" => &PROJECT,
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
    let reference = std::env::var(&variable).unwrap_or_else(|_| {
        if provider == "project" {
            "devdesk/project-tests:1".into()
        } else {
            panic!(
                "set {variable} to an explicitly prepared image; use native_acceptance.py --prepare"
            )
        }
    });
    let output = std::process::Command::new("docker")
        .args(["image", "inspect", &reference])
        .output()
        .expect("Docker must be installed");
    assert!(
        output.status.success(),
        "prepare the selected {provider} recipe and set {variable}; images are never silently pulled"
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

//! Protocol and build asset versions; business defaults are configuration resources.
use crate::*;

pub const DEFAULT_DSH_VERSION: &str = "0.2.2-rc.1";
pub const DEFAULT_CODEX_VERSION: &str = "0.167.0";

pub fn default_codex_recipe_digest() -> String {
    configuration_hash(&[(
        "Dockerfile",
        include_str!("../../../containers/codex/Dockerfile"),
    )])
}

/// Matches the embedded DSH runtime recipe; sessions pin its immutable materialization.
pub fn default_dsh_recipe_digest() -> String {
    configuration_hash(&[
        (
            "Dockerfile",
            include_str!("../../../containers/dsh/Dockerfile"),
        ),
        (
            "permissions/backends/dsh/package.json",
            include_str!("../../../permissions/backends/dsh/package.json"),
        ),
        (
            "permissions/backends/dsh/tool-policy.mjs",
            include_str!("../../../permissions/backends/dsh/tool-policy.mjs"),
        ),
        (
            "permissions/backends/dsh/bwrap-runner.sh",
            include_str!("../../../permissions/backends/dsh/bwrap-runner.sh"),
        ),
    ])
}

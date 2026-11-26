//! What a domain package asks of the kernel, and what this Host answers.
use crate::{extensions, platform_tools, rpc_contracts};
use devdesk_domain::{Result, err};
use serde::{Deserialize, Serialize};

pub const PACKAGE_FORMAT: u32 = 2;
pub const DOMAIN_PROTOCOL: u32 = 2;

/// The kernel surface a package depends on. Its build tool derives the lists from the package.
#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Requires {
    /// The lowest Host version the package runs on.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub host: Option<String>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub handlers: Vec<String>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub extensions: Vec<String>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub rpc: Vec<String>,
}

/// What one extension group provides on this Host.
#[derive(Clone, Debug, PartialEq, Eq, Serialize)]
pub struct ExtensionCapabilities {
    pub id: &'static str,
    pub version: u32,
    /// Platform tool handlers only a package requiring this group may call.
    pub handlers: Vec<&'static str>,
    /// Kernel inputs only such a package may declare for its handlers.
    pub core_inputs: Vec<&'static str>,
    pub hooks: Vec<&'static str>,
}

impl ExtensionCapabilities {
    /// The name a package writes in `requires.extensions`.
    pub fn reference(&self) -> String {
        format!("{}/{}", self.id, self.version)
    }
}

/// The surface this Host provides: the answer to every entry of [Requires].
#[derive(Clone, Debug, PartialEq, Eq, Serialize)]
pub struct Capabilities {
    pub host_version: String,
    pub package_format: u32,
    pub domain_protocol: u32,
    pub platform_handlers: Vec<&'static str>,
    /// Kernel inputs available to every package.
    pub core_inputs: Vec<&'static str>,
    pub extensions: Vec<ExtensionCapabilities>,
    pub rpc: Vec<String>,
}

impl Capabilities {
    pub fn current() -> Self {
        Self {
            host_version: env!("CARGO_PKG_VERSION").into(),
            package_format: PACKAGE_FORMAT,
            domain_protocol: DOMAIN_PROTOCOL,
            platform_handlers: platform_tools::platform_handlers().to_vec(),
            core_inputs: extensions::KERNEL_INPUTS.to_vec(),
            extensions: extensions::EXTENSION_GROUPS
                .iter()
                .map(|group| ExtensionCapabilities {
                    id: group.id,
                    version: group.version,
                    handlers: group.handlers.to_vec(),
                    core_inputs: group.core_inputs.to_vec(),
                    hooks: group.hooks.iter().map(|hook| hook.name).collect(),
                })
                .collect(),
            rpc: rpc_contracts::inventory()
                .values()
                .filter(|contract| contract.access != rpc_contracts::Access::Retired)
                .map(|contract| contract.method.clone())
                .collect(),
        }
    }

    /// The package is written for the format and protocol this Host speaks.
    pub fn admit(&self, format: u32, protocol: u32) -> Result<()> {
        if format != self.package_format || protocol != self.domain_protocol {
            return Err(err(
                "domain_protocol_unsupported",
                format!(
                    "领域包使用格式 {format}、协议 {protocol}；本 Host 只接受格式 {}、协议 {}",
                    self.package_format, self.domain_protocol
                ),
            ));
        }
        Ok(())
    }

    /// Every entry of `requires` exists on this Host. Each failure names what is missing.
    pub fn negotiate(&self, requires: &Requires) -> Result<()> {
        if let Some(minimum) = &requires.host
            && !version_at_least(&self.host_version, minimum)?
        {
            return Err(err(
                "host_version_unsupported",
                format!(
                    "领域包要求 Host 版本不低于 {minimum}；本 Host 是 {}",
                    self.host_version
                ),
            ));
        }
        let absent = missing(&requires.handlers, &self.platform_handlers);
        if !absent.is_empty() {
            return Err(err(
                "handler_unsupported",
                format!("本 Host 没有这些平台处理器：{}", absent.join("、")),
            ));
        }
        let absent = missing(
            &requires.extensions,
            &self
                .extensions
                .iter()
                .map(ExtensionCapabilities::reference)
                .collect::<Vec<_>>(),
        );
        if !absent.is_empty() {
            return Err(err(
                "extension_unavailable",
                format!("本 Host 没有这些扩展组：{}", absent.join("、")),
            ));
        }
        let absent = missing(&requires.rpc, &self.rpc);
        if !absent.is_empty() {
            return Err(err(
                "rpc_unsupported",
                format!("本 Host 没有这些 RPC：{}", absent.join("、")),
            ));
        }
        Ok(())
    }
}

/// The required names the Host does not provide.
fn missing(required: &[String], provided: &[impl AsRef<str>]) -> Vec<String> {
    required
        .iter()
        .filter(|name| {
            !provided
                .iter()
                .any(|provided| provided.as_ref() == name.as_str())
        })
        .cloned()
        .collect()
}

/// Numeric `major.minor.patch` comparison. A pre-release or build suffix is not compared;
/// a version that is not three numbers at most cannot be negotiated.
fn version_at_least(actual: &str, minimum: &str) -> Result<bool> {
    let numbers = |version: &str| {
        let mut parts = [0u64; 3];
        let core = version.split(['-', '+']).next().unwrap_or_default();
        if core.split('.').count() > 3 {
            return Err(err(
                "host_version_unsupported",
                format!("无法比较 Host 版本 {version}"),
            ));
        }
        for (slot, part) in parts.iter_mut().zip(core.split('.')) {
            *slot = part.parse().map_err(|_| {
                err(
                    "host_version_unsupported",
                    format!("无法比较 Host 版本 {version}"),
                )
            })?;
        }
        Ok(parts)
    };
    Ok(numbers(actual)? >= numbers(minimum)?)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn versions_compare_numerically_up_to_the_patch_component() {
        for (actual, minimum) in [
            ("0.6.0", "0.6.0"),
            ("0.6.1", "0.6.0"),
            ("0.7.0", "0.6.9"),
            ("1.0.0", "0.99.99"),
            ("0.6", "0.6.0"),
        ] {
            assert!(
                version_at_least(actual, minimum).unwrap(),
                "{actual} >= {minimum}"
            );
        }
        for (actual, minimum) in [("0.10.0", "0.9.0"), ("10.0.0", "9.0.0")] {
            assert!(
                version_at_least(actual, minimum).unwrap(),
                "{actual} >= {minimum}"
            );
        }
        for (actual, minimum) in [("0.5.9", "0.6.0"), ("0.6.0", "0.6.1"), ("0.9.0", "0.10.0")] {
            assert!(
                !version_at_least(actual, minimum).unwrap(),
                "{actual} < {minimum}"
            );
        }
        assert!(version_at_least("dev", "0.6.0").is_err());
        assert!(version_at_least("0.6.0", "next").is_err());
    }

    #[test]
    fn negotiation_names_what_the_host_lacks() {
        let capabilities = Capabilities::current();
        capabilities.admit(PACKAGE_FORMAT, DOMAIN_PROTOCOL).unwrap();
        assert_eq!(
            capabilities
                .admit(PACKAGE_FORMAT - 1, DOMAIN_PROTOCOL)
                .unwrap_err()
                .code,
            "domain_protocol_unsupported"
        );
        capabilities.negotiate(&Requires::default()).unwrap();
        let requires = Requires {
            host: Some("0.0.1".into()),
            handlers: vec![capabilities.platform_handlers[0].to_owned()],
            extensions: capabilities
                .extensions
                .iter()
                .map(ExtensionCapabilities::reference)
                .collect(),
            rpc: vec![capabilities.rpc[0].clone()],
        };
        capabilities.negotiate(&requires).unwrap();
        // A group's handlers and kernel inputs are published with the group.
        let git = capabilities
            .extensions
            .iter()
            .find(|group| group.id == "engineering.git")
            .unwrap();
        assert_eq!(git.reference(), "engineering.git/1");
        assert!(git.handlers.contains(&"workspace.snapshot"));
        assert!(git.core_inputs.contains(&"engineering.git.snapshot"));
        assert!(git.hooks.contains(&"pull_request_creation"));
        let too_new = Requires {
            host: Some("999.0.0".into()),
            ..Default::default()
        };
        assert_eq!(
            capabilities.negotiate(&too_new).unwrap_err().code,
            "host_version_unsupported"
        );
        let handler = Requires {
            handlers: vec!["workflow.teleport".into()],
            ..Default::default()
        };
        assert_eq!(
            capabilities.negotiate(&handler).unwrap_err().code,
            "handler_unsupported"
        );
        let group = Requires {
            extensions: vec!["engineering.git/9".into()],
            ..Default::default()
        };
        assert_eq!(
            capabilities.negotiate(&group).unwrap_err().code,
            "extension_unavailable"
        );
        let rpc = Requires {
            rpc: vec!["query.nowhere".to_owned()],
            ..Default::default()
        };
        assert_eq!(
            capabilities.negotiate(&rpc).unwrap_err().code,
            "rpc_unsupported"
        );
    }
}

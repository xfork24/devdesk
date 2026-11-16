//! Explicit ingress inventory shared by admission and regression checks.
use serde::Deserialize;
use std::{collections::BTreeMap, sync::OnceLock};

#[derive(Debug, Clone, Copy, PartialEq, Eq, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Access {
    Read,
    Mutation,
    PlatformRead,
    PlatformMutation,
    Retired,
}

#[derive(Debug, Deserialize)]
pub struct MethodContract {
    pub method: String,
    pub access: Access,
    pub scope: String,
    pub precondition: String,
    pub sites: Vec<String>,
}

#[derive(Deserialize)]
struct Inventory {
    entries: Vec<MethodContract>,
}

pub fn inventory() -> &'static BTreeMap<String, MethodContract> {
    static INVENTORY: OnceLock<BTreeMap<String, MethodContract>> = OnceLock::new();
    INVENTORY.get_or_init(|| {
        let inventory: Inventory = serde_json::from_str(include_str!("rpc_contracts.json"))
            .expect("validated RPC contract inventory");
        inventory
            .entries
            .into_iter()
            .map(|contract| (contract.method.clone(), contract))
            .collect()
    })
}

pub fn contract(method: &str) -> Option<&'static MethodContract> {
    inventory().get(method)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn all_mutating_ingresses_declare_a_version_strategy() {
        let source: Inventory = serde_json::from_str(include_str!("rpc_contracts.json")).unwrap();
        assert_eq!(source.entries.len(), inventory().len());
        for contract in inventory().values() {
            assert!(!contract.scope.is_empty());
            assert!(!contract.sites.is_empty());
            if matches!(contract.access, Access::Mutation | Access::PlatformMutation) {
                assert!(!contract.precondition.is_empty());
                assert_ne!(contract.precondition, "none", "{}", contract.method);
            }
        }
        assert_eq!(
            contract("settings.update").unwrap().access,
            Access::Mutation
        );
        assert_eq!(
            contract("codex.auth.models").unwrap().access,
            Access::Mutation
        );
        assert_eq!(contract("query.workspace").unwrap().access, Access::Read);
        assert!(contract("unregistered.new_method").is_none());
    }
}

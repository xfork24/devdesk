//! Executable protocol facts are supplied by the installed adapters, never by source declarations.
use crate::*;
use std::{
    collections::{BTreeMap, BTreeSet},
    sync::OnceLock,
};
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct DriverCapabilities {
    pub implementation_id: String,
    pub features: BTreeSet<String>,
}
static DRIVERS: OnceLock<BTreeMap<String, DriverCapabilities>> = OnceLock::new();
pub fn install_driver_capabilities(drivers: BTreeMap<String, DriverCapabilities>) -> Result<()> {
    require(
        drivers.values().all(|d| !d.implementation_id.is_empty()),
        "驱动能力必须有实际实现身份",
    )?;
    if let Some(current) = DRIVERS.get() {
        return require(current == &drivers, "当前进程的驱动能力已初始化为其他实现");
    }
    let expected = drivers.clone();
    if DRIVERS.set(drivers).is_err() {
        require(DRIVERS.get() == Some(&expected), "驱动注册并发冲突")?;
    }
    Ok(())
}
pub fn driver_supports(provider: &str, feature: &str) -> bool {
    DRIVERS
        .get()
        .and_then(|d| d.get(provider))
        .is_some_and(|d| d.features.contains(feature))
}
pub fn driver_capabilities() -> BTreeMap<String, DriverCapabilities> {
    DRIVERS.get().cloned().unwrap_or_default()
}

use serde::{Deserialize, Serialize};
use specta::Type;

use crate::core::clash::api;

#[derive(Debug, Clone, Deserialize, Serialize, Default, Type)]
pub struct ProxyGroupItem {
    /// 1
    #[serde(default)]
    pub hidden: bool, // Mihomo Only
    /// 2
    pub name: String,
    /// 3
    pub all: Vec<api::ProxyItem>,
    /// 4
    pub history: Vec<api::ProxyItemHistory>,
}

#[derive(Debug, Clone, Deserialize, Serialize, Default, Type)]
#[serde(rename_all = "camelCase")]
pub struct Proxies {
    pub groups: Vec<ProxyGroupItem>,
    /// 2
    pub name: String,
    /// 3
    pub global: ProxyGroupItem,
}

//! Running-core controller payloads and application IPC DTO projections.
//! The instance-bound protocol capability lives in actor_v2::api.

use indexmap::IndexMap;
use serde::{Deserialize, Serialize};
use specta::Type;
use tracing::instrument;

/// TUN runtime projection returned by the running core's `GET /configs` endpoint.
/// Legacy runtime patches, traffic and agent probing still use this expanded
/// projection; keep it separate from the public API DTO.
#[derive(Debug, Clone, Default, Deserialize, Serialize, Type)]
#[serde(default, rename_all = "kebab-case")]
pub struct RuntimeTun {
    pub enable: bool,
    pub device: String,
    pub auto_route: bool,
    pub auto_detect_interface: bool,
    pub strict_route: bool,
    pub route_address: Vec<String>,
    pub route_exclude_address: Vec<String>,
    pub inet4_route_address: Vec<String>,
    pub inet6_route_address: Vec<String>,
}

/// Expanded running-core config projection for Chimera's legacy runtime
/// patch/rollback, geodata and agent paths. Not the frontend API contract.
#[derive(Debug, Clone, Default, Deserialize, Serialize, Type)]
pub struct ClashRuntimeConfig {
    pub port: Option<u16>,
    pub mode: Option<String>,
    #[serde(rename = "geodata-mode")]
    pub geodata_mode: Option<bool>,
    pub ipv6: Option<bool>,
    #[serde(rename = "socket-port")]
    pub socket_port: Option<u16>,
    #[serde(rename = "allow-lan")]
    pub allow_lan: Option<bool>,
    #[serde(rename = "log-level")]
    pub log_level: Option<String>,
    #[serde(rename = "mixed-port")]
    pub mixed_port: Option<u16>,
    #[serde(rename = "redir-port")]
    pub redir_port: Option<u16>,
    #[serde(rename = "socks-port")]
    pub socks_port: Option<u16>,
    #[serde(rename = "tproxy-port")]
    pub tproxy_port: Option<u16>,
    #[serde(rename = "external-controller")]
    pub external_controller: Option<String>,
    pub secret: Option<String>,
    #[specta(skip)]
    pub tun: Option<RuntimeTun>,
}

/// Public controller config DTO. Fields and JSON names mirror ref's
/// `nyanpasu-core::clash::api::ClashConfig` (exported as ClashApiConfig).
/// The Rust name differs to avoid confusion with Chimera's persisted ClashConfig.
#[derive(Debug, Clone, Default, Deserialize, Serialize, Type)]
pub struct ClashApiConfig {
    pub port: Option<u16>,
    pub mode: Option<String>,
    pub ipv6: Option<bool>,
    #[serde(rename = "socket-port")]
    pub socket_port: Option<u16>,
    #[serde(rename = "allow-lan")]
    pub allow_lan: Option<bool>,
    #[serde(rename = "log-level")]
    pub log_level: Option<String>,
    #[serde(rename = "mixed-port")]
    pub mixed_port: Option<u16>,
    #[serde(rename = "redir-port")]
    pub redir_port: Option<u16>,
    #[serde(rename = "socks-port")]
    pub socks_port: Option<u16>,
    #[serde(rename = "tproxy-port")]
    pub tproxy_port: Option<u16>,
    #[serde(rename = "external-controller")]
    pub external_controller: Option<String>,
    pub secret: Option<String>,
}

#[derive(Debug, Clone, Deserialize, Serialize, Type)]
pub struct ClashVersion {
    pub version: String,
    pub premium: Option<bool>,
    pub meta: Option<bool>,
}

#[derive(Debug, Clone, Deserialize, Serialize, Type)]
pub struct ClashRule {
    pub r#type: String,
    pub payload: String,
    pub proxy: String,
}

#[derive(Debug, Clone, Deserialize, Serialize, Type)]
pub struct RulesRes {
    pub rules: Vec<ClashRule>,
}

#[derive(Debug, Clone, Deserialize, Serialize, Type)]
pub struct RuleProviderItem {
    pub behavior: Option<String>,
    pub format: Option<String>,
    pub name: String,
    #[serde(rename = "ruleCount")]
    pub rule_count: Option<u32>,
    pub r#type: Option<String>,
    #[serde(rename = "updatedAt")]
    pub updated_at: Option<String>,
    #[serde(rename = "vehicleType")]
    pub vehicle_type: Option<String>,
}

#[derive(Debug, Clone, Deserialize, Serialize, Type)]
pub struct ProvidersRulesRes {
    pub providers: IndexMap<String, RuleProviderItem>,
}

/// 缩短clash的日志
#[instrument]
pub fn parse_log(log: String) -> String {
    if log.starts_with("time=") && log.len() > 33 {
        return log[33..].to_owned();
    }
    if log.len() > 9 {
        return log[9..].to_owned();
    }
    log
}

#[derive(Debug, Clone, Deserialize, Serialize, Default, Type)]
#[serde(rename_all = "camelCase")]
pub struct ProxyItemHistory {
    pub time: String,
    pub delay: i64,
}

#[derive(Debug, Clone, Deserialize, Serialize, Default, Type)]
#[serde(rename_all = "camelCase")]
pub struct ProxyItem {
    pub name: String,
    pub history: Vec<ProxyItemHistory>,
    pub all: Option<Vec<String>>,
    #[serde(default)]
    pub hidden: bool, // Mihomo Only
    pub now: Option<String>, // 当前选中的代理
    pub r#type: String,      // TODO: 考虑改成枚举
    pub udp: bool,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub icon: Option<String>, // Mihomo Only
}

#[derive(Debug, Clone, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ConnectionItem {
    pub id: String,
    #[serde(default)]
    pub chains: Vec<String>,
}

#[derive(Debug, Clone, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ConnectionsRes {
    #[serde(default)]
    pub connections: Vec<ConnectionItem>,
}

#[derive(Default, Debug, Clone, Deserialize, Serialize, Type)]
pub struct DelayRes {
    pub(crate) delay: u64,
}

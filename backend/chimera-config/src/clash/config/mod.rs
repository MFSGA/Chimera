pub mod clash_strategy;
pub mod overrides;
pub mod tun_stack;

use serde::{Deserialize, Serialize};
use specta::Type;
use struct_patch::Patch;

use clash_strategy::*;
use overrides::*;
use tun_stack::*;

/// Clash Default mixed-port
pub const DEFAULT_MIXED_PORT: u16 = 7890;
/// Clash Default external-controller port
#[cfg(debug_assertions)]
pub const DEFAULT_EXTERNAL_CONTROLLER_PORT: u16 = 9872;
#[cfg(not(debug_assertions))]
pub const DEFAULT_EXTERNAL_CONTROLLER_PORT: u16 = 17650;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Deserialize, Serialize, Default, Type)]
#[serde(rename_all = "snake_case")]
pub enum ClashControlChannel {
    #[default]
    PreferIpc,
    HttpOnly,
}

/// Clash Related Config
#[derive(Default, Debug, Clone, Deserialize, Serialize, Type, Patch)]
#[patch(attribute(serde_with::skip_serializing_none))]
#[patch(attribute(derive(Debug, Default, Clone, Serialize, Deserialize, Type)))]
#[patch(attribute(serde(rename_all = "snake_case")))]
#[serde(rename_all = "snake_case")]
pub struct ClashConfig {
    /// Clash Overrides config, used to patch clash config directly
    #[patch(attribute(specta(optional)))]
    pub overrides: ClashGuardOverrides,

    /// clash tun mode
    #[patch(attribute(specta(optional)))]
    pub enable_tun_mode: bool,

    /// web ui list
    #[patch(attribute(specta(optional)))]
    pub web_ui_list: Vec<String>,

    /// 支持关闭字段过滤，避免meta的新字段都被过滤掉，默认为真
    #[patch(attribute(specta(optional)))]
    pub enable_clash_fields: bool,

    /// 外部控制器端口策略
    #[patch(nesting)]
    #[patch(attribute(specta(optional)))]
    #[patch(attribute(serde(default)))]
    pub external_controller: ExternalControllerStrategy,

    #[serde(default)]
    #[patch(attribute(specta(optional)))]
    pub clash_control_channel: ClashControlChannel,
    #[serde(default)]
    #[patch(attribute(specta(optional)))]
    pub clash_ipc_disable_http_controller: bool,

    /// Mixed Proxy(Socks5, HTTP) Port Strategy
    #[patch(nesting)]
    #[patch(attribute(specta(optional)))]
    #[patch(attribute(serde(default)))]
    pub mixed_port: PortStrategy,

    /// Socks5 Proxy Port
    #[patch(attribute(serde(default, with = "::serde_with::rust::double_option")))]
    #[patch(attribute(specta(type = Option<Option<PortStrategy>>)))]
    #[patch(attribute(specta(optional)))]
    pub socks_port: Option<PortStrategy>,

    /// HTTP Proxy Port
    #[patch(attribute(serde(default, with = "::serde_with::rust::double_option")))]
    #[patch(attribute(specta(type = Option<Option<PortStrategy>>)))]
    #[patch(attribute(specta(optional)))]
    pub http_port: Option<PortStrategy>,

    /// Transparent proxy listener ports. `None` leaves profile-provided ports intact.
    #[patch(attribute(serde(default, with = "::serde_with::rust::double_option")))]
    #[patch(attribute(specta(type = Option<Option<PortStrategy>>)))]
    pub redir_port: Option<PortStrategy>,

    #[patch(attribute(serde(default, with = "::serde_with::rust::double_option")))]
    #[patch(attribute(specta(type = Option<Option<PortStrategy>>)))]
    pub tproxy_port: Option<PortStrategy>,

    /// Capture policy applied by the platform network service.
    #[serde(default)]
    #[patch(nesting)]
    #[patch(attribute(serde(default)))]
    #[patch(attribute(specta(optional)))]
    pub transparent_proxy: TransparentProxyConfig,

    /// 断开连接策略
    #[patch(nesting)]
    #[patch(attribute(specta(optional)))]
    #[patch(attribute(serde(default)))]
    pub break_connection: BreakConnectionStrategy,

    /// Tun 堆栈选择
    #[patch(attribute(specta(optional)))]
    pub tun_stack: TunStack,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Deserialize, Serialize, Default, Type)]
#[serde(rename_all = "snake_case")]
pub enum TransparentProxyMode {
    #[default]
    Disabled,
    Redir,
    Tproxy,
}

#[derive(Debug, Clone, PartialEq, Eq, Deserialize, Serialize, Type, Patch)]
#[patch(attribute(derive(Debug, Default, Clone, Serialize, Deserialize, PartialEq, Eq, Type)))]
#[patch(attribute(serde(default, rename_all = "snake_case")))]
#[serde(default, rename_all = "snake_case")]
pub struct TransparentProxyConfig {
    pub mode: TransparentProxyMode,
    pub local: bool,
    pub interfaces: Vec<String>,
    pub ipv6: bool,
}

impl Default for TransparentProxyConfig {
    fn default() -> Self {
        Self {
            mode: TransparentProxyMode::Disabled,
            local: true,
            interfaces: Vec::new(),
            ipv6: false,
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error, Serialize, Type)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum TransparentProxyValidationError {
    #[error("transparent proxy listeners require Linux and Mihomo")]
    UnsupportedTarget,
    #[error("transparent proxy capture cannot be combined with TUN")]
    TunConflict,
    #[error("{mode:?} capture requires its corresponding managed listener port")]
    MissingListener { mode: TransparentProxyMode },
    #[error("transparent proxy listener port must be nonzero")]
    ZeroPort,
    #[error("transparent proxy capture needs local traffic or at least one interface")]
    EmptyTarget,
    #[error("invalid transparent proxy interface name: {name}")]
    InvalidInterface { name: String },
    #[error("transparent proxy interface list contains duplicates")]
    DuplicateInterface,
    #[error("transparent proxy supports at most 16 interfaces")]
    TooManyInterfaces,
}

impl ClashConfig {
    /// Validate the network policy and listener settings for one runtime target.
    pub fn validate_transparent_proxy(
        &self,
        core: crate::application::ClashCore,
        linux: bool,
    ) -> Result<(), TransparentProxyValidationError> {
        use crate::application::ClashCore;
        use TransparentProxyMode::{Disabled, Redir, Tproxy};

        let capture_enabled = self.transparent_proxy.mode != Disabled;
        if (capture_enabled || self.redir_port.is_some() || self.tproxy_port.is_some())
            && (!linux || !matches!(core, ClashCore::Mihomo | ClashCore::MihomoAlpha))
        {
            return Err(TransparentProxyValidationError::UnsupportedTarget);
        }
        if capture_enabled && self.enable_tun_mode {
            return Err(TransparentProxyValidationError::TunConflict);
        }
        if self
            .redir_port
            .iter()
            .chain(self.tproxy_port.iter())
            .any(|port| port.kind != PortStrategyKind::Random && port.start_port == 0)
        {
            return Err(TransparentProxyValidationError::ZeroPort);
        }
        match self.transparent_proxy.mode {
            Disabled => return Ok(()),
            Redir if self.redir_port.is_none() => {
                return Err(TransparentProxyValidationError::MissingListener { mode: Redir });
            }
            Tproxy if self.tproxy_port.is_none() => {
                return Err(TransparentProxyValidationError::MissingListener { mode: Tproxy });
            }
            Redir | Tproxy => {}
        }
        if !self.transparent_proxy.local && self.transparent_proxy.interfaces.is_empty() {
            return Err(TransparentProxyValidationError::EmptyTarget);
        }
        if self.transparent_proxy.interfaces.len() > 16 {
            return Err(TransparentProxyValidationError::TooManyInterfaces);
        }
        let mut seen = std::collections::HashSet::new();
        for name in &self.transparent_proxy.interfaces {
            if name.is_empty()
                || name.len() > 15
                || !name
                    .bytes()
                    .all(|byte| byte.is_ascii_alphanumeric() || b"_.-".contains(&byte))
            {
                return Err(TransparentProxyValidationError::InvalidInterface {
                    name: name.clone(),
                });
            }
            if !seen.insert(name) {
                return Err(TransparentProxyValidationError::DuplicateInterface);
            }
        }
        Ok(())
    }
}

#[cfg(test)]
mod patch_tests {
    use super::*;
    use struct_patch::Patch;

    #[test]
    fn nested_patches_preserve_unmodified_strategy_fields() {
        let mut config = ClashConfig {
            mixed_port: PortStrategy {
                kind: PortStrategyKind::Random,
                start_port: 18080,
            },
            ..ClashConfig::default()
        };
        let patch: ClashConfigPatch = serde_yaml_ng::from_str(
            "mixed_port:\n  start_port: 28080\nbreak_connection:\n  on_mode_change: false\n",
        )
        .expect("sparse nested patch must deserialize");

        config.apply(patch);

        assert_eq!(config.mixed_port.kind, PortStrategyKind::Random);
        assert_eq!(config.mixed_port.start_port, 28080);
        assert!(!config.break_connection.on_mode_change);
        assert!(config.break_connection.on_profile_change);
    }

    #[test]
    fn old_clash_config_defaults_control_channel_without_application_fields() {
        let mut yaml = serde_yaml_ng::to_value(ClashConfig::default()).unwrap();
        let mapping = yaml.as_mapping_mut().unwrap();
        mapping.remove("clash_control_channel");
        mapping.remove("clash_ipc_disable_http_controller");
        mapping.remove("redir_port");
        mapping.remove("tproxy_port");
        mapping.remove("transparent_proxy");
        let config: ClashConfig = serde_yaml_ng::from_value(yaml).unwrap();
        assert_eq!(config.clash_control_channel, ClashControlChannel::PreferIpc);
        assert!(!config.clash_ipc_disable_http_controller);
        assert_eq!(config.redir_port, None);
        assert_eq!(config.tproxy_port, None);
        assert_eq!(config.transparent_proxy, TransparentProxyConfig::default());
        let app = serde_yaml_ng::to_value(crate::application::ChimeraAppConfig::default()).unwrap();
        assert!(app.get("clash_control_channel").is_none());
        assert!(app.get("clash_ipc_disable_http_controller").is_none());
    }

    #[test]
    fn transparent_proxy_validation_checks_platform_mode_and_listener() {
        let mut config = ClashConfig::default();
        config.transparent_proxy.mode = TransparentProxyMode::Tproxy;
        assert!(matches!(
            config.validate_transparent_proxy(crate::application::ClashCore::Mihomo, true),
            Err(TransparentProxyValidationError::MissingListener {
                mode: TransparentProxyMode::Tproxy
            })
        ));

        config.tproxy_port = Some(PortStrategy::new_allow_fallback(7894));
        assert!(
            config
                .validate_transparent_proxy(crate::application::ClashCore::Mihomo, true)
                .is_ok()
        );
        assert!(matches!(
            config.validate_transparent_proxy(crate::application::ClashCore::ClashRs, true),
            Err(TransparentProxyValidationError::UnsupportedTarget)
        ));
        assert!(matches!(
            config.validate_transparent_proxy(crate::application::ClashCore::Mihomo, false),
            Err(TransparentProxyValidationError::UnsupportedTarget)
        ));
        config.enable_tun_mode = true;
        assert!(matches!(
            config.validate_transparent_proxy(crate::application::ClashCore::Mihomo, true),
            Err(TransparentProxyValidationError::TunConflict)
        ));
    }

    #[test]
    fn transparent_proxy_validation_checks_targets_and_zero_port() {
        let mut config = ClashConfig::default();
        config.redir_port = Some(PortStrategy {
            kind: PortStrategyKind::Fixed,
            start_port: 0,
        });
        assert!(matches!(
            config.validate_transparent_proxy(crate::application::ClashCore::Mihomo, true),
            Err(TransparentProxyValidationError::ZeroPort)
        ));

        config.redir_port = Some(PortStrategy::new_allow_fallback(7893));
        config.transparent_proxy.mode = TransparentProxyMode::Redir;
        config.transparent_proxy.local = false;
        assert!(matches!(
            config.validate_transparent_proxy(crate::application::ClashCore::Mihomo, true),
            Err(TransparentProxyValidationError::EmptyTarget)
        ));
        config.transparent_proxy.interfaces = vec!["eth0".into(), "eth0".into()];
        assert!(matches!(
            config.validate_transparent_proxy(crate::application::ClashCore::Mihomo, true),
            Err(TransparentProxyValidationError::DuplicateInterface)
        ));
        config.transparent_proxy.interfaces = vec!["interface-name-too-long".into()];
        assert!(matches!(
            config.validate_transparent_proxy(crate::application::ClashCore::Mihomo, true),
            Err(TransparentProxyValidationError::InvalidInterface { .. })
        ));
    }

    /// `socks_port`/`http_port` are `Option<PortStrategy>` originals under the
    /// struct-level `skip_serializing_none` + field-level `double_option` combo:
    /// absent keeps, explicit `null` clears, and absent is skipped on serialize.
    #[test]
    fn optional_ports_clear_keep_and_sparse_serialize() {
        let seeded = || ClashConfig {
            socks_port: Some(PortStrategy::new_allow_fallback(1080)),
            ..ClashConfig::default()
        };

        // Absent → keep.
        let keep: ClashConfigPatch =
            serde_yaml_ng::from_str("enable_tun_mode: true\n").expect("patch must deserialize");
        assert_eq!(keep.socks_port, None, "absent decodes to outer None (keep)");
        let mut cfg = seeded();
        cfg.apply(keep);
        assert!(cfg.socks_port.is_some(), "absent must keep socks_port");

        // Explicit null → clear.
        let clear: ClashConfigPatch =
            serde_yaml_ng::from_str("socks_port: null\n").expect("patch must deserialize");
        assert_eq!(
            clear.socks_port,
            Some(None),
            "null decodes to Some(None) (clear)"
        );
        let mut cfg = seeded();
        cfg.apply(clear);
        assert_eq!(cfg.socks_port, None, "explicit null must clear socks_port");

        // Sparse serialize: absent http_port must not appear.
        let mut patch = ClashConfig::new_empty_patch();
        patch.socks_port = Some(None);
        let dumped = serde_yaml_ng::to_string(&patch).expect("serialize patch");
        assert!(
            dumped.contains("socks_port: null"),
            "Some(None) -> null, got:\n{dumped}"
        );
        assert!(
            !dumped.contains("http_port"),
            "absent skipped, got:\n{dumped}"
        );
    }
}

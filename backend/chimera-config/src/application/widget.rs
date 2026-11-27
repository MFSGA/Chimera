use serde::{Deserialize, Serialize};
use specta::Type;

// Part of this module's public API, so consumers need not depend on the widget protocol.
pub use chimera_widget_protocol::StatisticWidgetVariant;

/// Whether the network statistic widget is shown.
#[derive(Debug, Default, Serialize, Deserialize, Type, Copy, Clone, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
#[serde(tag = "kind", content = "value")]
pub enum NetworkStatisticWidgetConfig {
    #[default]
    Disabled,
    Enabled(StatisticWidgetVariant),
}

/// Which logo the statistic widget shows.
#[derive(Debug, Default, Serialize, Deserialize, Clone, Copy, PartialEq, Eq, Type)]
pub struct NetworkStatisticWidgetLogo {
    /// Switch between the normal, TUN and system proxy icons with the proxy
    /// state, as the tray does. Otherwise the normal icon stays.
    #[serde(default)]
    pub follow_state: bool,
    #[serde(default)]
    pub source: NetworkStatisticWidgetLogoSource,
}

/// Where the statistic widget's icons come from.
#[derive(Debug, Default, Serialize, Deserialize, Clone, Copy, PartialEq, Eq, Type)]
#[serde(rename_all = "snake_case")]
pub enum NetworkStatisticWidgetLogoSource {
    /// The tray's icons, including the ones the user set for the tray.
    #[default]
    Tray,
    /// A set of the widget's own, which leaves the tray alone.
    Widget,
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn widget_config_keeps_its_stored_format() {
        for (config, json) in [
            (
                NetworkStatisticWidgetConfig::Disabled,
                r#"{"kind":"disabled"}"#,
            ),
            (
                NetworkStatisticWidgetConfig::Enabled(StatisticWidgetVariant::Large),
                r#"{"kind":"enabled","value":"large"}"#,
            ),
            (
                NetworkStatisticWidgetConfig::Enabled(StatisticWidgetVariant::Small),
                r#"{"kind":"enabled","value":"small"}"#,
            ),
        ] {
            assert_eq!(serde_json::to_string(&config).unwrap(), json);
            assert_eq!(
                serde_json::from_str::<NetworkStatisticWidgetConfig>(json).unwrap(),
                config
            );
        }
    }

    #[test]
    fn widget_logo_is_a_defaulted_ordinary_preference() {
        use crate::application::{
            ApplicationPreferencesPatch, ApplicationSettings, ChimeraAppConfig,
            ChimeraAppConfigPatch, SystemSettingsPatch,
        };
        let mut legacy = serde_json::to_value(ChimeraAppConfig::default()).unwrap();
        legacy
            .as_object_mut()
            .unwrap()
            .remove("network_statistic_widget_logo");
        let config: ChimeraAppConfig = serde_json::from_value(legacy).unwrap();
        assert_eq!(
            config.network_statistic_widget_logo,
            NetworkStatisticWidgetLogo::default()
        );
        let value = serde_json::json!({"network_statistic_widget_logo":{"follow_state":true,"source":"widget"}});
        let preferences: ApplicationPreferencesPatch =
            serde_json::from_value(value.clone()).unwrap();
        let logo = NetworkStatisticWidgetLogo {
            follow_state: true,
            source: NetworkStatisticWidgetLogoSource::Widget,
        };
        assert_eq!(preferences.network_statistic_widget_logo, Some(logo));
        let patch: ChimeraAppConfigPatch = preferences.into();
        assert_eq!(patch.network_statistic_widget_logo, Some(logo));
        assert!(serde_json::from_value::<SystemSettingsPatch>(value).is_err());
        let config = ChimeraAppConfig {
            network_statistic_widget_logo: logo,
            ..config
        };
        assert_eq!(
            ApplicationSettings::from(&config).network_statistic_widget_logo,
            logo
        );
    }

    #[test]
    fn widget_logo_keeps_its_stored_format() {
        let logo = NetworkStatisticWidgetLogo {
            follow_state: true,
            source: NetworkStatisticWidgetLogoSource::Widget,
        };
        let json = r#"{"follow_state":true,"source":"widget"}"#;
        assert_eq!(serde_json::to_string(&logo).unwrap(), json);
        assert_eq!(
            serde_json::from_str::<NetworkStatisticWidgetLogo>(json).unwrap(),
            logo
        );
        assert_eq!(
            serde_json::from_str::<NetworkStatisticWidgetLogo>("{}").unwrap(),
            NetworkStatisticWidgetLogo::default()
        );
    }
}

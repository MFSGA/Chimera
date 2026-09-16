use serde::{Deserialize, Serialize};
use specta::Type;

/// Network statistic widget size. Kept in the config crate so the setting's
/// wire contract does not depend on the optional desktop widget implementation.
#[derive(Debug, Serialize, Deserialize, Type, Copy, Clone, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum StatisticWidgetVariant {
    Large,
    Small,
}

/// Whether the network statistic widget is shown.
#[derive(Debug, Default, Serialize, Deserialize, Type, Copy, Clone, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
#[serde(tag = "kind", content = "value")]
pub enum NetworkStatisticWidgetConfig {
    #[default]
    Disabled,
    Enabled(StatisticWidgetVariant),
}

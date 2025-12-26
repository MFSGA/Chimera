use crate::config::profile::item::remote::RemoteProfileBuilder;

// todo: add the serde
#[derive(Debug, serde::Deserialize, specta::Type)]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum ProfileBuilder {
    Remote(RemoteProfileBuilder),
    // Local(LocalProfileBuilder),
    // Merge(MergeProfileBuilder),
    // Script(ScriptProfileBuilder),
}

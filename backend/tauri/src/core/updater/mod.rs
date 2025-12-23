use serde::{Deserialize, Serialize};
use specta::Type;

// TODO: manifest v2 should be kebad-case
#[derive(Deserialize, Serialize, Clone, Debug, Type)]
pub struct ManifestVersionLatest {
    mihomo: String,
    mihomo_alpha: String,
    clash_rs: String,
    clash_rs_alpha: String,
    clash_premium: String,
}

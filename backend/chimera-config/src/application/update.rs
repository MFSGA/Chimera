use serde::{Deserialize, Serialize};
use specta::Type;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Deserialize, Serialize, Type)]
#[serde(rename_all = "snake_case")]
pub enum UpdateSource {
    Chimera,
    Github,
    Ghfast,
    Sourceforge,
}

pub fn default_update_sources() -> Vec<UpdateSource> {
    vec![UpdateSource::Chimera, UpdateSource::Github]
}

pub fn validate_update_sources(sources: &[UpdateSource]) -> Result<(), &'static str> {
    if sources.is_empty() {
        return Err("at least one update source must be selected");
    }
    for (index, source) in sources.iter().enumerate() {
        if sources[..index].contains(source) {
            return Err("update sources must not contain duplicates");
        }
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn default_sources_preserve_the_external_configuration_values() {
        let sources = default_update_sources();
        assert_eq!(
            serde_json::to_value(&sources).unwrap(),
            serde_json::json!(["chimera", "github"])
        );
        assert!(validate_update_sources(&sources).is_ok());
    }

    #[test]
    fn empty_and_duplicate_sources_are_rejected() {
        assert!(validate_update_sources(&[]).is_err());
        assert!(validate_update_sources(&[UpdateSource::Chimera, UpdateSource::Chimera]).is_err());
        assert!(validate_update_sources(&[UpdateSource::Chimera, UpdateSource::Github]).is_ok());
    }
}

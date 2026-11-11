use serde::{Deserialize, Deserializer, Serialize};
use specta::Type;

/// A runtime config field Chimera may take over from the profiles.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Type)]
#[serde(tag = "kind", content = "value", rename_all = "snake_case")]
pub enum ManageableField<T> {
    /// Written into the runtime config over whatever the profiles and transforms produced.
    Managed(T),
    /// Left to the profiles and transforms, subject to the field filter.
    Unmanaged,
}

impl<T> ManageableField<T> {
    /// The value Chimera writes, if it manages the field.
    pub fn managed(&self) -> Option<&T> {
        match self {
            Self::Managed(value) => Some(value),
            Self::Unmanaged => None,
        }
    }
}

/// Older Chimera versions stored these settings as bare values. Accept that
/// representation as Managed(value) while writing the tagged reference shape.
impl<'de, T: Deserialize<'de>> Deserialize<'de> for ManageableField<T> {
    fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
    where
        D: Deserializer<'de>,
    {
        #[derive(Deserialize)]
        #[serde(untagged)]
        enum Representation<T> {
            Tagged(Tagged<T>),
            Legacy(T),
        }

        #[derive(Deserialize)]
        #[serde(tag = "kind", content = "value", rename_all = "snake_case")]
        enum Tagged<T> {
            Managed(T),
            Unmanaged,
        }

        match Representation::deserialize(deserializer)? {
            Representation::Tagged(Tagged::Managed(value)) | Representation::Legacy(value) => {
                Ok(Self::Managed(value))
            }
            Representation::Tagged(Tagged::Unmanaged) => Ok(Self::Unmanaged),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn serializes_adjacently_tagged() {
        assert_eq!(
            serde_json::to_value(ManageableField::Managed(false)).unwrap(),
            serde_json::json!({ "kind": "managed", "value": false })
        );
        assert_eq!(
            serde_json::to_value(ManageableField::<bool>::Unmanaged).unwrap(),
            serde_json::json!({ "kind": "unmanaged" })
        );
    }

    #[test]
    fn reads_legacy_values_as_managed() {
        assert_eq!(
            serde_json::from_value::<ManageableField<bool>>(serde_json::json!(true)).unwrap(),
            ManageableField::Managed(true)
        );
        assert_eq!(
            serde_json::from_value::<ManageableField<bool>>(serde_json::json!(false)).unwrap(),
            ManageableField::Managed(false)
        );
    }

    #[test]
    fn reads_tagged_values() {
        assert_eq!(
            serde_json::from_value::<ManageableField<bool>>(
                serde_json::json!({ "kind": "managed", "value": false })
            )
            .unwrap(),
            ManageableField::Managed(false)
        );
        assert_eq!(
            serde_json::from_value::<ManageableField<bool>>(
                serde_json::json!({ "kind": "unmanaged" })
            )
            .unwrap(),
            ManageableField::Unmanaged
        );
    }
}

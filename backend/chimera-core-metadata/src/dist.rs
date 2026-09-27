//! Identity of a *distributed* core artifact, as the resource layer knows it.
//!
//! [`ClashCoreKind`] is the behavioral axis alone — the family whose console
//! layout and config semantics the service dispatches on. Which build of that
//! family is installed (release channel, compile variant, anything a future
//! manifest invents) is a separate, open question, and this module mirrors how
//! the resources manifest answers it: a variant's semantic identity is its exact
//! tag set, not a fixed list of columns.

use std::collections::BTreeSet;

use serde::{Deserialize, Serialize};
use specta::Type;

use crate::ClashCoreKind;

/// One variant tag, mirroring the manifest's `ResourceTag.id`: `group:value`
/// with a single colon, or a bare `value` with no group.
///
/// The vocabulary is open by construction. Manifests are fetched at runtime and
/// evolve faster than this crate ships, so an unknown tag must decode, compare,
/// hash, sort and round-trip rather than fail. Normalization (trim, ASCII
/// lowercase) happens on construction *and* on decode, which is what lets
/// equality stay plain string equality.
#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord, Hash, Type, Serialize, Deserialize)]
#[serde(from = "String")]
#[specta(transparent)]
pub struct VariantTag(String);

impl VariantTag {
    /// The one tag group the service itself has a name for. Values inside it
    /// stay open, and compile-variant groups (`goamd64`, …) are deliberately not
    /// named here: that vocabulary belongs to the manifest and is still settling.
    pub const GROUP_CHANNEL: &'static str = "channel";

    pub fn new(tag: impl AsRef<str>) -> Self {
        Self(tag.as_ref().trim().to_ascii_lowercase())
    }

    pub fn as_str(&self) -> &str {
        &self.0
    }

    /// The part before the first `:`, if the tag is grouped.
    pub fn group(&self) -> Option<&str> {
        self.0.split_once(':').map(|(group, _)| group)
    }

    /// The part after the first `:`, or the whole tag when it carries no group.
    pub fn value(&self) -> &str {
        self.0
            .split_once(':')
            .map_or(self.0.as_str(), |(_, value)| value)
    }
}

impl From<String> for VariantTag {
    fn from(tag: String) -> Self {
        Self::new(tag)
    }
}

impl std::fmt::Display for VariantTag {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(&self.0)
    }
}

/// Which distributed artifact is installed: the closed behavioral family plus
/// the manifest's open variant identity.
///
/// A runtime identity handshake, not a mirror of the manifest schema —
/// registries, mirrors, checksums and version plans stay in the manifest layer.
/// It is also deliberately not what a [`crate::LogFrame`] carries: console
/// output can only ever evidence the family.
///
/// The canonical `tag_key` is not carried and not re-derived. That key is the
/// manifest generator's own normalization, and reimplementing it here would give
/// two algorithms one chance each to drift; comparing tag sets needs neither.
#[derive(Debug, Clone, PartialEq, Eq, Type, Serialize, Deserialize)]
pub struct CoreDistribution {
    pub kind: ClashCoreKind,
    /// The manifest variant's stable alias (`ResourceVariant.id`), e.g.
    /// `"alpha-goamd64-v2"`. A display and reference handle only — the semantic
    /// identity is [`Self::tags`], exactly as in the manifest.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[specta(optional)]
    pub variant: Option<String>,
    /// The variant's exact tag set. Sorted and deduplicated by construction, so
    /// equality is order-independent the way the manifest's `tag_key` is. Empty
    /// means untagged or unknown provenance.
    #[serde(default, skip_serializing_if = "BTreeSet::is_empty")]
    #[specta(optional)]
    pub tags: BTreeSet<VariantTag>,
}

impl CoreDistribution {
    pub fn new(kind: ClashCoreKind) -> Self {
        Self {
            kind,
            variant: None,
            tags: BTreeSet::new(),
        }
    }

    /// The first tag value in `group`, in sorted tag order.
    pub fn tag_value(&self, group: &str) -> Option<&str> {
        self.tags
            .iter()
            .find(|tag| tag.group() == Some(group))
            .map(VariantTag::value)
    }

    /// Convenience for the one well-known group.
    pub fn channel(&self) -> Option<&str> {
        self.tag_value(VariantTag::GROUP_CHANNEL)
    }
}

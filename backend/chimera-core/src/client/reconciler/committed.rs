//! Selection policy and connection interruption for the committed runtime.
use chimera_config::{
    clash::config::{ClashConfig, overrides::Mode},
    profile::{ProfileId, Profiles},
};

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Selection {
    mode: Mode,
    current: Option<ProfileId>,
}
impl Selection {
    pub fn of(inputs: &super::inputs::RuntimeInputs) -> Self {
        Self::from_sources(&inputs.clash, &inputs.profiles)
    }
    pub fn from_sources(clash: &ClashConfig, profiles: &Profiles) -> Self {
        Self {
            mode: clash.overrides.mode(),
            current: profiles.current.clone(),
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SelectionChange {
    Mode,
    Profile,
}

impl Selection {
    pub fn interruption(
        &self,
        previous: Option<&Self>,
        clash: &chimera_config::clash::config::ClashConfig,
    ) -> Option<SelectionChange> {
        self.interruption_with_policy(
            previous,
            clash.break_connection.on_mode_change,
            clash.break_connection.on_profile_change,
        )
    }

    pub fn interruption_with_policy(
        &self,
        previous: Option<&Self>,
        on_mode_change: bool,
        on_profile_change: bool,
    ) -> Option<SelectionChange> {
        let previous = previous?;
        if previous.mode != self.mode && on_mode_change {
            Some(SelectionChange::Mode)
        } else if previous.current != self.current && on_profile_change {
            Some(SelectionChange::Profile)
        } else {
            None
        }
    }
}

/// The three source domains whose durable commits the runtime owner observes.
/// Like ref's ConfigDomain, this is source identity, not a Core command.
pub use chimera_contracts::mutation::ConfigDomain as SourceDomain;

/// The reconciler mailbox's monotonic view of source publications. Never use
/// this view as a substitute for reading fresh source snapshots during apply.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct SourceVersions {
    application: Option<u64>,
    clash: Option<u64>,
    profiles: Option<u64>,
}

impl SourceVersions {
    /// Read the committed versions without calling a source actor that may
    /// already be waiting on the CoreLifecycle mailbox. Production supplies
    /// the Profiles snapshot; isolated legacy fixtures can omit that owner.
    pub fn from_committed(
        application: &crate::client::application::ApplicationClient,
        clash: &crate::client::clash_config::ClashConfigClient,
        profiles_version: Option<u64>,
    ) -> Self {
        let mut versions = Self::default();
        versions.observe(
            SourceDomain::Application,
            application.committed_runtime_source().1,
        );
        versions.observe(SourceDomain::Clash, clash.committed_runtime_source().1);
        if let Some(version) = profiles_version {
            versions.observe(SourceDomain::Profiles, version);
        }
        versions
    }

    pub fn observe(&mut self, domain: SourceDomain, version: u64) {
        let seen = match domain {
            SourceDomain::Application => &mut self.application,
            SourceDomain::Clash => &mut self.clash,
            SourceDomain::Profiles => &mut self.profiles,
        };
        if seen.is_none_or(|previous| version > previous) {
            *seen = Some(version);
        }
    }

    pub fn merge(&mut self, other: &Self) {
        for domain in [
            SourceDomain::Application,
            SourceDomain::Clash,
            SourceDomain::Profiles,
        ] {
            if let Some(version) = other.version(domain) {
                self.observe(domain, version);
            }
        }
    }

    pub fn version(&self, domain: SourceDomain) -> Option<u64> {
        match domain {
            SourceDomain::Application => self.application,
            SourceDomain::Clash => self.clash,
            SourceDomain::Profiles => self.profiles,
        }
    }

    /// A compatibility Apply already fences the version of the source that
    /// triggered it. Fence the *other* committed source domains too: a newer
    /// Application, Clash or Profiles target invalidates an in-flight build
    /// even when the initiating source version has not changed.
    pub fn ensure_dependencies_current(
        &self,
        origin: SourceDomain,
        current: &Self,
    ) -> anyhow::Result<()> {
        for domain in [
            SourceDomain::Application,
            SourceDomain::Clash,
            SourceDomain::Profiles,
        ] {
            if domain != origin && self.version(domain) != current.version(domain) {
                return Err(anyhow::Error::new(super::attempt::CommittedTargetAdvanced).context(
                    format!(
                        "{domain:?} source advanced during {origin:?} runtime apply: expected {:?}, observed {:?}",
                        self.version(domain),
                        current.version(domain),
                    ),
                ));
            }
        }
        Ok(())
    }
}

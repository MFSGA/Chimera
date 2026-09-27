//! Source-domain changes admitted to the application runtime workflow.
//!
//! This Profile slice follows ref's `DomainChange` / `MutationDomain` contract.
//! Chimera's application and clash writers remain on their current lifecycle
//! path until their migration slices move onto the shared participant.

use std::sync::Arc;

use chimera_config::profile::Profiles;
use chimera_core::state::StateChange;

#[derive(Debug, Clone)]
pub(crate) enum DomainChange {
    Profiles {
        previous: Option<Arc<Profiles>>,
        candidate: Arc<Profiles>,
    },
}

pub(crate) trait MutationDomain: Clone + Send + Sync + 'static {
    fn domain_change(change: StateChange<Self>) -> DomainChange;
}

impl MutationDomain for Profiles {
    fn domain_change(change: StateChange<Self>) -> DomainChange {
        DomainChange::Profiles {
            previous: change
                .previous
                .map(|previous| Arc::new(previous.state.clone())),
            candidate: change.current,
        }
    }
}

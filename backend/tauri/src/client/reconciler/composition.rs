//! Compose the pinned ref CandidateCheck from the existing source owners and
//! Core-owned runtime session. Construction itself is read-only.
//!
//! Profiles production installs this gate through MutationCoordinator and
//! sends checked commits to the single RuntimeReconciler mailbox. Application and
//! Clash mutations still use their separate legacy runtime paths.

use std::sync::Arc;

use chimera_config::runtime::executor::ProfileContentSource;
use chimera_core::client::{
    reconciler::composition::{CandidateGateSources, production_candidate_gate},
    runtime_build::CoreSpecResolver,
};

use super::{
    check::{CandidateCheck, CheckedCandidatePort},
    ports::ScriptRunnerFactory,
};
use crate::{client::runtime::RuntimePaths, state::mutation::ProfileContentReader};

/// Bridges ref's Checked/SourceCommitted notifications into the *existing*
/// RuntimeReconciler Actor. Checked admissions are acknowledged and committed
/// notices use a dedicated mailbox message; they do not call the legacy
/// SourceCommitted rebuild handler.
pub(in crate::client) struct ReconcilerCheckedInbox {
    owner: crate::client::reconciler::RuntimeReconcilerClient,
}

impl ReconcilerCheckedInbox {
    pub(in crate::client) fn new(
        owner: crate::client::reconciler::RuntimeReconcilerClient,
    ) -> Self {
        Self { owner }
    }

    fn source_domain(
        domain: chimera_contracts::mutation::ConfigDomain,
    ) -> crate::client::reconciler::committed::SourceDomain {
        use crate::client::reconciler::committed::SourceDomain;
        match domain {
            chimera_contracts::mutation::ConfigDomain::Application => SourceDomain::Application,
            chimera_contracts::mutation::ConfigDomain::Clash => SourceDomain::Clash,
            chimera_contracts::mutation::ConfigDomain::Profiles => SourceDomain::Profiles,
        }
    }
}

#[async_trait::async_trait]
impl CheckedCandidatePort for ReconcilerCheckedInbox {
    async fn checked(
        &self,
        domain: chimera_contracts::mutation::ConfigDomain,
        prepared: crate::client::core_lifecycle::ports::PreparedRuntime,
        core_spec: chimera_core_manager::CoreSpec,
    ) -> Result<(), Arc<chimera_contracts::runtime_error::RuntimeError>> {
        let domain = Self::source_domain(domain);
        self.owner
            .checked_candidate_for_domain(domain, prepared, core_spec)
            .await
    }

    fn source_only_committed(
        &self,
        domain: chimera_contracts::mutation::ConfigDomain,
        version: chimera_core::state::Version,
    ) {
        self.owner
            .source_committed(Self::source_domain(domain), *version, false);
    }

    fn source_committed(
        &self,
        domain: chimera_contracts::mutation::ConfigDomain,
        version: chimera_core::state::Version,
    ) {
        self.owner
            .checked_source_committed(Self::source_domain(domain), *version);
    }

    fn source_requires_reconcile(
        &self,
        domain: chimera_contracts::mutation::ConfigDomain,
        version: chimera_core::state::Version,
    ) {
        self.owner
            .source_committed(Self::source_domain(domain), *version, true);
    }

    fn abort_uncommitted(&self, domain: chimera_contracts::mutation::ConfigDomain) {
        self.owner
            .abort_uncommitted_checked(Self::source_domain(domain));
    }

    async fn checked_for_request(
        &self,
        domain: chimera_contracts::mutation::ConfigDomain,
        prepared: crate::client::core_lifecycle::ports::PreparedRuntime,
        core_spec: chimera_core_manager::CoreSpec,
        request: String,
        interrupt_mode: bool,
    ) -> Result<(), Arc<chimera_contracts::runtime_error::RuntimeError>> {
        self.owner
            .checked_candidate_for_domain_with_request_effect(
                Self::source_domain(domain),
                prepared,
                core_spec,
                request,
                interrupt_mode,
            )
            .await
    }

    fn source_committed_for_request(
        &self,
        domain: chimera_contracts::mutation::ConfigDomain,
        version: chimera_core::state::Version,
        request: String,
    ) {
        self.owner.checked_source_committed_for_request(
            Self::source_domain(domain),
            *version,
            request,
        );
    }

    fn abort_uncommitted_for_request(
        &self,
        domain: chimera_contracts::mutation::ConfigDomain,
        request: String,
    ) {
        self.owner
            .abort_uncommitted_checked_request(Self::source_domain(domain), request);
    }
}

/// Assemble the Profiles production gate from the live source owners and the
/// canonical Core host. Only checked-source notifications consume its products;
/// no legacy SourceCommitted rebuild is sent for the same Profiles commit.
/// No fallback session or speculative source snapshot is ever constructed.
pub(in crate::client) fn prepare_production_gate(
    application: &crate::client::application::ApplicationClient,
    clash: &crate::client::clash_config::ClashConfigClient,
    profiles: ProfileContentReader,
    content: Arc<dyn ProfileContentSource + Send + Sync>,
    paths: RuntimePaths,
    core: &chimera_core::actor_v2::CoreClient,
    owner: &crate::client::reconciler::RuntimeReconcilerClient,
    core_specs: Arc<CoreSpecResolver>,
    scripts: Arc<ScriptRunnerFactory>,
) -> anyhow::Result<CandidateCheck> {
    let session = owner.runtime_preparation_session().ok_or_else(|| {
        anyhow::anyhow!("ref CandidateCheck requires the active Core-owned runtime session")
    })?;
    let sources = CandidateGateSources {
        application: application.committed_snapshot_handle()?,
        clash: clash.committed_snapshot_handle()?,
        profile_content: profiles,
        content,
        session,
        paths,
    };
    Ok(production_candidate_gate(
        sources,
        core,
        core_specs,
        scripts,
        Arc::new(ReconcilerCheckedInbox::new(owner.clone())),
    ))
}

pub(crate) fn host_core_spec_resolver() -> Arc<CoreSpecResolver> {
    Arc::new(|core| {
        chimera_core::client::core_spec::resolve_with(core, |choice, core_type| {
            if *choice == chimera_config::application::ClashCore::ChimeraClientAlpha {
                let name = format!("chimera-client-alpha{}", std::env::consts::EXE_SUFFIX);
                crate::core::clash::core::find_binary_path_by_name(&name)
            } else {
                crate::core::clash::core::find_binary_path(core_type)
            }
        })
    })
}

pub(crate) fn host_script_runner_factory() -> Arc<ScriptRunnerFactory> {
    Arc::new(|| Ok(Box::new(crate::enhance::EnhanceScriptRunner::new()?)))
}

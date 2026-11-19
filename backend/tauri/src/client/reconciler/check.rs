//! Read-only runtime candidate gate shared by typed Clash, Application and
//! Profiles checks while the source actors are migrated to ref RuntimeGate.
//!
//! This module never applies a runtime, publishes a source document, or takes
//! CoreLifecycle's mutating command lease. Source owners must still commit
//! their prepared documents before asking the lifecycle to reconcile.

use anyhow::{Result, bail};

use crate::client::{
    application_workflow::{
        inputs::RuntimeInputs,
        mutation::{CheckRecord, RuntimeCheckFailure},
    },
    core_lifecycle::ports::CoreLifecyclePort,
};

/// Freeze the full candidate input *before* invoking the independent Core
/// validator. An unavailable backend and an invalid candidate are both
/// failures, never implicit approval.
pub(crate) async fn check_candidate(
    core: &dyn CoreLifecyclePort,
    inputs: RuntimeInputs,
    service_connected: bool,
) -> Result<CheckRecord> {
    let (app, clash, profiles, content) = inputs.into_parts().map_err(|error| {
        anyhow::Error::new(RuntimeCheckFailure::candidate_unavailable(
            error.to_string(),
        ))
    })?;
    let run_type = crate::core::RunType::classify(
        app.enable_service_mode,
        if service_connected {
            crate::core::service::ipc::IpcState::Connected
        } else {
            crate::core::service::ipc::IpcState::Disconnected
        },
    );
    core.validate_profile_runtime(clash, app.core, run_type, profiles, app, content)
        .await
}

/// A source may commit a runtime-affecting candidate only after a real pass.
/// In particular, NotOwed/Skipped are not interchangeable with Passed here.
pub(crate) fn require_passed(record: CheckRecord, domain: &str) -> Result<()> {
    match record {
        CheckRecord::Passed => Ok(()),
        other => bail!("independent {domain} runtime candidate check did not pass: {other:?}"),
    }
}

/// Ref CandidateCheck boundary adapted to Chimera's typed source clients.
pub(crate) struct CandidateCheck {
    core: std::sync::Arc<dyn CoreLifecyclePort>,
    application: super::super::application::ApplicationClient,
    clash: super::super::clash_config::ClashConfigClient,
    profile_content: crate::state::mutation::ProfileContentReader,
}

impl CandidateCheck {
    pub(crate) fn new(
        core: std::sync::Arc<dyn CoreLifecyclePort>,
        application: super::super::application::ApplicationClient,
        clash: super::super::clash_config::ClashConfigClient,
        profile_content: crate::state::mutation::ProfileContentReader,
    ) -> Self {
        Self {
            core,
            application,
            clash,
            profile_content,
        }
    }
}

#[async_trait::async_trait]
impl crate::state::mutation::RuntimeGate for CandidateCheck {
    async fn check(&self, candidate: crate::state::mutation::RuntimeCandidate) -> Result<()> {
        let profiles_dir = crate::utils::dirs::app_profiles_dir()?;
        let source = crate::enhance::FsProfileContentSource::new(profiles_dir);
        let content = self
            .profile_content
            .candidate(async {
                chimera_core::client::runtime_build_port::FrozenProfileContent::capture_content(
                    &candidate.profiles,
                    &source,
                    candidate.staged,
                )
            })
            .await;
        let inputs = RuntimeInputs {
            app: self.application.get_typed(),
            clash: self.clash.get()?,
            profiles: candidate.profiles,
            content,
        };
        let checked = check_candidate(
            self.core.as_ref(),
            inputs,
            crate::core::service::ipc::get_ipc_state().is_connected(),
        )
        .await?;
        require_passed(checked, "profiles")
    }
}

#[cfg(test)]
mod tests {
    use std::{
        collections::BTreeMap,
        sync::{
            Arc, Mutex,
            atomic::{AtomicUsize, Ordering},
        },
    };

    use chimera_config::{
        application::{ChimeraAppConfig, ClashCore},
        clash::config::ClashConfig,
        profile::Profiles,
    };

    use super::*;
    use crate::{client::reconciler::inputs::FrozenProfileContent, core::RunType};

    #[derive(Default)]
    struct RecordingChecker {
        runs: Mutex<Vec<RunType>>,
        calls: AtomicUsize,
        reject: bool,
    }

    #[async_trait::async_trait]
    impl CoreLifecyclePort for RecordingChecker {
        async fn reconcile(&self, _: ClashConfig, _: ClashCore, _: RunType) -> Result<()> {
            panic!("candidate checking must not reconcile");
        }

        async fn stop(&self) -> Result<()> {
            panic!("candidate checking must not stop the core");
        }

        async fn change_core(&self, _: ClashCore) -> Result<()> {
            panic!("candidate checking must not switch binaries");
        }

        async fn status(&self) -> Result<crate::client::core_lifecycle::ports::CoreStatusSnapshot> {
            panic!("candidate checking must not probe the mutable lifecycle");
        }

        fn recovery_notify(&self) -> Option<Arc<tokio::sync::Notify>> {
            None
        }

        async fn on_profile_change(&self, _: bool) {
            panic!("candidate checking must not interrupt connections");
        }

        async fn validate_profile_runtime(
            &self,
            _: ClashConfig,
            _: ClashCore,
            run_type: RunType,
            _: Arc<Profiles>,
            app: ChimeraAppConfig,
            _: BTreeMap<String, String>,
        ) -> Result<CheckRecord> {
            assert!(app.enable_service_mode);
            self.calls.fetch_add(1, Ordering::SeqCst);
            self.runs.lock().unwrap().push(run_type);
            if self.reject {
                Ok(CheckRecord::Rejected("invalid candidate".into()))
            } else {
                Ok(CheckRecord::Passed)
            }
        }
    }

    fn inputs() -> RuntimeInputs {
        RuntimeInputs {
            app: ChimeraAppConfig {
                enable_service_mode: true,
                ..Default::default()
            },
            clash: ClashConfig::default(),
            profiles: Arc::new(Profiles::default()),
            content: FrozenProfileContent::default(),
        }
    }

    #[tokio::test]
    async fn independent_gate_uses_requested_host_without_mutating_core() {
        let checker = RecordingChecker::default();
        let checked = check_candidate(&checker, inputs(), true).await.unwrap();
        require_passed(checked, "application").unwrap();
        assert_eq!(checker.calls.load(Ordering::SeqCst), 1);
        assert_eq!(*checker.runs.lock().unwrap(), [RunType::Service]);

        let checked = check_candidate(&checker, inputs(), false).await.unwrap();
        require_passed(checked, "clash").unwrap();
        assert_eq!(
            *checker.runs.lock().unwrap(),
            [RunType::Service, RunType::Normal]
        );
    }

    #[tokio::test]
    async fn rejected_core_and_unreadable_frozen_content_fail_closed() {
        let checker = RecordingChecker {
            reject: true,
            ..Default::default()
        };
        let result = check_candidate(&checker, inputs(), true).await.unwrap();
        assert!(
            require_passed(result, "clash")
                .unwrap_err()
                .to_string()
                .contains("invalid candidate")
        );

        let mut unreadable = inputs();
        unreadable
            .content
            .0
            .insert("active.yaml".into(), Err("content read failed".into()));
        let error = check_candidate(&checker, unreadable, true)
            .await
            .unwrap_err();
        assert!(error.to_string().contains("content read failed"));
        assert_eq!(
            checker.calls.load(Ordering::SeqCst),
            1,
            "the lower checker must never receive incomplete content"
        );
    }

    #[test]
    fn a_source_requires_a_positive_runtime_check() {
        assert!(require_passed(CheckRecord::Passed, "clash").is_ok());
        for outcome in [
            CheckRecord::NotOwed,
            CheckRecord::Skipped("not checked".into()),
            CheckRecord::Unserviceable("backend unavailable".into()),
            CheckRecord::Rejected("core rejected candidate".into()),
        ] {
            let error = require_passed(outcome, "application").unwrap_err();
            assert!(error.to_string().contains("did not pass"));
        }
    }
}

//! Desktop hotkey transaction and presentation adapters.
pub mod adapters;
pub(crate) mod main_thread;
pub mod ports;
#[cfg(test)]
mod tests;

use self::ports::{HotkeyAction, HotkeyBindings, HotkeyParseError};
use super::{
    ChimeraClient,
    effects::{
        plan::{ApplicationEffectInputs, EffectKind},
        status::{EffectHealth, EffectRevision, EffectStatus},
    },
};
use anyhow::Result;
pub use chimera_core::client::hotkey::{HotkeyArgs, HotkeyClient};

/// Keep the existing translated desktop message at the transport boundary.
pub(crate) fn localized_parse_error(error: &HotkeyParseError) -> String {
    use rust_i18n::t;
    match error {
        HotkeyParseError::MalformedEntry { entry } => {
            t!("hotkey_error.malformed_entry", entry = entry).into_owned()
        }
        HotkeyParseError::UnknownFunction { function } => {
            t!("hotkey_error.unknown_function", function = function).into_owned()
        }
        HotkeyParseError::EmptyKeySegment { accelerator }
        | HotkeyParseError::UnsupportedAccelerator { accelerator, .. } => {
            t!("hotkey_error.invalid_hotkey", hotkey = accelerator).into_owned()
        }
        HotkeyParseError::MissingSuperKey { .. } => {
            t!("hotkey_error.missing_super_key").into_owned()
        }
        HotkeyParseError::DuplicateAccelerator { accelerator, .. } => {
            t!("hotkey_error.duplicate_accelerator", hotkey = accelerator).into_owned()
        }
    }
}

pub(crate) fn validate_bindings(
    raw: &[String],
    accelerators: &dyn ports::AcceleratorValidator,
) -> Result<HotkeyBindings> {
    HotkeyBindings::parse(raw, accelerators)
        .map_err(|error| anyhow::anyhow!(localized_parse_error(&error)))
}

impl ChimeraClient {
    pub(crate) async fn set_hotkeys(
        &self,
        hotkeys: Vec<String>,
    ) -> Result<super::MutationOutcome<()>> {
        let _guard = self.inner.hotkey_mutation.lock().await;
        validate_bindings(&hotkeys, self.inner.accelerators.as_ref())?;
        let snapshot = self
            .inner
            .shared
            .application()
            .patch_typed(chimera_config::application::ChimeraAppConfigPatch {
                hotkeys: Some(hotkeys),
                ..Default::default()
            })
            .await?;
        self.inner.reconciler().source_committed(
            crate::client::reconciler::committed::SourceDomain::Application,
            snapshot.version,
            false,
        );
        let mut degradations = Vec::new();
        if let Err(error) = crate::config::core::Config::verge().data().save_file() {
            degradations.push(super::Degradation {
                phase: super::DegradationPhase::LegacyMirror,
                code: "legacy_hotkey_mirror_save_failed".into(),
                message: error.to_string(),
                retryable: false,
            });
        }
        crate::core::handle::Handle::refresh_verge();

        let status = self.reconcile_hotkeys().await;
        degradations.extend(super::effects::status::degradation_of(&status));
        Ok(super::MutationOutcome::from_parts((), degradations))
    }

    /// Runs a full hotkey reconcile through the shared application effect actor.
    pub(crate) async fn reconcile_hotkeys(&self) -> EffectStatus {
        let inputs = self.get_app_config().and_then(|application| {
            self.get_clash_config()
                .map(|clash| ApplicationEffectInputs::project(&application, &clash, None))
        });
        let status = match inputs {
            Ok(inputs) => self.inner.effects.reconcile_hotkeys(inputs).await,
            Err(error) => EffectStatus {
                kind: EffectKind::Hotkeys,
                desired_revision: EffectRevision::default(),
                applied_revision: EffectRevision::default(),
                health: EffectHealth::Degraded {
                    code: "hotkey_config_read_failed",
                    message: format!("{error:#}"),
                    retryable: false,
                },
            },
        };
        if let EffectHealth::Degraded { code, message, .. } = &status.health {
            tracing::warn!(%code, %message, "saved global shortcuts did not fully register");
        }
        status
    }

    /// Compose the reference dispatcher with the desktop transaction and window ports.
    pub(crate) async fn dispatch_hotkey_action(&self, action: HotkeyAction) -> Result<()> {
        self.inner.shared.dispatch_hotkey_action(action).await
    }
}

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
use chimera_config::clash::config::overrides::Mode;
use chimera_config::{
    application::ChimeraAppConfigPatch,
    clash::config::{
        ClashConfig, ClashConfigPatch, TransparentProxyConfigPatch, TransparentProxyMode,
        clash_strategy::port::{PortStrategy, PortStrategyKind},
    },
};
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

    /// Applies a pressed shortcut through the same Chimera mutation paths used
    /// by the settings UI. The OS callback never invokes this method directly.
    pub(crate) async fn dispatch_hotkey_action(&self, action: HotkeyAction) -> Result<()> {
        match action {
            HotkeyAction::OpenOrCloseDashboard => {
                self.inner.window.toggle_dashboard().await?;
                Ok(())
            }
            HotkeyAction::ClashModeRule => self.set_clash_mode(Mode::Rule).await,
            HotkeyAction::ClashModeGlobal => self.set_clash_mode(Mode::Global).await,
            HotkeyAction::ClashModeDirect => self.set_clash_mode(Mode::Direct).await,
            HotkeyAction::ClashModeScript => self.set_clash_mode(Mode::Script).await,
            HotkeyAction::ToggleSystemProxy => {
                let enabled = self.get_app_config()?.enable_system_proxy;
                self.set_system_proxy(!enabled).await
            }
            HotkeyAction::EnableSystemProxy => self.set_system_proxy(true).await,
            HotkeyAction::DisableSystemProxy => self.set_system_proxy(false).await,
            HotkeyAction::ToggleTunMode => {
                let enabled = self.get_clash_config()?.enable_tun_mode;
                self.set_tun_mode(!enabled).await
            }
            HotkeyAction::EnableTunMode => self.set_tun_mode(true).await,
            HotkeyAction::DisableTunMode => self.set_tun_mode(false).await,
            HotkeyAction::ToggleTransparentProxy => {
                let enabled = self.get_clash_config()?.transparent_proxy.mode
                    != TransparentProxyMode::Disabled;
                self.set_transparent_proxy(!enabled).await
            }
            HotkeyAction::EnableTransparentProxy => self.set_transparent_proxy(true).await,
            HotkeyAction::DisableTransparentProxy => self.set_transparent_proxy(false).await,
        }
    }

    async fn set_clash_mode(&self, mode: Mode) -> Result<()> {
        self.patch_clash_overrides(crate::config::runtime::ClashConfigOverrides {
            mode: Some(mode.to_string()),
            ..Default::default()
        })
        .await
    }

    async fn set_system_proxy(&self, enabled: bool) -> Result<()> {
        self.patch_app_config(ChimeraAppConfigPatch {
            enable_system_proxy: Some(enabled),
            ..Default::default()
        })
        .await?;
        Ok(())
    }

    async fn set_transparent_proxy(&self, enabled: bool) -> Result<()> {
        anyhow::ensure!(
            cfg!(target_os = "linux"),
            "transparent capture is supported only on Linux"
        );
        let config = self.get_clash_config()?;
        let patch = switch_patch(&config, enabled);
        // Check the candidate before querying the privileged execution host.
        self.inner.shared.validate_clash_candidate(&patch)?;
        if enabled && config.transparent_proxy.mode == TransparentProxyMode::Disabled {
            let status = self.get_transparent_proxy_status().await?;
            anyhow::ensure!(
                status.supported,
                "the service does not support transparent capture"
            );
            anyhow::ensure!(
                status.error.is_none(),
                "transparent capture status is unhealthy"
            );
        }
        let outcome = self.patch_clash_config(patch).await?;
        for degradation in outcome.degradations() {
            tracing::warn!(code = %degradation.code, message = %degradation.message, "a hotkey action committed with a degraded side effect");
        }
        Ok(())
    }

    async fn set_tun_mode(&self, enabled: bool) -> Result<()> {
        self.patch_clash_config(ClashConfigPatch {
            enable_tun_mode: Some(enabled),
            ..Default::default()
        })
        .await?;
        Ok(())
    }
}

/// The patch that switches transparent capture on, keeping a mode the user
/// already chose, or off.
fn switch_patch(config: &ClashConfig, enabled: bool) -> ClashConfigPatch {
    let mode = if enabled {
        match config.transparent_proxy.mode {
            TransparentProxyMode::Disabled => TransparentProxyMode::Tproxy,
            mode => mode,
        }
    } else {
        TransparentProxyMode::Disabled
    };
    ClashConfigPatch {
        transparent_proxy: TransparentProxyConfigPatch {
            mode: Some(mode),
            ..Default::default()
        },
        tproxy_port: (mode == TransparentProxyMode::Tproxy && config.tproxy_port.is_none())
            .then_some(Some(PortStrategy {
                kind: PortStrategyKind::Fixed,
                start_port: 7894,
            })),
        ..Default::default()
    }
}

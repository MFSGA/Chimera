//! The single implementation of [`ApplicationEffectsPort`]: it fans one plan
//! out to the owner of each effect.
//!
//! The executor owns ordering and result mapping. Platform operations remain
//! behind the adapter boundary so tests can verify dispatch without changing
//! host settings.

use std::sync::Arc;

use super::{
    adapters::ApplicationEffectAdapters,
    plan::{ApplicationEffect, ApplicationEffectPlan, EffectKind, LoggerDesired, TrayRefresh},
    ports::ApplicationEffectsPort,
    status::{EffectHealth, EffectRevision, EffectStatus},
};
use crate::client::hotkey::{
    HotkeyClient,
    ports::{AcceleratorValidator, HotkeyBindings},
};

pub(crate) struct ApplicationEffectExecutor {
    hotkeys: HotkeyClient,
    accelerators: Arc<dyn AcceleratorValidator>,
    adapters: Arc<dyn ApplicationEffectAdapters>,
}

impl ApplicationEffectExecutor {
    pub(crate) fn new(
        hotkeys: HotkeyClient,
        accelerators: Arc<dyn AcceleratorValidator>,
        adapters: Arc<dyn ApplicationEffectAdapters>,
    ) -> Self {
        Self {
            hotkeys,
            accelerators,
            adapters,
        }
    }

    async fn apply_hotkeys(&self, revision: EffectRevision, raw: &[String]) -> EffectStatus {
        match HotkeyBindings::parse(raw, self.accelerators.as_ref()) {
            Ok(desired) => self.hotkeys.reconcile(revision, desired).await,
            Err(error) => degraded(
                EffectKind::Hotkeys,
                revision,
                "hotkey_invalid_bindings",
                error.to_string(),
                false,
            ),
        }
    }

    fn apply_logger(&self, revision: EffectRevision, desired: &LoggerDesired) -> EffectStatus {
        self.status_from_result(
            revision,
            EffectKind::Logger,
            self.adapters.refresh_logger(desired),
        )
    }

    async fn apply_tray(&self, revision: EffectRevision, refresh: TrayRefresh) -> EffectStatus {
        self.status_from_result(
            revision,
            EffectKind::Tray,
            self.adapters.refresh_tray(refresh).await,
        )
    }

    fn status_from_result(
        &self,
        revision: EffectRevision,
        kind: EffectKind,
        result: Result<(), super::adapters::EffectAdapterError>,
    ) -> EffectStatus {
        match result {
            Ok(()) => healthy(kind, revision),
            Err(super::adapters::EffectAdapterError::Failed {
                code,
                message,
                retryable,
            }) => degraded(kind, revision, code, message, retryable),
            Err(super::adapters::EffectAdapterError::Unsupported { code }) => EffectStatus {
                kind,
                desired_revision: revision,
                applied_revision: EffectRevision::default(),
                health: EffectHealth::Unsupported { code },
            },
        }
    }
}

#[async_trait::async_trait]
impl ApplicationEffectsPort for ApplicationEffectExecutor {
    async fn apply(
        &self,
        revision: EffectRevision,
        plan: ApplicationEffectPlan,
    ) -> Vec<EffectStatus> {
        let mut statuses = Vec::with_capacity(plan.effects().len());
        for effect in plan.effects() {
            let status = match effect {
                ApplicationEffect::Locale(language) => self.status_from_result(
                    revision,
                    EffectKind::Locale,
                    self.adapters.set_locale(*language),
                ),
                ApplicationEffect::Logger(desired) => self.apply_logger(revision, desired),
                ApplicationEffect::AutoLaunch(enabled) => self.status_from_result(
                    revision,
                    EffectKind::AutoLaunch,
                    self.adapters.set_auto_launch(*enabled).await,
                ),
                ApplicationEffect::SystemProxy(desired) => {
                    // The first startup plan can arrive before the session has
                    // resolved the mixed port. Keep it pending for the actor's
                    // retry instead of installing a stale endpoint.
                    if desired.enabled && desired.port.is_none() {
                        degraded(
                            EffectKind::SystemProxy,
                            revision,
                            "system_proxy_port_unresolved",
                            "the session has not resolved a mixed port yet".into(),
                            true,
                        )
                    } else if desired.enabled && desired.pac_url.is_some() {
                        self.status_from_result(
                            revision,
                            EffectKind::SystemProxy,
                            Err(super::adapters::EffectAdapterError::Unsupported {
                                code: "pac_proxy_unsupported",
                            }),
                        )
                    } else {
                        self.status_from_result(
                            revision,
                            EffectKind::SystemProxy,
                            self.adapters.set_system_proxy(desired).await,
                        )
                    }
                }
                ApplicationEffect::ProxyGuard(desired) => self.status_from_result(
                    revision,
                    EffectKind::ProxyGuard,
                    self.adapters.set_proxy_guard(*desired),
                ),
                ApplicationEffect::Hotkeys(raw) => self.apply_hotkeys(revision, raw).await,
                ApplicationEffect::Widget(config) => self.status_from_result(
                    revision,
                    EffectKind::Widget,
                    self.adapters.set_widget(*config),
                ),
                ApplicationEffect::Tray(refresh) => self.apply_tray(revision, *refresh).await,
            };
            statuses.push(status);
        }
        statuses
    }

    async fn shutdown(&self) -> Vec<EffectStatus> {
        let revision = EffectRevision::default();
        let proxy = self.status_from_result(
            revision,
            EffectKind::SystemProxy,
            self.adapters.restore_system_proxy().await,
        );
        let hotkeys = self.hotkeys.unregister_all().await;
        vec![proxy, hotkeys]
    }
}

fn healthy(kind: EffectKind, revision: EffectRevision) -> EffectStatus {
    EffectStatus {
        kind,
        desired_revision: revision,
        applied_revision: revision,
        health: EffectHealth::Healthy,
    }
}

fn degraded(
    kind: EffectKind,
    revision: EffectRevision,
    code: &'static str,
    message: String,
    retryable: bool,
) -> EffectStatus {
    EffectStatus {
        kind,
        desired_revision: revision,
        applied_revision: EffectRevision::default(),
        health: EffectHealth::Degraded {
            code,
            message,
            retryable,
        },
    }
}

#[cfg(test)]
mod tests {
    use std::sync::Mutex;

    use chimera_config::{
        application::{
            ChimeraAppConfig, LoggingLevel, NetworkStatisticWidgetConfig, StatisticWidgetVariant,
        },
        clash::config::ClashConfig,
        runtime::executor::ResolvedPortBindings,
    };

    use super::*;
    use crate::client::{
        effects::{adapters::EffectAdapterError, plan::ApplicationEffectInputs},
        hotkey::{
            HotkeyArgs,
            adapters::PlatformAcceleratorValidator,
            ports::{HotkeyAction, HotkeyActionSink, ShortcutRegistrar},
        },
    };

    #[derive(Default)]
    struct RecordingAdapters {
        effects: Mutex<Vec<EffectKind>>,
        widget_unsupported: bool,
    }

    #[async_trait::async_trait]
    impl ApplicationEffectAdapters for RecordingAdapters {
        fn set_locale(
            &self,
            _language: chimera_config::application::I18nLanguage,
        ) -> Result<(), EffectAdapterError> {
            self.effects.lock().unwrap().push(EffectKind::Locale);
            Ok(())
        }

        fn refresh_logger(&self, _desired: &LoggerDesired) -> Result<(), EffectAdapterError> {
            self.effects.lock().unwrap().push(EffectKind::Logger);
            Ok(())
        }

        async fn set_auto_launch(&self, _enabled: bool) -> Result<(), EffectAdapterError> {
            self.effects.lock().unwrap().push(EffectKind::AutoLaunch);
            Ok(())
        }

        async fn set_system_proxy(
            &self,
            _desired: &super::super::plan::SystemProxyDesired,
        ) -> Result<(), EffectAdapterError> {
            self.effects.lock().unwrap().push(EffectKind::SystemProxy);
            Ok(())
        }

        fn set_proxy_guard(
            &self,
            _desired: super::super::plan::ProxyGuardDesired,
        ) -> Result<(), EffectAdapterError> {
            self.effects.lock().unwrap().push(EffectKind::ProxyGuard);
            Ok(())
        }

        fn set_widget(
            &self,
            _config: chimera_config::application::NetworkStatisticWidgetConfig,
        ) -> Result<(), EffectAdapterError> {
            self.effects.lock().unwrap().push(EffectKind::Widget);
            if self.widget_unsupported
                && matches!(_config, NetworkStatisticWidgetConfig::Enabled(_))
            {
                return Err(EffectAdapterError::Unsupported {
                    code: "widget_runtime_unavailable",
                });
            }
            Ok(())
        }

        async fn refresh_tray(&self, _refresh: TrayRefresh) -> Result<(), EffectAdapterError> {
            self.effects.lock().unwrap().push(EffectKind::Tray);
            Ok(())
        }

        async fn restore_system_proxy(&self) -> Result<(), EffectAdapterError> {
            Ok(())
        }
    }

    struct NoopRegistrar;

    impl ShortcutRegistrar for NoopRegistrar {
        fn validate(
            &self,
            _accelerator: &str,
        ) -> Result<(), super::super::super::hotkey::ports::HotkeyParseError> {
            Ok(())
        }

        fn register(
            &self,
            _accelerator: &str,
            _action: HotkeyAction,
            _sink: Arc<dyn HotkeyActionSink>,
        ) -> anyhow::Result<()> {
            Ok(())
        }

        fn unregister(&self, _accelerator: &str) -> anyhow::Result<()> {
            Ok(())
        }

        fn unregister_all(&self) -> anyhow::Result<()> {
            Ok(())
        }
    }

    struct NoopSink;
    impl HotkeyActionSink for NoopSink {
        fn dispatch(&self, _action: HotkeyAction) {}
    }

    #[tokio::test]
    async fn full_plan_dispatches_each_effect_in_reference_order() {
        let hotkeys = HotkeyClient::spawn(HotkeyArgs {
            registrar: Arc::new(NoopRegistrar),
            sink: Arc::new(NoopSink),
        })
        .await
        .unwrap();
        let adapters = Arc::new(RecordingAdapters::default());
        let executor = ApplicationEffectExecutor::new(
            hotkeys,
            Arc::new(PlatformAcceleratorValidator),
            adapters.clone(),
        );
        let mut app = ChimeraAppConfig::default();
        app.app_log_level = LoggingLevel::Debug;
        let inputs = ApplicationEffectInputs::project(&app, &ClashConfig::default(), None);
        let plan = ApplicationEffectPlan::full(&inputs);

        let statuses =
            ApplicationEffectsPort::apply(&executor, EffectRevision::new(7), plan.clone()).await;

        assert_eq!(statuses.len(), plan.effects().len());
        assert!(statuses.iter().all(|status| {
            status.desired_revision == EffectRevision::new(7)
                && status.health == EffectHealth::Healthy
        }));
        assert_eq!(
            adapters.effects.lock().unwrap().as_slice(),
            [
                EffectKind::Locale,
                EffectKind::Logger,
                EffectKind::AutoLaunch,
                EffectKind::SystemProxy,
                EffectKind::ProxyGuard,
                EffectKind::Widget,
                EffectKind::Tray,
            ]
        );
        assert_eq!(statuses[5].kind, EffectKind::Hotkeys);
    }

    #[tokio::test]
    async fn missing_proxy_port_is_reported_as_retryable_without_calling_adapter() {
        let hotkeys = HotkeyClient::spawn(HotkeyArgs {
            registrar: Arc::new(NoopRegistrar),
            sink: Arc::new(NoopSink),
        })
        .await
        .unwrap();
        let adapters = Arc::new(RecordingAdapters::default());
        let executor = ApplicationEffectExecutor::new(
            hotkeys,
            Arc::new(PlatformAcceleratorValidator),
            adapters.clone(),
        );
        let mut app = ChimeraAppConfig::default();
        app.enable_system_proxy = true;
        let inputs = ApplicationEffectInputs::project(&app, &ClashConfig::default(), None);
        let proxy = ApplicationEffectPlan::full(&inputs)
            .effects()
            .iter()
            .find(|effect| effect.kind() == EffectKind::SystemProxy)
            .cloned()
            .unwrap();

        let statuses = ApplicationEffectsPort::apply(
            &executor,
            EffectRevision::new(8),
            ApplicationEffectPlan::from_effects(vec![proxy]),
        )
        .await;

        assert!(matches!(
            statuses[0].health,
            EffectHealth::Degraded {
                code: "system_proxy_port_unresolved",
                retryable: true,
                ..
            }
        ));
        assert!(adapters.effects.lock().unwrap().is_empty());
    }

    #[tokio::test]
    async fn enabled_widget_is_reported_unsupported_by_the_current_runtime() {
        let hotkeys = HotkeyClient::spawn(HotkeyArgs {
            registrar: Arc::new(NoopRegistrar),
            sink: Arc::new(NoopSink),
        })
        .await
        .unwrap();
        let adapters = Arc::new(RecordingAdapters {
            widget_unsupported: true,
            ..Default::default()
        });
        let executor = ApplicationEffectExecutor::new(
            hotkeys,
            Arc::new(PlatformAcceleratorValidator),
            adapters,
        );
        let mut app = ChimeraAppConfig::default();
        app.network_statistic_widget =
            NetworkStatisticWidgetConfig::Enabled(StatisticWidgetVariant::Small);
        let inputs = ApplicationEffectInputs::project(&app, &ClashConfig::default(), None);
        let widget = ApplicationEffectPlan::full(&inputs)
            .effects()
            .iter()
            .find(|effect| effect.kind() == EffectKind::Widget)
            .cloned()
            .unwrap();

        let statuses = ApplicationEffectsPort::apply(
            &executor,
            EffectRevision::new(9),
            ApplicationEffectPlan::from_effects(vec![widget]),
        )
        .await;

        assert_eq!(
            statuses[0].health,
            EffectHealth::Unsupported {
                code: "widget_runtime_unavailable"
            }
        );
    }

    #[tokio::test]
    async fn pac_does_not_block_disabling_the_plain_system_proxy() {
        let hotkeys = HotkeyClient::spawn(HotkeyArgs {
            registrar: Arc::new(NoopRegistrar),
            sink: Arc::new(NoopSink),
        })
        .await
        .unwrap();
        let adapters = Arc::new(RecordingAdapters::default());
        let executor = ApplicationEffectExecutor::new(
            hotkeys,
            Arc::new(PlatformAcceleratorValidator),
            adapters.clone(),
        );
        let mut app = ChimeraAppConfig::default();
        app.pac_url = Some("http://pac.test/proxy.pac".parse().unwrap());
        let ports = ResolvedPortBindings {
            mixed_port: 7890,
            port: None,
            socks_port: None,
            external_controller: None,
        };
        let inputs = ApplicationEffectInputs::project(&app, &ClashConfig::default(), Some(ports));
        let proxy = ApplicationEffectPlan::full(&inputs)
            .effects()
            .iter()
            .find(|effect| effect.kind() == EffectKind::SystemProxy)
            .cloned()
            .unwrap();

        let statuses = ApplicationEffectsPort::apply(
            &executor,
            EffectRevision::new(10),
            ApplicationEffectPlan::from_effects(vec![proxy]),
        )
        .await;

        assert_eq!(statuses[0].health, EffectHealth::Healthy);
        assert_eq!(
            adapters.effects.lock().unwrap().as_slice(),
            [EffectKind::SystemProxy]
        );
    }
}

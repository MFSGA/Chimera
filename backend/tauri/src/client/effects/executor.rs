//! The single implementation of [`ApplicationEffectsPort`]: it fans one plan
//! out to the owner of each effect.
//!
//! The executor owns ordering and result mapping. Platform operations remain
//! behind the adapter boundary so tests can verify dispatch without changing
//! host settings.

use std::sync::Arc;

use super::{
    adapters::ApplicationEffectAdapters,
    plan::{
        ApplicationEffect, ApplicationEffectPlan, EffectKind, LoggerDesired, ProxyGuardDesired,
        SystemProxyDesired, TrayRefresh,
    },
    ports::ApplicationEffectsPort,
    status::{EffectHealth, EffectRevision, EffectStatus},
};
use crate::client::hotkey::{
    HotkeyClient,
    ports::{AcceleratorValidator, HotkeyBindings},
};
use crate::client::system_proxy::SystemProxyClient;

pub(crate) struct ApplicationEffectExecutor {
    system_proxy: SystemProxyClient,
    hotkeys: HotkeyClient,
    accelerators: Arc<dyn AcceleratorValidator>,
    adapters: Arc<dyn ApplicationEffectAdapters>,
}

impl ApplicationEffectExecutor {
    pub(crate) fn new(
        system_proxy: SystemProxyClient,
        hotkeys: HotkeyClient,
        accelerators: Arc<dyn AcceleratorValidator>,
        adapters: Arc<dyn ApplicationEffectAdapters>,
    ) -> Self {
        Self {
            system_proxy,
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

    fn system_proxy_desires(
        plan: &ApplicationEffectPlan,
    ) -> (
        Option<SystemProxyDesired>,
        Option<ProxyGuardDesired>,
        Option<bool>,
    ) {
        let mut proxy = None;
        let mut guard = None;
        let mut auto_launch = None;
        for effect in plan.effects() {
            match effect {
                ApplicationEffect::SystemProxy(desired) => proxy = Some(desired.clone()),
                ApplicationEffect::ProxyGuard(desired) => guard = Some(*desired),
                ApplicationEffect::AutoLaunch(enabled) => auto_launch = Some(*enabled),
                _ => {}
            }
        }
        (proxy, guard, auto_launch)
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
        let (proxy, guard, auto_launch) = Self::system_proxy_desires(&plan);
        let mut system_proxy_statuses: Option<Vec<EffectStatus>> = None;
        let mut statuses = Vec::with_capacity(plan.effects().len());
        for effect in plan.effects() {
            let status = match effect {
                ApplicationEffect::Locale(language) => self.status_from_result(
                    revision,
                    EffectKind::Locale,
                    self.adapters.set_locale(*language),
                ),
                ApplicationEffect::Logger(desired) => self.apply_logger(revision, desired),
                ApplicationEffect::AutoLaunch(_)
                | ApplicationEffect::SystemProxy(_)
                | ApplicationEffect::ProxyGuard(_) => {
                    let owned = match system_proxy_statuses {
                        Some(ref owned) => owned,
                        None => system_proxy_statuses.insert(
                            self.system_proxy
                                .reconcile(revision, proxy.clone(), guard, auto_launch)
                                .await,
                        ),
                    };
                    let kind = effect.kind();
                    owned
                        .iter()
                        .find(|status| status.kind == kind)
                        .cloned()
                        .unwrap_or_else(|| {
                            degraded(
                                kind,
                                revision,
                                "effect_owner_silent",
                                format!("the owner of {kind:?} reported no status"),
                                true,
                            )
                        })
                }
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
        let hotkeys = self.hotkeys.unregister_all().await;
        vec![hotkeys]
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
        system_proxy::{
            SystemProxyArgs, SystemProxyClient,
            ports::{MockAutoLaunchPort, MockOsProxyPort, MockPacPort, OsProxyConfig},
        },
    };
    use tokio_util::{sync::CancellationToken, task::TaskTracker};

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
    }

    async fn new_executor(
        adapters: Arc<dyn ApplicationEffectAdapters>,
    ) -> ApplicationEffectExecutor {
        let mut os = MockOsProxyPort::new();
        os.expect_get().returning(|| {
            Ok(OsProxyConfig {
                enable: false,
                host: "127.0.0.1".to_owned(),
                port: 7890,
                bypass: "localhost".to_owned(),
            })
        });
        os.expect_set().returning(|_| Ok(()));
        os.expect_default_bypass().return_const("localhost");

        let mut auto_launch = MockAutoLaunchPort::new();
        auto_launch.expect_is_enabled().returning(|| Ok(false));
        auto_launch.expect_set_enabled().returning(|_| Ok(()));

        let mut pac = MockPacPort::new();
        pac.expect_is_supported().returning(|| true);
        pac.expect_apply().returning(|_, _| Ok(()));
        pac.expect_disable().returning(|| Ok(()));

        let shutdown = CancellationToken::new();
        let tasks = TaskTracker::new();
        let system_proxy = SystemProxyClient::spawn(
            SystemProxyArgs {
                os: Arc::new(os),
                auto_launch: Arc::new(auto_launch),
                pac: Arc::new(pac),
                schedule_guard_ticks: false,
                shutdown,
            },
            &tasks,
        )
        .await
        .unwrap();
        let hotkeys = HotkeyClient::spawn(HotkeyArgs {
            registrar: Arc::new(NoopRegistrar),
            sink: Arc::new(NoopSink),
        })
        .await
        .unwrap();
        ApplicationEffectExecutor::new(
            system_proxy,
            hotkeys,
            Arc::new(PlatformAcceleratorValidator),
            adapters,
        )
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
        let adapters = Arc::new(RecordingAdapters::default());
        let executor = new_executor(adapters.clone()).await;
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
                EffectKind::Widget,
                EffectKind::Tray,
            ]
        );
        assert_eq!(statuses[5].kind, EffectKind::Hotkeys);
    }

    #[tokio::test]
    async fn missing_proxy_port_is_reported_as_retryable_without_calling_adapter() {
        let adapters = Arc::new(RecordingAdapters::default());
        let executor = new_executor(adapters.clone()).await;
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
        let adapters = Arc::new(RecordingAdapters {
            widget_unsupported: true,
            ..Default::default()
        });
        let executor = new_executor(adapters).await;
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
        let adapters = Arc::new(RecordingAdapters::default());
        let executor = new_executor(adapters.clone()).await;
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
        assert!(adapters.effects.lock().unwrap().is_empty());
    }
}

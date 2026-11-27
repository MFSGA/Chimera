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
        SystemProxyDesired, TransparentProxyDesired, TrayRefresh, TrayView,
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
    #[cfg(test)]
    core: Arc<dyn crate::client::core_lifecycle::CoreLifecyclePort>,
    /// Production side effects enter the ref CoreActor mailbox. The legacy
    /// port is retained for injected test adapters only.
    #[cfg(test)]
    core_router: Option<chimera_core::actor_v2::CoreClient>,
}

impl ApplicationEffectExecutor {
    pub(crate) fn new(
        system_proxy: SystemProxyClient,
        hotkeys: HotkeyClient,
        accelerators: Arc<dyn AcceleratorValidator>,
        adapters: Arc<dyn ApplicationEffectAdapters>,
        #[cfg(test)] core: Arc<dyn crate::client::core_lifecycle::CoreLifecyclePort>,
    ) -> Self {
        Self {
            system_proxy,
            hotkeys,
            accelerators,
            adapters,
            #[cfg(test)]
            core,
            #[cfg(test)]
            core_router: None,
        }
    }

    #[cfg(test)]
    pub(crate) fn with_core_router(mut self, core: chimera_core::actor_v2::CoreClient) -> Self {
        self.core_router = Some(core);
        self
    }

    async fn apply_hotkeys(&self, revision: EffectRevision, raw: &[String]) -> EffectStatus {
        match HotkeyBindings::parse(raw, self.accelerators.as_ref()) {
            Ok(desired) => self.hotkeys.reconcile(revision, desired).await,
            Err(error) => degraded(
                EffectKind::Hotkeys,
                revision,
                "hotkey_invalid_bindings",
                crate::client::hotkey::localized_parse_error(&error),
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

    #[cfg(test)]
    async fn apply_transparent_proxy(
        &self,
        revision: EffectRevision,
        desired: &TransparentProxyDesired,
    ) -> EffectStatus {
        use chimera_config::clash::config::TransparentProxyMode as ConfigMode;
        use chimera_ipc::api::network::transparent_proxy::{
            NetworkTransparentProxyMode as WireMode, NetworkTransparentProxyRequest,
        };

        let enabled = desired.config.mode != ConfigMode::Disabled;
        if !cfg!(target_os = "linux") && !enabled {
            return healthy(EffectKind::TransparentProxy, revision);
        }
        let port = if enabled {
            match desired.port.filter(|port| *port != 0) {
                Some(port) => port,
                None => {
                    return degraded(
                        EffectKind::TransparentProxy,
                        revision,
                        "transparent_proxy_failed",
                        "transparent proxy listener port is not resolved".into(),
                        true,
                    );
                }
            }
        } else {
            0
        };
        let mode = match desired.config.mode {
            ConfigMode::Disabled => WireMode::Disabled,
            ConfigMode::Redir => WireMode::Redir,
            ConfigMode::Tproxy => WireMode::Tproxy,
        };
        let request = NetworkTransparentProxyRequest {
            mode,
            port,
            local: desired.config.local,
            interfaces: desired.config.interfaces.clone(),
            ipv6: desired.config.ipv6,
            expected_revision: chimera_ipc::api::status::RevisionIdInfo {
                epoch: 0,
                generation: 0,
                effective_hash: String::new(),
            },
        };
        let result = match &self.core_router {
            Some(core) => core
                .reconcile_transparent_proxy(request)
                .await
                .map_err(|error| (error.to_string(), error.retryable)),
            None => self
                .core
                .reconcile_transparent_proxy(&request)
                .await
                .map_err(|error| (format!("{error:#}"), true)),
        };
        let status = match result {
            Ok(status) => status,
            Err((message, retryable)) => {
                return degraded(
                    EffectKind::TransparentProxy,
                    revision,
                    "transparent_proxy_failed",
                    message,
                    retryable,
                );
            }
        };
        if !status.supported && !enabled && !status.active {
            return healthy(EffectKind::TransparentProxy, revision);
        }
        if let Some(error) = status.error {
            return degraded(
                EffectKind::TransparentProxy,
                revision,
                "transparent_proxy_failed",
                error,
                true,
            );
        }
        if !status.supported || status.active != enabled || (enabled && status.mode != Some(mode)) {
            return degraded(
                EffectKind::TransparentProxy,
                revision,
                "transparent_proxy_failed",
                "the Service did not reach the requested transparent proxy state".into(),
                true,
            );
        }
        healthy(EffectKind::TransparentProxy, revision)
    }

    async fn apply_tray(
        &self,
        revision: EffectRevision,
        refresh: TrayRefresh,
        view: TrayView,
    ) -> EffectStatus {
        self.status_from_result(
            revision,
            EffectKind::Tray,
            self.adapters.refresh_tray(refresh, view).await,
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
                ApplicationEffect::CoreLogStorage(_) => {
                    #[cfg(test)]
                    {
                        healthy(EffectKind::CoreLogStorage, revision)
                    }
                    #[cfg(not(test))]
                    {
                        unreachable!("CoreRuntimeEffects owns storage")
                    }
                }
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
                ApplicationEffect::TransparentProxy(desired) => {
                    #[cfg(test)]
                    {
                        self.apply_transparent_proxy(revision, desired).await
                    }
                    #[cfg(not(test))]
                    {
                        let _ = desired;
                        unreachable!("CoreRuntimeEffects owns transparent capture")
                    }
                }
                ApplicationEffect::Hotkeys(raw) => self.apply_hotkeys(revision, raw).await,
                ApplicationEffect::Widget(config) => self.status_from_result(
                    revision,
                    EffectKind::Widget,
                    self.adapters.set_widget(*config).await,
                ),
                ApplicationEffect::WidgetLogo(logo) => self.status_from_result(
                    revision,
                    EffectKind::WidgetLogo,
                    self.adapters.set_widget_logo(*logo).await,
                ),
                ApplicationEffect::Tray(refresh, view) => {
                    self.apply_tray(revision, *refresh, *view).await
                }
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

use super::status::{degraded, healthy};
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

    use chimera_contracts::system_proxy::{
        MockAutoLaunchPort, MockOsProxyPort, MockPacPort, OsProxyConfig,
    };

    use super::*;
    use crate::client::{
        effects::{adapters::EffectAdapterError, plan::ApplicationEffectInputs},
        hotkey::{
            HotkeyArgs,
            adapters::PlatformAcceleratorValidator,
            ports::{HotkeyAction, HotkeyActionSink, ShortcutRegistrar},
        },
        system_proxy::{SystemProxyArgs, SystemProxyClient},
    };
    use tokio_util::{sync::CancellationToken, task::TaskTracker};

    #[derive(Default)]
    struct RecordingAdapters {
        effects: Mutex<Vec<EffectKind>>,
        tray_refreshes: Mutex<Vec<(TrayRefresh, TrayView)>>,
        widget_unavailable: bool,
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

        async fn set_widget(
            &self,
            _config: chimera_config::application::NetworkStatisticWidgetConfig,
        ) -> Result<(), EffectAdapterError> {
            self.effects.lock().unwrap().push(EffectKind::Widget);
            if self.widget_unavailable
                && matches!(_config, NetworkStatisticWidgetConfig::Enabled(_))
            {
                return Err(EffectAdapterError::Failed {
                    code: "widget_unavailable",
                    message: "widget runtime not installed".into(),
                    retryable: true,
                });
            }
            Ok(())
        }

        async fn set_widget_logo(
            &self,
            _logo: super::super::plan::WidgetLogoDesired,
        ) -> Result<(), EffectAdapterError> {
            self.effects.lock().unwrap().push(EffectKind::WidgetLogo);
            Ok(())
        }

        async fn refresh_tray(
            &self,
            refresh: TrayRefresh,
            view: TrayView,
        ) -> Result<(), EffectAdapterError> {
            self.effects.lock().unwrap().push(EffectKind::Tray);
            self.tray_refreshes.lock().unwrap().push((refresh, view));
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
            Arc::new(crate::client::core_lifecycle::LegacyCoreBridge::new(
                Arc::new(crate::core::actor_v2::CoreFacade::new_local()),
            )),
        )
    }

    struct NoopRegistrar;

    #[async_trait::async_trait]
    impl ShortcutRegistrar for NoopRegistrar {
        fn validate(
            &self,
            _accelerator: &str,
        ) -> Result<(), super::super::super::hotkey::ports::HotkeyParseError> {
            Ok(())
        }

        async fn register(
            &self,
            _accelerator: &str,
            _action: HotkeyAction,
            _sink: Arc<dyn HotkeyActionSink>,
        ) -> anyhow::Result<()> {
            Ok(())
        }

        async fn unregister(&self, _accelerator: &str) -> anyhow::Result<()> {
            Ok(())
        }

        async fn unregister_all(&self) -> anyhow::Result<()> {
            Ok(())
        }
    }

    struct NoopSink;
    impl HotkeyActionSink for NoopSink {
        fn dispatch(&self, _action: HotkeyAction) {}
    }

    struct EffectsLocalHost;

    #[async_trait::async_trait]
    impl chimera_core::actor_v2::endpoint::ControlEndpoint for EffectsLocalHost {
        fn host(&self) -> chimera_core::actor_v2::endpoint::ExecutionHost {
            chimera_core::actor_v2::endpoint::ExecutionHost::Local
        }

        async fn submit(
            &self,
            _: chimera_core::actor_v2::endpoint::CoreSubmission,
        ) -> Result<chimera_ipc::api::core::v2::OperationInfo, chimera_core_manager::CoreError>
        {
            panic!("effects must not submit a Core lifecycle operation")
        }

        async fn wait_operation(
            &self,
            _: chimera_core_manager::OperationId,
            _: std::time::Duration,
        ) -> Option<chimera_ipc::api::core::v2::OperationInfo> {
            panic!("effects must not wait for unrelated Core operations")
        }

        async fn status(
            &self,
        ) -> Result<
            chimera_core::actor_v2::endpoint::CoreStatusSnapshot,
            chimera_core_manager::CoreError,
        > {
            Ok(chimera_core::actor_v2::endpoint::CoreStatusSnapshot {
                instance_id: None,
                controller: None,
                state: Some(chimera_ipc::api::status::CoreStateDetail::Stopped { reason: None }),
                state_changed_at: 0,
                revision: None,
                source_hash: None,
                healthy: Some(true),
                applied_kind: None,
            })
        }
    }

    #[tokio::test]
    async fn production_transparent_effect_uses_ref_host_fence_not_legacy_port() {
        use chimera_config::clash::config::TransparentProxyMode;
        let core = chimera_core::actor_v2::CoreClient::spawn(Arc::new(EffectsLocalHost))
            .await
            .unwrap();
        let executor = new_executor(Arc::new(RecordingAdapters::default()))
            .await
            .with_core_router(core.clone());
        let mut config = chimera_config::clash::config::TransparentProxyConfig::default();
        config.mode = TransparentProxyMode::Redir;
        let status = executor
            .apply_transparent_proxy(
                EffectRevision::new(12),
                &TransparentProxyDesired {
                    config,
                    port: Some(7890),
                },
            )
            .await;
        assert!(matches!(
            status.health,
            EffectHealth::Degraded {
                code: "transparent_proxy_failed",
                retryable: false,
                ..
            }
        ));
        core.finish_shutdown().await;
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
                EffectKind::WidgetLogo,
                EffectKind::Tray,
            ]
        );
        assert_eq!(
            adapters.tray_refreshes.lock().unwrap().as_slice(),
            [(TrayRefresh::Full, inputs.tray_view())]
        );
        assert_eq!(statuses[6].kind, EffectKind::TransparentProxy);
        assert_eq!(statuses[7].kind, EffectKind::Hotkeys);
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
    async fn enabled_widget_without_runtime_is_reported_retryable() {
        let adapters = Arc::new(RecordingAdapters {
            widget_unavailable: true,
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

        assert!(matches!(
            statuses[0].health,
            EffectHealth::Degraded {
                code: "widget_unavailable",
                retryable: true,
                ..
            }
        ));
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
            redir_port: None,
            tproxy_port: None,
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

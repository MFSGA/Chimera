//! Reference widget controller and injected runtime boundary.

use crate::widget::{WIDGET_STOP_BOUND, WidgetError};
use chimera_config::application::{NetworkStatisticWidgetConfig, NetworkStatisticWidgetLogoSource};
use chimera_core::client::effects::plan::WidgetLogoState;
use chimera_paths::PathResolver;
use chimera_rpc_protocol::values::TrayIcon;
use chimera_widget_protocol::{Logo, StatisticWidgetVariant};
use std::sync::Arc;
use tokio::time::Instant;

#[derive(Debug, snafu::Snafu)]
pub enum InstallError {
    #[snafu(display("the widget runtime was installed twice"))]
    AlreadyInstalled,
}

#[async_trait::async_trait]
pub trait WidgetController: Send + Sync + 'static {
    async fn apply(&self, config: NetworkStatisticWidgetConfig) -> Result<(), WidgetError>;
    async fn show_logo(
        &self,
        source: NetworkStatisticWidgetLogoSource,
        state: WidgetLogoState,
    ) -> Result<(), WidgetError>;
}

/// The widget process itself, as the controller uses it.
///
/// Separate from [`WidgetController`](WidgetController): the controller owns the lifecycle rules
/// (when a start is redundant, when a stop is a no-op) and this is the thing
/// those rules drive, so the rules are testable without spawning a process.
#[async_trait::async_trait]
#[cfg_attr(test, mockall::automock)]
pub trait WidgetRuntime: Send + Sync + 'static {
    async fn start(&self, variant: StatisticWidgetVariant) -> Result<(), WidgetError>;
    async fn stop(&self, deadline: Instant) -> Result<(), WidgetError>;
    async fn is_running(&self) -> bool;
    /// Remembers `logo` for every later start and sends it to the running
    /// widget. A send that fails is only logged, as a traffic sample is.
    async fn set_logo(&self, logo: Logo);
}

/// The network statistic widget, bound after the client exists.
///
/// The widget needs a subscription to the client's connection stream, and the
/// client needs this controller — the two cannot both be built first. The
/// composition root therefore builds this empty, hands it to the executor, and
/// fills it in once the widget is up. It is a one-shot cell owned by the
/// composition root, not a global: nothing can look it up.
pub struct TauriWidgetController {
    /// Where the icons the logo shows are stored.
    paths: PathResolver,
    runtime: tokio::sync::OnceCell<Arc<dyn WidgetRuntime>>,
    /// The variant this controller last started. Narrow implementation detail,
    /// not shared state: only `apply` touches it, and it exists so a repeated
    /// configuration does not tear the widget down and build it again.
    started: tokio::sync::Mutex<Option<StatisticWidgetVariant>>,
}

impl TauriWidgetController {
    pub fn new(paths: PathResolver) -> Self {
        Self {
            paths,
            runtime: tokio::sync::OnceCell::new(),
            started: tokio::sync::Mutex::new(None),
        }
    }

    /// Called once by the composition root, after the widget is running.
    pub fn install(&self, runtime: Arc<dyn WidgetRuntime>) -> Result<(), InstallError> {
        self.runtime
            .set(runtime)
            .map_err(|_| InstallError::AlreadyInstalled)
    }

    fn runtime(&self) -> Result<&Arc<dyn WidgetRuntime>, WidgetError> {
        self.runtime.get().ok_or(WidgetError::Unavailable)
    }
}

#[async_trait::async_trait]
impl WidgetController for TauriWidgetController {
    async fn apply(&self, config: NetworkStatisticWidgetConfig) -> Result<(), WidgetError> {
        let runtime = self.runtime()?;
        let mut started = self.started.lock().await;
        match config {
            NetworkStatisticWidgetConfig::Disabled => {
                // Unconditional: a start that failed and could not clean up
                // keeps its widget owned without running. With nothing owned
                // the stop is a no-op.
                runtime
                    .stop(tokio::time::Instant::now() + WIDGET_STOP_BOUND)
                    .await?;
                *started = None;
            }
            NetworkStatisticWidgetConfig::Enabled(variant) => {
                // A reconcile repeats the whole desired state, so the same
                // variant arrives on every unrelated settings change. Starting
                // it again would stop and respawn a widget the user is looking
                // at.
                if *started == Some(variant) && runtime.is_running().await {
                    return Ok(());
                }
                runtime.start(variant).await?;
                *started = Some(variant);
            }
        }
        Ok(())
    }

    async fn show_logo(
        &self,
        source: NetworkStatisticWidgetLogoSource,
        state: WidgetLogoState,
    ) -> Result<(), WidgetError> {
        let runtime = self.runtime()?;
        let state = match state {
            WidgetLogoState::Normal => TrayIcon::Normal,
            WidgetLogoState::Tun => TrayIcon::Tun,
            WidgetLogoState::SystemProxy => TrayIcon::SystemProxy,
        };
        let logo = crate::core::widget_logo::resolve(&self.paths, source, state);
        runtime.set_logo(logo).await;
        Ok(())
    }
}

#[async_trait::async_trait]
impl WidgetRuntime for crate::widget::WidgetManager {
    async fn start(&self, variant: StatisticWidgetVariant) -> Result<(), WidgetError> {
        crate::widget::WidgetManager::start(self, variant).await
    }

    async fn stop(&self, deadline: tokio::time::Instant) -> Result<(), WidgetError> {
        crate::widget::WidgetManager::stop(self, deadline).await
    }

    async fn is_running(&self) -> bool {
        crate::widget::WidgetManager::is_running(self).await
    }

    async fn set_logo(&self, logo: Logo) {
        crate::widget::WidgetManager::set_logo(self, logo).await
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn widget_controller() -> TauriWidgetController {
        TauriWidgetController::new(PathResolver::with_base_dirs(
            "/nonexistent/config".into(),
            "/nonexistent/data".into(),
        ))
    }

    fn controller_with(runtime: MockWidgetRuntime) -> TauriWidgetController {
        let controller = widget_controller();
        controller
            .install(Arc::new(runtime))
            .expect("the runtime installs once");
        controller
    }

    #[tokio::test]
    async fn widget_before_install_is_unavailable() {
        assert!(matches!(
            widget_controller()
                .apply(NetworkStatisticWidgetConfig::Disabled)
                .await,
            Err(WidgetError::Unavailable)
        ));
    }

    #[tokio::test]
    async fn widget_logo_before_the_runtime_is_installed_is_unavailable() {
        let error = widget_controller()
            .show_logo(
                chimera_config::application::NetworkStatisticWidgetLogoSource::Tray,
                chimera_core::client::effects::plan::WidgetLogoState::Normal,
            )
            .await
            .unwrap_err();
        assert!(matches!(error, WidgetError::Unavailable));
    }

    #[tokio::test]
    async fn widget_same_variant_is_a_noop() {
        let mut runtime = MockWidgetRuntime::new();
        runtime
            .expect_start()
            .times(1)
            .returning(|_| Box::pin(async { Ok(()) }));
        runtime
            .expect_is_running()
            .returning(|| Box::pin(async { true }));
        runtime.expect_stop().never();
        let controller = controller_with(runtime);

        let desired = NetworkStatisticWidgetConfig::Enabled(StatisticWidgetVariant::Small);
        controller.apply(desired).await.expect("first apply starts");
        controller
            .apply(desired)
            .await
            .expect("a repeated apply is accepted");
    }

    #[tokio::test]
    async fn widget_variant_change_restarts_the_widget() {
        let mut runtime = MockWidgetRuntime::new();
        runtime
            .expect_start()
            .times(2)
            .returning(|_| Box::pin(async { Ok(()) }));
        runtime
            .expect_is_running()
            .returning(|| Box::pin(async { true }));
        let controller = controller_with(runtime);

        controller
            .apply(NetworkStatisticWidgetConfig::Enabled(
                StatisticWidgetVariant::Small,
            ))
            .await
            .expect("first apply starts");
        controller
            .apply(NetworkStatisticWidgetConfig::Enabled(
                StatisticWidgetVariant::Large,
            ))
            .await
            .expect("a different variant restarts");
    }

    #[tokio::test]
    async fn disabled_config_stops_a_running_widget() {
        let mut runtime = MockWidgetRuntime::new();
        runtime
            .expect_stop()
            .times(1)
            .returning(|_| Box::pin(async { Ok(()) }));
        runtime.expect_start().never();
        let controller = controller_with(runtime);

        controller
            .apply(NetworkStatisticWidgetConfig::Disabled)
            .await
            .expect("disabling stops the widget");
    }

    /// Whether the widget runs is not asked: one that is owned without running
    /// has to be stopped too, and a stop with nothing owned is a no-op.
    #[tokio::test]
    async fn disabled_config_stops_without_asking_whether_the_widget_runs() {
        let mut runtime = MockWidgetRuntime::new();
        runtime.expect_is_running().never();
        runtime
            .expect_stop()
            .times(1)
            .returning(|_| Box::pin(async { Ok(()) }));
        runtime.expect_start().never();
        let controller = controller_with(runtime);

        controller
            .apply(NetworkStatisticWidgetConfig::Disabled)
            .await
            .expect("there is nothing to stop");
    }
}

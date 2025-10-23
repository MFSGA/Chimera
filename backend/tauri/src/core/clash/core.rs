use std::{borrow::Cow, sync::Arc};

use anyhow::Result;
use nyanpasu_ipc::api::status::CoreState;
use nyanpasu_utils::core::instance::CoreInstanceBuilder;
use once_cell::sync::OnceCell;
use parking_lot::Mutex;

use crate::{
    config::{
        chimera::ClashCore,
        core::{Config, ConfigType},
    },
    utils::dirs,
};

#[derive(Debug, Clone, Copy)]
pub enum RunType {
    /// Run as child process directly
    Normal,
    /// Run by Nyanpasu Service via a ipc call
    Service,
    // TODO: Not implemented yet
    /// Run as elevated process, if profile advice to run as elevated
    Elevated,
}

impl Default for RunType {
    fn default() -> Self {
        let enable_service = {
            *Config::verge()
                .latest()
                .enable_service_mode
                .as_ref()
                .unwrap_or(&false)
        };
        if enable_service && crate::core::service::ipc::get_ipc_state().is_connected() {
            tracing::info!("run core as service");
            RunType::Service
        } else {
            tracing::info!("run core as child process");
            RunType::Normal
        }
    }
}

#[derive(Debug)]
enum Instance {
    Child {},
    Service {},
}

impl Instance {
    /// get core state with state changed timestamp
    pub async fn status<'a>(&self) -> (Cow<'a, CoreState>, i64) {
        todo!()
    }

    pub fn run_type(&self) -> RunType {
        match self {
            Instance::Child { .. } => RunType::Normal,
            Instance::Service { .. } => RunType::Service,
        }
    }

    pub async fn state<'a>(&self) -> Cow<'a, CoreState> {
        todo!()
    }

    pub async fn stop(&self) -> Result<()> {
        todo!()
    }

    pub fn try_new(run_type: RunType) -> Result<Self> {
        let core_type: nyanpasu_utils::core::CoreType = {
            (Config::verge()
                .latest()
                .clash_core
                .as_ref()
                .unwrap_or(&ClashCore::ClashPremium))
            .into()
        };

        let data_dir = camino::Utf8PathBuf::from_path_buf(dirs::app_data_dir()?)
            .map_err(|e| anyhow::anyhow!("failed to convert data dir to utf8 path: {:?}", e))?;
        let binary = camino::Utf8PathBuf::from_path_buf(find_binary_path(&core_type)?)
            .map_err(|e| anyhow::anyhow!("failed to convert binary path to utf8 path: {:?}", e))?;
        let config_path = camino::Utf8PathBuf::from_path_buf(Config::generate_file(
            ConfigType::Run,
        )?)
        .map_err(|e| anyhow::anyhow!("failed to convert config path to utf8 path: {:?}", e))?;

        let pid_path = camino::Utf8PathBuf::from_path_buf(dirs::clash_pid_path()?)
            .map_err(|e| anyhow::anyhow!("failed to convert pid path to utf8 path: {:?}", e))?;
        match run_type {
            RunType::Normal => {
                let instance = Arc::new(
                    CoreInstanceBuilder::default()
                        .core_type(core_type)
                        .app_dir(data_dir)
                        .binary_path(binary)
                        .config_path(config_path.clone())
                        .pid_path(pid_path)
                        .build()?,
                );
                Ok(Instance::Child {
                    // child: Mutex::new(instance),
                    // kill_flag: Arc::new(AtomicBool::new(false)),
                    // stated_changed_at: Arc::new(AtomicI64::new(get_current_ts())),
                })
            }
            RunType::Service => {
                todo!()
            }
            RunType::Elevated => {
                todo!()
            }
        }
    }

    pub async fn start(&self) -> Result<()> {
        todo!()
    }
}

#[derive(Debug)]
pub struct CoreManager {
    instance: Mutex<Option<Arc<Instance>>>,
}

impl CoreManager {
    pub fn global() -> &'static CoreManager {
        static CORE_MANAGER: OnceCell<CoreManager> = OnceCell::new();
        CORE_MANAGER.get_or_init(|| CoreManager {
            instance: Mutex::new(None),
        })
    }

    pub async fn status<'a>(&self) -> (Cow<'a, CoreState>, i64, RunType) {
        let instance = {
            let instance = self.instance.lock();
            instance.as_ref().cloned()
        };
        if let Some(instance) = instance {
            let (state, ts) = instance.status().await;
            (state, ts, instance.run_type())
        } else {
            (
                Cow::Owned(CoreState::Stopped(None)),
                0_i64,
                RunType::default(),
            )
        }
    }

    /// 启动核心
    pub async fn run_core(&self) -> Result<()> {
        {
            let instance = {
                let instance = self.instance.lock();
                instance.as_ref().cloned()
            };
            if let Some(instance) = instance.as_ref()
                && matches!(instance.state().await.as_ref(), CoreState::Running)
            {
                log::debug!(target: "app", "core is already running, stop it first...");
                instance.stop().await?;
            }
        }

        // 检查端口是否可用
        Config::clash()
            .latest()
            .prepare_external_controller_port()?;
        let run_type = RunType::default();
        let instance = Arc::new(Instance::try_new(run_type)?);

        #[cfg(target_os = "macos")]
        {
            let enable_tun = Config::verge().latest().enable_tun_mode.unwrap_or(false);
            let _ = self
                .change_default_network_dns(enable_tun)
                .await
                .inspect_err(|e| log::error!(target: "app", "failed to set system dns: {:?}", e));
        }

        {
            let mut this = self.instance.lock();
            *this = Some(instance.clone());
        }
        instance.start().await
    }
}

// TODO: support system path search via a config or flag
// FIXME: move this fn to nyanpasu-utils
/// Search the binary path of the core: Data Dir -> Sidecar Dir
pub fn find_binary_path(
    core_type: &nyanpasu_utils::core::CoreType,
) -> std::io::Result<std::path::PathBuf> {
    let data_dir = dirs::app_data_dir()
        .map_err(|err| std::io::Error::new(std::io::ErrorKind::NotFound, err.to_string()))?;
    let binary_path = data_dir.join(core_type.get_executable_name());
    if binary_path.exists() {
        return Ok(binary_path);
    }
    let app_dir = dirs::app_install_dir()
        .map_err(|err| std::io::Error::new(std::io::ErrorKind::NotFound, err.to_string()))?;
    let binary_path = app_dir.join(core_type.get_executable_name());
    if binary_path.exists() {
        return Ok(binary_path);
    }
    Err(std::io::Error::new(
        std::io::ErrorKind::NotFound,
        format!("{} not found", core_type.get_executable_name()),
    ))
}

pub use chimera_config::application::LoggingLevel;

use super::IVerge;

impl IVerge {
    pub fn get_log_level(&self) -> LoggingLevel {
        self.app_log_level.clone().unwrap_or_default()
    }
}

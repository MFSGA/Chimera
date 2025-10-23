use serde_yaml::Mapping;

use crate::{config::core::Config, enhance::chain::PostProcessingOutput};

mod chain;

/// Enhance mode
/// 返回最终配置、该配置包含的键、和script执行的结果
pub async fn enhance() -> (Mapping, Vec<String>, PostProcessingOutput) {
    // config.yaml 的配置
    let clash_config = { Config::clash().latest().0.clone() };

    todo!()
}

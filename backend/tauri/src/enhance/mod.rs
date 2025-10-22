use serde_yaml::Mapping;

use crate::enhance::chain::PostProcessingOutput;

mod chain;

/// Enhance mode
/// 返回最终配置、该配置包含的键、和script执行的结果
pub async fn enhance() -> (Mapping, Vec<String>, PostProcessingOutput) {
    todo!()
}

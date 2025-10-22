use serde::{Deserialize, Serialize};

#[derive(Default, Debug, Clone, Serialize, Deserialize, specta::Type)]
/// 后处理输出
pub struct PostProcessingOutput {}

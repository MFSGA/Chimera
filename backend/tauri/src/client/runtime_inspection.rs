//! Desktop IPC forwarding to the shared runtime inspection reader.
use super::ChimeraClient;
use chimera_core::client::runtime_inspection::RuntimeInspectionReader;
pub use chimera_core::client::runtime_inspection::{
    RuntimeInspection, RuntimeInspectionContent, RuntimeInspectionDiff, RuntimeInspectionNode,
};

impl ChimeraClient {
    fn inspection_reader(&self) -> RuntimeInspectionReader<'_> {
        RuntimeInspectionReader::new(self.inner.core.as_ref(), self.inner.core_api.as_ref())
    }

    pub(crate) async fn runtime_exists(&self) -> Vec<String> {
        self.inspection_reader().runtime_exists().await
    }
    pub(crate) async fn inspect_runtime(&self) -> Option<RuntimeInspection> {
        self.inspection_reader().inspect_runtime().await
    }
    pub(crate) async fn inspect_applied_runtime(&self) -> Option<RuntimeInspection> {
        self.inspection_reader().inspect_applied_runtime().await
    }
    pub(crate) async fn inspect_runtime_node(
        &self,
        snapshot_id: &str,
        node_id: u32,
    ) -> anyhow::Result<RuntimeInspectionContent> {
        self.inspection_reader()
            .inspect_runtime_node(snapshot_id, node_id)
            .await
    }
}

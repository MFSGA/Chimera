//! Global hotkeys.
use crate::registry::RpcState;
use chimera_core::client::ChimeraClient;

#[chimera_rpc::rpc(query, expose = [http])]
pub fn get_hotkey_functions() -> Vec<&'static str> {
    #[cfg(not(any(target_os = "android", target_os = "ios")))]
    {
        use chimera_core::client::hotkey::ports::{HotkeyAction, action_available};
        HotkeyAction::all()
            .iter()
            .filter(|action| action_available(**action))
            .map(|action| action.as_str())
            .collect()
    }
    #[cfg(any(target_os = "android", target_os = "ios"))]
    Vec::new()
}

#[chimera_rpc::rpc(query, expose = [http])]
pub async fn get_hotkeys(client: RpcState<'_, ChimeraClient>) -> anyhow::Result<Vec<String>> {
    Ok(client.get_app_config()?.hotkeys)
}

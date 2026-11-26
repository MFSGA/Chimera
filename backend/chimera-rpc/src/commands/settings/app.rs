//! Ordinary and sensitive application settings from the committed owner.
use crate::registry::RpcState;
use anyhow::Result;
use chimera_config::application::{ApplicationSettings, ChimeraAppConfig};
use chimera_core::client::ChimeraClient;

#[chimera_rpc::rpc(query, expose = [http], sensitive)]
pub async fn get_app_config(client: RpcState<'_, ChimeraClient>) -> Result<ChimeraAppConfig> {
    Ok(client.get_app_config()?)
}

#[chimera_rpc::rpc(query, expose = [http, local])]
pub async fn get_app_settings(client: RpcState<'_, ChimeraClient>) -> Result<ApplicationSettings> {
    Ok((&client.get_app_config()?).into())
}

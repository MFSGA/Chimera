//! Desktop adapter for shared network automation; production remains disabled.
pub(crate) use chimera_core::client::network_automation::ports;
pub(crate) use chimera_core::client::network_automation::{
    NetworkAutomationArgs, NetworkAutomationClient,
};
mod switches;
pub(crate) use switches::ConfigCaptureSwitches;
#[cfg(test)]
mod tests;

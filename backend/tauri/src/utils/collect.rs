use std::collections::HashMap;

use serde::Serialize;
use sysinfo::System;

use crate::{
    config::core::Config,
    consts::{BUILD_INFO, BuildInfo},
};

#[derive(Debug, Serialize, specta::Type)]
pub struct DeviceInfo {
    pub cpu: Vec<String>,
    pub memory: String,
}

#[derive(Debug, Serialize, specta::Type)]
pub struct EnvInfo {
    pub os: String,
    pub arch: String,
    pub core: HashMap<String, String>,
    pub device: DeviceInfo,
    pub build_info: BuildInfo,
}

fn format_bytes(bytes: u64) -> String {
    const UNITS: [&str; 5] = ["B", "KiB", "MiB", "GiB", "TiB"];
    let mut value = bytes as f64;
    let mut unit = 0;

    while value >= 1024.0 && unit < UNITS.len() - 1 {
        value /= 1024.0;
        unit += 1;
    }

    if unit == 0 {
        format!("{} {}", bytes, UNITS[unit])
    } else {
        format!("{value:.2} {}", UNITS[unit])
    }
}

fn collect_device_info(system: &System) -> DeviceInfo {
    let mut cpus: Vec<(u64, String, usize)> = Vec::new();

    for cpu in system.cpus() {
        let brand = cpu.brand().to_string();
        if let Some((_, _, count)) = cpus.iter_mut().find(|(_, name, _)| name == &brand) {
            *count += 1;
        } else {
            cpus.push((cpu.frequency(), brand, 1));
        }
    }

    DeviceInfo {
        cpu: cpus
            .into_iter()
            .map(|(freq, name, count)| format!("{name} @ {:.2}GHz x {count}", freq as f64 / 1000.0))
            .collect(),
        memory: format_bytes(system.total_memory()),
    }
}

fn collect_core_info() -> HashMap<String, String> {
    let mut core = HashMap::new();
    let verge = Config::verge();
    let latest = verge.latest();
    let configured_core = latest.clash_core.unwrap_or_default();

    core.insert("configured_core".to_string(), configured_core.to_string());

    core
}

pub fn collect_envs() -> EnvInfo {
    let mut system = System::new_all();
    system.refresh_all();

    EnvInfo {
        os: format!(
            "{} {}",
            System::long_os_version().unwrap_or_default(),
            System::kernel_version().unwrap_or_default(),
        )
        .trim()
        .to_string(),
        arch: System::cpu_arch(),
        core: collect_core_info(),
        device: collect_device_info(&system),
        build_info: BUILD_INFO.clone(),
    }
}

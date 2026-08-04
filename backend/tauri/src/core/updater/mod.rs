use std::{
    collections::HashMap,
    sync::{Arc, OnceLock},
    time::{Duration, Instant},
};

use crate::{
    config::chimera::ClashCore,
    utils::candy::{ReqwestSpeedTestExt, parse_gh_url},
};
use anyhow::{Result, anyhow};
use dashmap::DashMap;
use serde::{Deserialize, Serialize};
use specta::Type;
use tokio::sync::RwLock;

mod instance;
mod shared;

pub use instance::UpdaterSummary;

#[cfg(not(feature = "e2e"))]
pub(crate) fn recover_interrupted_updates_on_launch() -> Result<()> {
    const CORE_NAMES: [&str; 6] = [
        "clash",
        "clash-rs",
        "mihomo",
        "chimera-client",
        "mihomo-alpha",
        "clash-rs-alpha",
    ];
    let executable = tauri::utils::platform::current_exe()?;
    let core_dir = executable
        .parent()
        .ok_or_else(|| anyhow!("failed to get core directory during updater recovery"))?;
    instance::recover_interrupted_core_replacements_in_dir(core_dir, &CORE_NAMES)
}

const MIRROR_CACHE_TTL: Duration = Duration::from_secs(60 * 60);

fn mirror_cache_is_fresh(cached_at: Option<Instant>, now: Instant) -> bool {
    cached_at.is_some_and(|cached_at| now.saturating_duration_since(cached_at) < MIRROR_CACHE_TTL)
}

pub struct UpdaterManager {
    manifest_version: ManifestVersion,
    client: reqwest::Client,
    mirror: Arc<parking_lot::RwLock<Option<(String, Instant)>>>,
    instances: Arc<DashMap<usize, Arc<instance::Updater>>>,
}

impl Default for UpdaterManager {
    fn default() -> Self {
        Self {
            manifest_version: ManifestVersion::default(),
            client: crate::utils::candy::get_reqwest_client().unwrap(),
            mirror: Arc::new(parking_lot::RwLock::new(None)),
            instances: Arc::new(DashMap::new()),
        }
    }
}

#[derive(Deserialize, Serialize, Clone, Debug)]
pub struct ManifestVersion {
    manifest_version: u64,
    latest: ManifestVersionLatest,
    arch_template: ArchTemplate,
    updated_at: String,
}

// TODO: manifest v2 should be kebad-case
#[derive(Deserialize, Serialize, Clone, Debug, Type)]
pub struct ManifestVersionLatest {
    mihomo: String,
    mihomo_alpha: String,
    clash_rs: String,
    chimera_client: String,
    clash_rs_alpha: String,
    clash_premium: String,
}

#[derive(Deserialize, Serialize, Default, Clone, Debug)]
pub struct ArchTemplate {
    mihomo: HashMap<String, String>,
    mihomo_alpha: HashMap<String, String>,
    clash_rs: HashMap<String, String>,
    clash_rs_alpha: HashMap<String, String>,
    clash_premium: HashMap<String, String>,
    chimera_client: HashMap<String, String>,
}

impl Default for ManifestVersion {
    fn default() -> Self {
        Self {
            manifest_version: 0,
            latest: ManifestVersionLatest::default(),
            arch_template: ArchTemplate::default(),
            updated_at: String::new(),
        }
    }
}

impl Default for ManifestVersionLatest {
    fn default() -> Self {
        Self {
            mihomo: String::new(),
            mihomo_alpha: String::new(),
            clash_rs: String::new(),
            chimera_client: String::new(),
            clash_rs_alpha: String::new(),
            clash_premium: String::new(),
        }
    }
}

impl ManifestVersion {
    fn get_matches(&self, core_type: &ClashCore) -> Option<(String, shared::CoreTypeMeta)> {
        let arch = shared::get_arch().ok()?;
        match core_type {
            ClashCore::ClashPremium => Some((
                self.arch_template
                    .clash_premium
                    .get(arch)?
                    .clone()
                    .replace("{}", &self.latest.clash_premium),
                shared::CoreTypeMeta::ClashPremium(self.latest.clash_premium.clone()),
            )),
            ClashCore::Mihomo => Some((
                self.arch_template
                    .mihomo
                    .get(arch)?
                    .clone()
                    .replace("{}", &self.latest.mihomo),
                shared::CoreTypeMeta::Mihomo(self.latest.mihomo.clone()),
            )),
            ClashCore::MihomoAlpha => Some((
                self.arch_template
                    .mihomo_alpha
                    .get(arch)?
                    .clone()
                    .replace("{}", &self.latest.mihomo_alpha),
                shared::CoreTypeMeta::MihomoAlpha,
            )),
            ClashCore::ClashRs => Some((
                self.arch_template
                    .clash_rs
                    .get(arch)?
                    .clone()
                    .replace("{}", &self.latest.clash_rs),
                shared::CoreTypeMeta::ClashRs(self.latest.clash_rs.clone()),
            )),
            ClashCore::ChimeraClient => Some((
                self.arch_template
                    .chimera_client
                    .get(arch)
                    .or_else(|| self.arch_template.clash_rs.get(arch))?
                    .clone()
                    .replace(
                        "{}",
                        if self.latest.chimera_client.is_empty() {
                            &self.latest.clash_rs
                        } else {
                            &self.latest.chimera_client
                        },
                    ),
                shared::CoreTypeMeta::ChimeraClient(if self.latest.chimera_client.is_empty() {
                    self.latest.clash_rs.clone()
                } else {
                    self.latest.chimera_client.clone()
                }),
            )),
            ClashCore::ClashRsAlpha => Some((
                self.arch_template
                    .clash_rs_alpha
                    .get(arch)?
                    .clone()
                    .replace("{}", &self.latest.clash_rs_alpha),
                shared::CoreTypeMeta::ClashRsAlpha,
            )),
        }
    }
}

impl UpdaterManager {
    pub fn new() -> Self {
        Self::default()
    }

    pub fn global() -> &'static RwLock<Self> {
        static INSTANCE: OnceLock<RwLock<UpdaterManager>> = OnceLock::new();
        INSTANCE.get_or_init(|| RwLock::new(UpdaterManager::new()))
    }

    pub fn get_latest_versions(&self) -> ManifestVersionLatest {
        self.manifest_version.latest.clone()
    }

    pub fn get_mirror(&self) -> Option<String> {
        self.mirror.read().clone().map(|(mirror, _)| mirror)
    }

    async fn get_latest_version_manifest(&self, mirror: &str) -> Result<ManifestVersion> {
        let url = parse_gh_url(
            mirror,
            // "https://github.com/libnyanpasu/clash-nyanpasu/raw/main/manifest/version.json",
            "https://github.com/MFSGA/Chimera/raw/master/manifest/version.json",
        )?;
        let res = self.client.get(url).send().await?;
        let status_code = res.status();
        if !status_code.is_success() {
            anyhow::bail!(
                "failed to get latest version manifest: response status is {}, expected 200",
                status_code
            );
        }
        Ok(res.json::<ManifestVersion>().await?)
    }

    pub async fn fetch_latest(&mut self) -> Result<()> {
        self.mirror_speed_test().await?;
        let mirror = self.get_mirror().unwrap();
        self.manifest_version = self.get_latest_version_manifest(&mirror).await?;
        Ok(())
    }

    pub async fn mirror_speed_test(&self) -> Result<()> {
        {
            let mirror = self.mirror.read();
            if mirror_cache_is_fresh(
                mirror.as_ref().map(|(_, cached_at)| *cached_at),
                Instant::now(),
            ) {
                return Ok(());
            }
        }

        let path = "https://github.com/MFSGA/Chimera/raw/master/manifest/version.json";
        let client = crate::utils::candy::get_reqwest_client()?;
        let results = client
            .mirror_speed_test(crate::utils::candy::INTERNAL_MIRRORS, path)
            .await?;
        let (fastest_mirror, speed) = results.first().ok_or(anyhow!("no mirrors found"))?;
        if speed - 1.0 < 0.0001 {
            anyhow::bail!("all mirrors are too slow");
        }
        {
            let mut mirror = self.mirror.write();
            *mirror = Some((fastest_mirror.to_string(), Instant::now()));
        }
        Ok(())
    }

    pub async fn update_core(&mut self, core_type: &ClashCore) -> Result<usize> {
        if self.manifest_version.manifest_version == 0 {
            self.fetch_latest().await?;
        } else {
            self.mirror_speed_test().await?;
        }
        let (artifact, tag) = self
            .manifest_version
            .get_matches(core_type)
            .ok_or(anyhow!("no matches found for core type: {:?}", core_type))?;
        let mirror = self.get_mirror().unwrap();
        let updater = Arc::new(
            instance::UpdaterBuilder::new()
                .set_client(self.client.clone())
                .set_core_type(*core_type)
                .set_mirror(mirror)
                .set_artifact(artifact)
                .set_tag(tag)
                .build()
                .await?,
        );
        let updater_ref = updater.clone();
        let updater_id = updater.get_updater_id();
        self.instances.insert(updater_id, updater);
        tokio::spawn(async move {
            updater_ref.start().await;
        });
        Ok(updater_id)
    }

    pub fn inspect_updater(&self, updater_id: usize) -> Option<UpdaterSummary> {
        let updater = self.instances.get(&updater_id)?;
        let report = updater.get_report();
        if matches!(
            report.state,
            instance::UpdaterState::Done | instance::UpdaterState::Failed(_)
        ) {
            let map = self.instances.clone();
            tokio::spawn(async move {
                tokio::time::sleep(std::time::Duration::from_secs(5)).await;
                map.remove(&updater_id);
            });
        }
        Some(report)
    }
}

#[cfg(test)]
mod tests {
    use std::time::{Duration, Instant};

    use super::mirror_cache_is_fresh;

    #[test]
    fn mirror_cache_freshness_uses_monotonic_one_hour_boundary() {
        let now = Instant::now();
        assert!(!mirror_cache_is_fresh(None, now));
        assert!(mirror_cache_is_fresh(Some(now), now));
        assert!(mirror_cache_is_fresh(
            now.checked_sub(Duration::from_secs(3_599)),
            now
        ));
        assert!(!mirror_cache_is_fresh(
            now.checked_sub(Duration::from_secs(3_600)),
            now
        ));
        assert!(!mirror_cache_is_fresh(
            now.checked_sub(Duration::from_secs(3_601)),
            now
        ));
    }
}

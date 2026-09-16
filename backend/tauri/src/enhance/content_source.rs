//! ProfileContentSource over the real profiles directory.

use std::path::PathBuf;

use chimera_config::{
    profile::ManagedProfilePath,
    runtime::executor::{PortError, ProfileContentSource},
};

pub struct FsProfileContentSource {
    profiles_dir: PathBuf,
}

impl FsProfileContentSource {
    pub fn new(profiles_dir: PathBuf) -> Self {
        Self { profiles_dir }
    }
}

impl ProfileContentSource for FsProfileContentSource {
    fn read(&self, path: &ManagedProfilePath) -> Result<String, PortError> {
        let full = self.profiles_dir.join(path.as_path());
        std::fs::read_to_string(&full)
            .map_err(|error| format!("read profile content {}: {error}", full.display()).into())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn reads_relative_managed_paths_from_profiles_dir() {
        let temp = tempfile::tempdir().unwrap();
        std::fs::write(temp.path().join("abc.yaml"), "proxies: []\n").unwrap();
        let source = FsProfileContentSource::new(temp.path().to_path_buf());
        let content = source
            .read(&ManagedProfilePath::new("abc.yaml").unwrap())
            .unwrap();
        assert_eq!(content, "proxies: []\n");
        assert!(
            source
                .read(&ManagedProfilePath::new("missing.yaml").unwrap())
                .is_err()
        );
    }
}

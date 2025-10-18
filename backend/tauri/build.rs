use std::fs::{exists, read};

use serde::Deserialize;

#[derive(Deserialize)]
struct PackageJson {
    version: String, // we only need the version
}

fn main() {
    let version: String = if let Ok(true) = exists("../../package.json") {
        let raw = read("../../package.json").unwrap();
        let pkg_json: PackageJson = serde_json::from_slice(&raw).unwrap();
        pkg_json.version
    } else {
        let raw = read("./tauri.conf.json").unwrap(); // TODO: fix it when windows arm64 need it
        let tauri_json: PackageJson = serde_json::from_slice(&raw).unwrap();
        tauri_json.version
    };
    let version = semver::Version::parse(&version).unwrap();
    let is_prerelase = !version.pre.is_empty();
    println!("cargo:rustc-env=CHIMERA_VERSION={version}");
    tauri_build::build()
}

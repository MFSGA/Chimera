//! Single source of truth for runtime command registration and TypeScript bindings.

use std::{fs, io, path::Path};

use tauri_specta::{collect_commands, collect_events};

#[cfg(feature = "agent")]
use crate::features;

const GENERATED_GET_PROFILES_BINDING: &str = r#"  getProfiles: () =>
    typedError<ProfilesResponse_Serialize, string>(
      __TAURI_INVOKE('get_profiles'),
    ),"#;
const FLATTENED_GET_PROFILES_BINDING: &str = r#"  getProfiles: () =>
    typedError<ProfilesResponse, string>(__TAURI_INVOKE('get_profiles')),"#;

const GENERATED_PROFILE_BUILDER_BINDING: &str = r#"export type ProfileBuilderRequest_Deserialize =
  | { type: 'remote'; profile: RemoteProfileBuilder }
  | { type: 'local'; profile: LocalProfileBuilder_Deserialize };

export type ProfileBuilderRequest_Serialize =
  | { type: 'remote'; profile: RemoteProfileBuilder }
  | { type: 'local'; profile: LocalProfileBuilder_Serialize };"#;
const FLATTENED_PROFILE_BUILDER_BINDING: &str = r#"export type ProfileBuilderRequest_Deserialize =
  | ({ type: 'remote' } & RemoteProfileBuilder)
  | ({ type: 'local' } & LocalProfileBuilder_Deserialize);

export type ProfileBuilderRequest_Serialize =
  | ({ type: 'remote' } & RemoteProfileBuilder)
  | ({ type: 'local' } & LocalProfileBuilder_Serialize);"#;

const GENERATED_PROFILE_RESPONSE_BINDING: &str = r#"export type ProfileResponse =
  ProfileResponse_Serialize | ProfileResponse_Deserialize;

export type ProfileResponse_Deserialize =
  | { type: 'remote'; profile: RemoteProfile_Deserialize }
  | { type: 'local'; profile: LocalProfile_Deserialize };

export type ProfileResponse_Serialize =
  | { type: 'remote'; profile: RemoteProfile_Serialize }
  | { type: 'local'; profile: LocalProfile_Serialize };"#;
const FLATTENED_PROFILE_RESPONSE_BINDING: &str = r#"export type ProfileResponse =
  | ({ type: 'remote' } & RemoteProfile_Serialize)
  | ({ type: 'local' } & LocalProfile_Serialize);"#;

const GENERATED_PROFILES_RESPONSE_BINDING: &str = r#"export type ProfilesResponse =
  ProfilesResponse_Serialize | ProfilesResponse_Deserialize;

export type ProfilesResponse_Deserialize = {
  current: string | null;
  items: ProfileResponse_Deserialize[];
  valid: string[];
  global_transforms: string[];
};

export type ProfilesResponse_Serialize = {
  current: string | null;
  items: ProfileResponse_Serialize[];
  valid: string[];
  global_transforms: string[];
};"#;
const FLATTENED_PROFILES_RESPONSE_BINDING: &str = r#"export type ProfilesResponse = {
  current: string | null;
  items: ProfileResponse[];
  valid: string[];
  global_transforms: string[];
};"#;

fn apply_binding_rewrite(
    contents: &mut String,
    generated: &str,
    replacement: &str,
    label: &str,
) -> io::Result<()> {
    if contents.contains(generated) {
        *contents = contents.replacen(generated, replacement, 1);
        return Ok(());
    }

    if contents.contains(replacement) {
        return Ok(());
    }

    Err(io::Error::new(
        io::ErrorKind::InvalidData,
        format!("generated TypeScript binding pattern changed: {label}"),
    ))
}

/// Align Specta output with serde's flattened Profile enum representation.
pub(crate) fn normalize_typescript_bindings(path: impl AsRef<Path>) -> io::Result<()> {
    let path = path.as_ref();
    let mut contents = fs::read_to_string(path)?;
    let rewrites = [
        (
            GENERATED_GET_PROFILES_BINDING,
            FLATTENED_GET_PROFILES_BINDING,
            "get_profiles return type",
        ),
        (
            GENERATED_PROFILE_BUILDER_BINDING,
            FLATTENED_PROFILE_BUILDER_BINDING,
            "ProfileBuilderRequest",
        ),
        (
            GENERATED_PROFILE_RESPONSE_BINDING,
            FLATTENED_PROFILE_RESPONSE_BINDING,
            "ProfileResponse",
        ),
        (
            GENERATED_PROFILES_RESPONSE_BINDING,
            FLATTENED_PROFILES_RESPONSE_BINDING,
            "ProfilesResponse",
        ),
    ];

    for (generated, replacement, label) in rewrites {
        apply_binding_rewrite(&mut contents, generated, replacement, label)?;
    }

    fs::write(path, contents)
}

macro_rules! build_builder {
    ($($command_root:ident $(:: $command_path:ident)*),* $(,)?) => {
        tauri_specta::Builder::<tauri::Wry>::new()
            .commands(collect_commands![
                crate::greet,
                crate::ipc::get_sys_proxy,
                crate::ipc::get_profiles,
                crate::ipc::import_profile,
                crate::ipc::view_profile,
                crate::ipc::reorder_profile,
                crate::ipc::reorder_profiles_by_list,
                crate::ipc::activate_profile,
                crate::ipc::set_profile_valid_fields,
                crate::ipc::patch_profile_metadata,
                crate::ipc::patch_remote_profile_options,
                crate::ipc::replace_profile_definition,
                crate::ipc::update_profile,
                crate::ipc::patch_profile,
                crate::ipc::delete_profile,
                crate::ipc::read_profile_file,
                crate::ipc::save_profile_file,
                crate::ipc::create_profile,
                crate::ipc::create_editor_window,
                crate::ipc::get_verge_config,
                crate::ipc::patch_verge_config,
                crate::ipc::get_clash_info,
                crate::ipc::patch_clash_config,
                crate::ipc::patch_clash_core_config,
                crate::ipc::get_proxies,
                crate::ipc::select_proxy,
                crate::ipc::change_clash_core,
                crate::ipc::get_runtime_yaml,
                crate::ipc::get_core_status,
                crate::ipc::url_delay_test,
                crate::ipc::get_ipsb_asn,
                crate::ipc::uwp::invoke_uwp_tool,
                crate::ipc::collect_logs,
                crate::ipc::collect_envs,
                crate::ipc::get_custom_app_dir,
                crate::ipc::set_custom_app_dir,
                crate::ipc::clash_api_get_proxy_delay,
                crate::ipc::clash_api_get_group_delay,
                crate::ipc::clash_api_delete_connections,
                crate::ipc::get_clash_ws_connections_state,
                crate::ipc::get_clash_ws_snapshot,
                crate::ipc::set_clash_ws_recording,
                crate::ipc::clear_clash_ws_history,
                crate::ipc::check_update,
                crate::ipc::is_portable,
                crate::ipc::is_appimage,
                crate::ipc::open_that,
                crate::ipc::cleanup_processes,
                crate::ipc::get_server_port,
                crate::ipc::get_core_dir,
                crate::ipc::get_service_install_prompt,
                crate::ipc::open_app_config_dir,
                crate::ipc::open_app_data_dir,
                crate::ipc::open_core_dir,
                crate::ipc::open_logs_dir,
                crate::ipc::get_core_version,
                crate::ipc::update_core,
                crate::ipc::restart_sidecar,
                crate::ipc::fetch_latest_core_versions,
                crate::ipc::inspect_updater,
                crate::ipc::get_storage_item,
                crate::ipc::set_storage_item,
                crate::ipc::remove_storage_item,
                crate::ipc::get_all_storage_items,
                crate::ipc::clear_storage,
                crate::ipc::service::status_service,
                crate::ipc::service::install_service,
                crate::ipc::service::uninstall_service,
                crate::ipc::service::start_service,
                crate::ipc::service::stop_service,
                crate::ipc::service::restart_service,
                crate::ipc::save_window_size_state,
                crate::ipc::create_main_window,
                crate::ipc::create_legacy_window,
                $($command_root $(::$command_path)*),*
            ])
            .events(collect_events![
                crate::core::storage::StorageValueChangedEvent,
                crate::core::clash::ClashConnectionsEvent,
                crate::core::clash::ws::ClashWsEvent,
            ])
            .dangerously_cast_bigints_to_number()
    };
}

pub(crate) fn build_specta_builder() -> tauri_specta::Builder<tauri::Wry> {
    #[cfg(feature = "agent")]
    let builder = build_builder![
        features::agent::commands::agent_get_network_snapshot,
        features::agent::commands::agent_propose_network_action,
        features::agent::commands::agent_execute_network_action,
        features::agent::commands::agent_cancel_network_action,
    ];
    #[cfg(not(feature = "agent"))]
    let builder = build_builder![];
    builder
}

#[cfg(test)]
mod tests {
    use std::path::Path;

    use super::{apply_binding_rewrite, build_specta_builder, normalize_typescript_bindings};

    const BINDINGS_PATH: &str = concat!(
        env!("CARGO_MANIFEST_DIR"),
        "/../../frontend/interface/src/ipc/bindings.ts"
    );
    const PRETTIER_CONFIG: &str = concat!(env!("CARGO_MANIFEST_DIR"), "/../../.prettierrc.cjs");

    fn export_bindings(path: &Path) {
        build_specta_builder()
            .export(
                specta_typescript::Typescript::default()
                    .header("/* oxlint-disable */\n// @ts-nocheck"),
                path,
            )
            .expect("failed to export TypeScript bindings");
    }

    fn format_bindings(path: &Path) {
        let formatter = if cfg!(target_os = "windows") {
            "pnpm.cmd"
        } else {
            "pnpm"
        };
        let status = std::process::Command::new(formatter)
            .args(["exec", "prettier", "--config", PRETTIER_CONFIG, "--write"])
            .arg(path)
            .status()
            .expect("failed to run Prettier");
        assert!(status.success(), "Prettier failed for generated bindings");
    }

    #[test]
    fn binding_rewrite_accepts_already_normalized_output() {
        let mut contents = "normalized binding".to_owned();

        apply_binding_rewrite(
            &mut contents,
            "generated binding",
            "normalized binding",
            "fixture",
        )
        .expect("already normalized bindings should be accepted");

        assert_eq!(contents, "normalized binding");
    }

    #[test]
    fn binding_rewrite_rejects_unknown_output() {
        let mut contents = "unexpected binding".to_owned();

        let error = apply_binding_rewrite(
            &mut contents,
            "generated binding",
            "normalized binding",
            "fixture",
        )
        .expect_err("unknown binding output should fail validation");

        assert_eq!(error.kind(), std::io::ErrorKind::InvalidData);
        assert!(error.to_string().contains("fixture"));
    }

    #[test]
    fn typescript_bindings_are_fresh() {
        let interface_directory = Path::new(BINDINGS_PATH)
            .parent()
            .expect("bindings path must have a parent");
        let directory = tempfile::Builder::new()
            .prefix(".specta-check-")
            .tempdir_in(interface_directory)
            .expect("failed to create bindings temp directory");
        let generated_path = directory.path().join("bindings.ts");
        export_bindings(&generated_path);
        format_bindings(&generated_path);
        normalize_typescript_bindings(&generated_path)
            .expect("failed to normalize generated TypeScript bindings");
        let generated = std::fs::read_to_string(generated_path)
            .expect("failed to read generated TypeScript bindings");
        let checked_in = std::fs::read_to_string(BINDINGS_PATH)
            .expect("failed to read checked-in TypeScript bindings");
        assert_eq!(
            checked_in, generated,
            "TypeScript bindings are stale; run the ignored update_typescript_bindings test"
        );
    }

    #[test]
    #[ignore = "writes frontend/interface/src/ipc/bindings.ts"]
    fn update_typescript_bindings() {
        let path = Path::new(BINDINGS_PATH);
        export_bindings(path);
        format_bindings(path);
        normalize_typescript_bindings(path)
            .expect("failed to normalize generated TypeScript bindings");
    }
}

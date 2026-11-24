//! Runtime input projections, target identities, and source-change detection.

use std::collections::HashSet;

use chimera_config::{
    application::ChimeraAppConfig,
    clash::config::{
        ClashConfig, ClashControlChannel,
        clash_strategy::port::{ExternalControllerStrategy, PortStrategy},
        dns::{DnsSettings, OneOrMany},
        overrides::{ClashGuardOverrides, TunOverrides},
        tun_stack::TunStack,
    },
    profile::{ManagedProfilePath, ProfileDefinition, ProfileId, Profiles},
};
use indexmap::{IndexMap, IndexSet};

use crate::client::runtime_build_port::current_profile_closure;

/// Runtime projections supplied by each source domain.
pub trait RuntimeDomain {
    fn reaches_runtime(previous: &Self, candidate: &Self, touched: &[ManagedProfilePath]) -> bool;
}

impl RuntimeDomain for ChimeraAppConfig {
    fn reaches_runtime(previous: &Self, candidate: &Self, _: &[ManagedProfilePath]) -> bool {
        application_runtime_inputs(previous) != application_runtime_inputs(candidate)
    }
}
impl RuntimeDomain for ClashConfig {
    fn reaches_runtime(previous: &Self, candidate: &Self, _: &[ManagedProfilePath]) -> bool {
        clash_runtime_inputs(previous) != clash_runtime_inputs(candidate)
    }
}
impl RuntimeDomain for Profiles {
    fn reaches_runtime(previous: &Self, candidate: &Self, touched: &[ManagedProfilePath]) -> bool {
        let before = profiles_target(previous);
        let after = profiles_target(candidate);
        selection_changed(previous, candidate)
            || before.is_none()
            || before != after
            || touched.iter().any(|path| {
                closure_files(candidate, &current_profile_closure(candidate)).contains(path)
            })
    }
}

/// Whether committed build inputs or content in the current closure changes.
pub fn reaches_runtime<T: RuntimeDomain>(
    previous: &T,
    candidate: &T,
    touched: &[ManagedProfilePath],
) -> bool {
    T::reaches_runtime(previous, candidate, touched)
}

/// Which application-config fields the runtime build and the core lifecycle
/// read.
///
/// - `enable_service_mode` is the execution host the lifecycle follows
///   (`core_lifecycle/workflow.rs`);
/// - `core` picks the builtin transform table, the tun flavor, and the binary
///   that gets started (`nyanpasu-profile-transform/src/runtime_builder.rs`,
///   `RuntimeBuildPort::core_spec`);
/// - `enable_builtin_enhanced` gates that transform table.
///
/// Every other field of [`ChimeraAppConfig`] drives a peripheral owner or
/// nothing at all.
#[derive(Debug, PartialEq, serde::Serialize)]
struct ApplicationRuntimeInputs<'a> {
    enable_service_mode: bool,
    core: &'a chimera_config::application::ClashCore,
    enable_builtin_enhanced: bool,
}

/// The fields of `app` the runtime reads, and nothing else.
fn application_runtime_inputs(app: &ChimeraAppConfig) -> ApplicationRuntimeInputs<'_> {
    ApplicationRuntimeInputs {
        enable_service_mode: app.enable_service_mode,
        core: &app.core,
        enable_builtin_enhanced: app.enable_builtin_enhanced,
    }
}

/// Every clash-config field the runtime build or the control channel reads.
///
/// Enumerated from the real consumers rather than from a field whitelist:
/// `RuntimeBuilder::build` reads `overrides`, `enable_clash_fields`,
/// `expand_include_all`, `enable_tun_mode`, `tun` and `tun_stack`;
/// `SessionPortResolver::resolve` turns the four port strategies into the runtime bindings;
/// and `RuntimePreparation::prepare` turns the two channel fields into the
/// core's local-IPC settings.
///
/// Deliberately absent: `web_ui_list` is a dashboard list no build stage reads,
/// and `break_connection` decides whether connections are interrupted *after* an
/// apply, so it is a property of the action and not an input of the config that
/// gets built.
#[derive(Debug, PartialEq, serde::Serialize)]
struct ClashRuntimeInputs<'a> {
    overrides: &'a ClashGuardOverrides,
    enable_clash_fields: bool,
    expand_include_all: bool,
    enable_tun_mode: bool,
    tun: TunOverrides,
    tun_stack: TunStack,
    mips_congestion_controller:
        Option<chimera_config::clash::config::tun_stack::TunCongestionController>,
    mixed_port: &'a PortStrategy,
    socks_port: Option<&'a PortStrategy>,
    http_port: Option<&'a PortStrategy>,
    redir_port: Option<&'a PortStrategy>,
    tproxy_port: Option<&'a PortStrategy>,
    transparent_proxy: &'a chimera_config::clash::config::TransparentProxyConfig,
    external_controller: &'a ExternalControllerStrategy,
    control_channel: ClashControlChannel,
    disable_http_controller: bool,
    dns: Option<&'a DnsSettings>,
    hosts: Option<&'a IndexMap<String, OneOrMany>>,
}

/// The fields of `clash` the runtime reads, and nothing else.
fn clash_runtime_inputs(clash: &ClashConfig) -> ClashRuntimeInputs<'_> {
    ClashRuntimeInputs {
        overrides: &clash.overrides,
        enable_clash_fields: clash.enable_clash_fields,
        expand_include_all: clash.expand_include_all,
        enable_tun_mode: clash.enable_tun_mode,
        tun: clash.tun,
        tun_stack: clash.tun_stack,
        mips_congestion_controller: (clash.tun_stack == TunStack::Mips)
            .then_some(clash.tun_mips.congestion_controller)
            .flatten(),
        mixed_port: &clash.mixed_port,
        socks_port: clash.socks_port.as_ref(),
        http_port: clash.http_port.as_ref(),
        redir_port: clash.redir_port.as_ref(),
        tproxy_port: clash.tproxy_port.as_ref(),
        transparent_proxy: &clash.transparent_proxy,
        external_controller: &clash.external_controller,
        control_channel: clash.clash_control_channel,
        disable_http_controller: clash.clash_ipc_disable_http_controller,
        dns: clash.managed_dns(),
        hosts: clash.managed_hosts(),
    }
}

/// Whether the candidate selects a different current profile, which is what
/// interrupts connections on a profile change.
pub fn selection_changed(previous: &Profiles, candidate: &Profiles) -> bool {
    previous.current != candidate.current
}

/// The runtime target a candidate asks for, as a stable identity.
///
/// This is the same projection the classification above reads, and that is the
/// point: two candidates that ask the core for the same thing are the same
/// target, however much of the rest of the document moved between them. A
/// digest of the whole source document would make an unrelated save — a
/// dashboard URL, a language — look like a different target, which silently
/// resets a convergence budget and makes a re-save of an unconverged value look
/// like a document that changed nothing (roadmap §9.2, D11/V22).
///
/// `None` when the projection cannot be serialized. A target with no identity
/// is never treated as equal to another one.
pub fn application_target(candidate: &ChimeraAppConfig) -> Option<String> {
    digest(&application_runtime_inputs(candidate))
}

pub fn clash_target(candidate: &ClashConfig) -> Option<String> {
    digest(&clash_runtime_inputs(candidate))
}

/// The profiles projection is the runtime dependency closure of the candidate
/// document: the current selection, the global transforms, `valid`, and every
/// closure member with the definition the build would resolve for it.
///
/// Captured content is combined with this structural projection by the runtime inputs.
pub fn profiles_target(candidate: &Profiles) -> Option<String> {
    let closure = current_profile_closure(candidate);
    digest(&ProfilesRuntimeInputs {
        current: candidate.current.as_ref(),
        global_transforms: &candidate.global_transforms,
        valid: &candidate.valid,
        definitions: closure
            .iter()
            .map(|uid| (uid, definition_of(candidate, uid)))
            .collect(),
    })
}

#[derive(Debug, serde::Serialize)]
struct ProfilesRuntimeInputs<'a> {
    current: Option<&'a ProfileId>,
    global_transforms: &'a [ProfileId],
    valid: &'a [String],
    /// The current closure in order, each member with the definition the build
    /// would resolve for it. A member that dangles in this document resolves to
    /// nothing, exactly as the classification reads it.
    definitions: Vec<(&'a ProfileId, Option<&'a ProfileDefinition>)>,
}

fn digest<T: serde::Serialize>(inputs: &T) -> Option<String> {
    serde_json::to_vec(inputs)
        .ok()
        .map(|bytes| chimera_core_manager::control::payload_digest(&bytes))
}

fn definition_of<'a>(profiles: &'a Profiles, uid: &ProfileId) -> Option<&'a ProfileDefinition> {
    profiles.items.get(uid).map(|item| &item.definition)
}

/// The managed files the closure reads. A composition owns no file of its own,
/// so only its members contribute one.
fn closure_files<'a>(
    profiles: &'a Profiles,
    closure: &IndexSet<ProfileId>,
) -> HashSet<&'a ManagedProfilePath> {
    closure
        .iter()
        .filter_map(|uid| definition_of(profiles, uid)?.source())
        .map(|source| &source.materialized().file)
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn app_source_diff_reads_only_runtime_build_fields() {
        let mut before = ChimeraAppConfig::default();
        before.language = chimera_config::application::I18nLanguage::English;
        let mut after = before.clone();
        after.language = chimera_config::application::I18nLanguage::Japanese;
        assert!(!reaches_runtime(&before, &after, &[]));
        assert_eq!(application_target(&before), application_target(&after));
        after.enable_builtin_enhanced = !after.enable_builtin_enhanced;
        assert!(reaches_runtime(&before, &after, &[]));
        assert_ne!(application_target(&before), application_target(&after));
        after.enable_builtin_enhanced = before.enable_builtin_enhanced;
        after.enable_service_mode = !before.enable_service_mode;
        assert!(reaches_runtime(&before, &after, &[]));
    }

    #[test]
    fn selected_mips_options_impact_only_the_active_runtime_target() {
        use chimera_config::clash::config::tun_stack::{TunCongestionController, TunStack};
        let before = ClashConfig::default();
        let mut after = before.clone();
        after.tun_mips.congestion_controller = Some(TunCongestionController::Bbr3);
        assert!(!reaches_runtime(&before, &after, &[]));
        assert_eq!(clash_target(&before), clash_target(&after));

        let mut active = before;
        active.tun_stack = TunStack::Mips;
        let mut changed = active.clone();
        changed.tun_mips.congestion_controller = Some(TunCongestionController::Bbr3);
        assert!(reaches_runtime(&active, &changed, &[]));
        assert_ne!(clash_target(&active), clash_target(&changed));
    }

    #[test]
    fn clash_source_diff_does_not_rebuild_for_nonruntime_fields() {
        let before = ClashConfig::default();
        let mut after = before.clone();
        after.web_ui_list.push("https://dashboard.example/".into());
        assert!(!reaches_runtime(&before, &after, &[]));
        assert_eq!(clash_target(&before), clash_target(&after));
        after.enable_tun_mode = !after.enable_tun_mode;
        assert!(reaches_runtime(&before, &after, &[]));
        assert_ne!(clash_target(&before), clash_target(&after));
        after.enable_tun_mode = before.enable_tun_mode;
        assert!(!reaches_runtime(&before, &after, &[]));
    }

    #[test]
    fn dns_and_hosts_drafts_do_not_rebuild_until_takeover() {
        use chimera_config::clash::config::dns::{ControlSource, OneOrMany};

        let before = ClashConfig::default();
        let mut after = before.clone();
        after.dns.nameserver = vec!["https://example.test/dns-query".into()];
        after
            .hosts
            .insert("local.test".into(), OneOrMany::One("127.0.0.1".into()));
        assert!(!reaches_runtime(&before, &after, &[]));
        assert_eq!(clash_target(&before), clash_target(&after));

        after.dns_control = ControlSource::Chimera;
        assert!(reaches_runtime(&before, &after, &[]));
        assert_ne!(clash_target(&before), clash_target(&after));

        let mut edited = after.clone();
        edited.dns.enable = false;
        assert!(reaches_runtime(&after, &edited, &[]));

        edited = after.clone();
        edited.hosts_control = ControlSource::Chimera;
        assert!(reaches_runtime(&after, &edited, &[]));
        let mut new_hosts = edited.clone();
        new_hosts.hosts.clear();
        assert!(reaches_runtime(&edited, &new_hosts, &[]));
    }

    #[test]
    fn profiles_source_diff_tracks_selection_but_not_document_revision() {
        let before = Profiles::default();
        let mut after = before.clone();
        after.current = Some(ProfileId("active".into()));
        assert!(reaches_runtime(&before, &after, &[]));
        assert!(selection_changed(&before, &after));
    }
}

pub fn profile_inputs_changed(
    previous: &Profiles,
    candidate: &Profiles,
    touched: &[ManagedProfilePath],
) -> bool {
    let before = current_profile_closure(previous);
    let after = current_profile_closure(candidate);

    // `global_transforms` is compared as a list, not through the closure: the
    // closure is a set, and reordering the global transforms leaves it equal
    // while changing the order they run in.
    if previous.current != candidate.current
        || previous.global_transforms != candidate.global_transforms
        || previous.valid != candidate.valid
        || before != after
    {
        return true;
    }

    for uid in &after {
        match (definition_of(previous, uid), definition_of(candidate, uid)) {
            // Dangling in both documents: the executor resolves neither, so
            // nothing about the build changed.
            (None, None) => {}
            (Some(previous), Some(candidate)) => {
                let (Ok(previous), Ok(candidate)) =
                    (serde_json::to_vec(previous), serde_json::to_vec(candidate))
                else {
                    // Unable to prove them equal, so assume they are not.
                    return true;
                };
                if previous != candidate {
                    return true;
                }
            }
            // The member appeared in or disappeared from the closure.
            _ => return true,
        }
    }

    let closure_files = closure_files(candidate, &after);
    if touched.iter().any(|path| closure_files.contains(path)) {
        return true;
    }

    false
}

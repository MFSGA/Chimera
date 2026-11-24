//! Runtime input projections, target identities, and source-change detection.

use std::collections::{BTreeSet, HashSet};

use chimera_config::{
    application::{ChimeraAppConfig, ChimeraAppConfigPatch},
    clash::config::{
        ClashConfig, ClashConfigPatch, ClashControlChannel,
        clash_strategy::port::{ExternalControllerStrategy, PortStrategy},
        dns::{DnsSettings, OneOrMany},
        overrides::{ClashGuardOverrides, ClashGuardOverridesPatch, TunOverrides},
        tun_stack::TunStack,
    },
    profile::{ManagedProfilePath, ProfileDefinition, ProfileId, Profiles},
};
use indexmap::{IndexMap, IndexSet};

use crate::client::{
    effects::plan::{
        ApplicationEffect, ApplicationEffectInputs, ApplicationEffectPlan, EffectKind,
    },
    runtime_build_port::current_profile_closure,
};

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

/// How much of the running core a candidate forces to change.
///
/// The variants are ordered by how much they disturb, so the verdict of a
/// mutation that spans domains is the `max` of theirs: a host switch restarts
/// the core with the freshly built config, which subsumes replacing the binary,
/// which subsumes reconciling it.
///
/// A rebuild and a control-channel change share one variant on purpose. Both
/// come out of the same candidate and go to one reconcile, which picks reload
/// or restart below the application layer (roadmap §6.2); this is where the
/// classification stops copying [`runtime_apply_kind`], whose split exists so
/// the facade can call one of two legacy entry points.
///
/// [`runtime_apply_kind`]: crate::client::effects::plan::runtime_apply_kind
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
pub enum RuntimeImpact {
    /// No build input and no control-channel input moved. There is nothing
    /// critical to try, and the mutation is a plain source-config save.
    None,
    /// A runtime build input and/or a control-channel input moved, so the
    /// generated config the core runs is no longer the committed one.
    Reconcile,
    /// The selected core binary changed: the running instance is replaced
    /// rather than reloaded.
    CoreSwap,
    /// The execution host changed: the core moves between the local process and
    /// the service host.
    HostSwitch,
}

/// What the request asked of the current profile selection.
///
/// Carried rather than derived: a pure document diff cannot tell "the user did
/// not touch the selection" from "the user asked for the profile that is
/// already selected", and the second still owes a one-shot action.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub enum ActivationIntent {
    /// The request does not address the current selection.
    #[default]
    None,
    /// `SetCurrent` / `SetCurrentIfNone`: run this profile.
    // Requested by the profiles actor, which moves onto the participant in T6.
    #[allow(dead_code)]
    Activate(ProfileId),
    /// Clear the current selection.
    #[allow(dead_code)]
    Deactivate,
}

/// Digest of the bytes a request stages for one managed path.
///
/// Opaque here: the classification only needs it to travel with the path it
/// belongs to, and the producer (the profiles domain) owns the algorithm.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ContentDigest(String);

impl ContentDigest {
    pub fn new(value: impl Into<String>) -> Self {
        Self(value.into())
    }

    pub fn as_str(&self) -> &str {
        &self.0
    }
}

/// A staged resource, addressed by the mutation that created it.
///
/// A config version cannot address it: two mutations of the same version can
/// each stage their own bytes for one path, so the handle is bound to the
/// request's `OperationId` (roadmap §4.1).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct StagedResourceToken(String);

impl StagedResourceToken {
    pub fn new(value: impl Into<String>) -> Self {
        Self(value.into())
    }

    pub fn as_str(&self) -> &str {
        &self.0
    }
}

/// Managed content a request replaces.
///
/// File bytes live outside the profiles document, so comparing two documents
/// cannot see that the same path now holds different content. A subscription
/// refresh that rewrites the selected profile's file is exactly that case.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct TouchedContent {
    /// The managed path whose bytes this request replaces.
    pub path: ManagedProfilePath,
    /// Digest of the staged bytes, when the producer computed one.
    ///
    /// The classification does not read it: a touched path inside the current
    /// closure counts as changed either way, which over-counts an identical
    /// rewrite and never under-counts a real one. It travels with the path so
    /// the Try can pin the content set it built against.
    pub content_digest: Option<ContentDigest>,
    /// The staged bytes themselves, when the request pre-staged them.
    pub resource: Option<StagedResourceToken>,
}

/// The runtime-relevant fields one request explicitly named.
///
/// "Named" is struct-patch presence and never a value comparison: a `Some`
/// field of a patch DTO is a field the request addressed, and it stays
/// evidence of that when the value it carries is the one already committed.
/// That is the point — resubmitting an outstanding runtime field unchanged
/// *is* a request for that runtime, and no comparison of the two documents can
/// see it (R15, §9.2).
///
/// The converse matters as much. A request that named no runtime-relevant
/// field asked the runtime for nothing, even when the document it produces is
/// byte-identical to the committed one. Reading the answer out of document
/// equality instead would re-evaluate an outstanding target on an unrelated
/// save, and would skip a genuine resubmission that happened to move an
/// unrelated field alongside it. Manual re-evaluation does not spend the
/// automatic apply budget.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct RequestedRuntimeFields {
    named: bool,
}

impl RequestedRuntimeFields {
    /// A request that replaced the whole document rather than patching it.
    pub fn whole_document() -> Self {
        Self { named: true }
    }

    pub fn runtime() -> Self {
        Self { named: true }
    }

    /// The runtime-relevant fields an application-config patch carries.
    pub fn of_application(patch: &ChimeraAppConfigPatch) -> Self {
        Self {
            named: patch.enable_service_mode.is_some()
                || patch.core.is_some()
                || patch.enable_builtin_enhanced.is_some(),
        }
    }

    /// The runtime-relevant fields a clash-config patch carries.
    pub fn of_clash(patch: &ClashConfigPatch) -> Self {
        use struct_patch::Status as _;
        Self {
            named: !patch.overrides.is_empty()
                || patch.enable_clash_fields.is_some()
                || patch.expand_include_all.is_some()
                || patch.enable_tun_mode.is_some()
                || !patch.tun.is_empty()
                || patch.tun_stack.is_some()
                || !patch.tun_mips.is_empty()
                || patch.mixed_port.kind.is_some()
                || patch.mixed_port.start_port.is_some()
                || patch.socks_port.is_some()
                || patch.http_port.is_some()
                || patch.redir_port.is_some()
                || patch.tproxy_port.is_some()
                || !patch.transparent_proxy.is_empty()
                || patch.external_controller.host.is_some()
                || patch.external_controller.port.kind.is_some()
                || patch.external_controller.port.start_port.is_some()
                || patch.clash_control_channel.is_some()
                || patch.clash_ipc_disable_http_controller.is_some()
                || patch.dns_control.is_some()
                || patch.hosts_control.is_some(),
        }
    }

    /// The clash overrides are patched through an entry point of their own, so
    /// a request that carries one names runtime exactly
    /// when that patch addresses a field.
    pub fn of_clash_overrides(patch: &ClashGuardOverridesPatch) -> Self {
        use struct_patch::Status as _;
        Self {
            named: !patch.is_empty(),
        }
    }

    /// Whether the request named a runtime-relevant field at all.
    pub fn names_any(&self) -> bool {
        self.named
    }
}

/// Whether applying an application-config patch must enter the core lifecycle.
///
/// Core selection and builtin-enhancement changes always affect the generated
/// runtime config. A Service Mode change does so only when the service host is
/// connected; disconnected host preferences remain a source-config save.
pub fn application_patch_requires_reconcile(
    patch: &ChimeraAppConfigPatch,
    service_connected: bool,
) -> bool {
    RequestedRuntimeFields::of_application(patch).names_any()
        && (patch.core.is_some() || patch.enable_builtin_enhanced.is_some() || service_connected)
}

/// Typed request intent that the committed candidate cannot express.
///
/// A pure diff of the source config loses the difference between "the user did
/// not ask" and "the user asked for the value that is already stored". The
/// second still carries a command, and dropping it would silently change
/// behaviour that users depend on (roadmap C7/D6).
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct MutationHints {
    pub requested_owners: Vec<EffectKind>,
    /// The request carried a `mode` field, whether or not the value moved.
    ///
    /// This is the repeated-mode command semantics: re-submitting the current
    /// mode is a request for the one-shot connection interruption, not for a
    /// rebuild. It therefore feeds the action, never [`RuntimeImpact`].
    pub mode_requested: bool,
    /// What the request asked of the current profile selection.
    pub activation: ActivationIntent,
    /// Managed content this request replaces.
    pub touched: Vec<TouchedContent>,
    /// Private candidate bytes, owned by this attempt until its source settles.
    pub staged_content: std::collections::BTreeMap<String, String>,
    /// Which runtime-relevant fields the request explicitly named.
    pub requested: RequestedRuntimeFields,
}

impl MutationHints {
    /// Whether this request explicitly asked the runtime for something.
    ///
    /// This is the third re-evaluation trigger of §9.2: while a committed
    /// document is still not applied, a request that names one of its runtime
    /// fields again owes it a fresh attempt even though the diff is empty,
    /// and a request that names none of them must leave that target and its
    /// convergence budget exactly as it found them (R15).
    ///
    /// The profiles selection is read off [`MutationHints::activation`], which
    /// is the typed form a request expresses it in: "run this profile" names
    /// the field the build reads whether or not the profile is already the
    /// current one. Replaced managed bytes are deliberately *not* read here —
    /// content inside the current closure already moves
    /// [`classify_profiles`]'s own verdict, and content outside it is not a
    /// runtime input at all.
    pub fn names_runtime_field(&self) -> bool {
        self.requested.names_any() || self.activation != ActivationIntent::None
    }
}

pub fn classify_application(
    previous: &ChimeraAppConfig,
    candidate: &ChimeraAppConfig,
) -> RuntimeImpact {
    // Widest verdict first: a host switch restarts the core with the freshly
    // built config, so it subsumes both of the others.
    if previous.enable_service_mode != candidate.enable_service_mode {
        RuntimeImpact::HostSwitch
    } else if previous.core != candidate.core {
        RuntimeImpact::CoreSwap
    } else if reaches_runtime(previous, candidate, &[]) {
        RuntimeImpact::Reconcile
    } else {
        RuntimeImpact::None
    }
}

/// Runtime verdict for a clash-config candidate, overrides included.
///
/// One verdict for build inputs and control-channel inputs alike: they are
/// decided by the same candidate and settled by one reconcile (roadmap §6.2).
pub fn classify_clash(previous: &ClashConfig, candidate: &ClashConfig) -> RuntimeImpact {
    if !reaches_runtime(previous, candidate, &[]) {
        RuntimeImpact::None
    } else {
        RuntimeImpact::Reconcile
    }
}

/// Runtime verdict for a profiles candidate.
///
/// Three sources decide it, and all three are needed: the dependency closure of
/// the current selection, the definitions inside that closure, and the content
/// the request touched. Comparing `ProfileItem`s alone would miss a file whose
/// bytes changed under an unchanged path, which is what a subscription refresh
/// does to the running profile.
///
/// Over-counting is deliberate where it is cheap: a touched closure path counts
/// as changed without consulting its digest, and a definition that only carries
/// a new materialization timestamp counts as changed too. Missing a real
/// content change of a current dependency is the failure that is not allowed.
///
/// [`MutationHints::activation`] is not read here. Re-selecting the profile that
/// is already current leaves the built config identical, so it owes a one-shot
/// action rather than a rebuild, exactly as it does today.
pub fn classify_profiles(
    previous: &Profiles,
    candidate: &Profiles,
    hints: &MutationHints,
) -> RuntimeImpact {
    let touched = hints
        .touched
        .iter()
        .map(|entry| entry.path.clone())
        .collect::<Vec<_>>();
    if reaches_runtime(previous, candidate, &touched) {
        RuntimeImpact::Reconcile
    } else {
        RuntimeImpact::None
    }
}

/// The peripheral owners whose *inputs* a candidate moves.
///
/// Only an owner listed here may be handed a new desired target (roadmap §9.1).
/// A language change must not raise the system proxy's target generation: doing
/// so would let any unrelated save reset that owner's retry budget.
///
/// This is not "the owner has converged". An owner absent here can still be
/// holding a target it never managed to apply. That fact lives in the owner's
/// own `EffectStatus`, where `applied_revision` trails `desired_revision`, and
/// the scheduler combines the two — it is never folded into this projection,
/// because then "nothing changed for you" and "you are behind" would be one
/// indistinguishable signal.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct ChangedOwnerInputs(BTreeSet<EffectKind>);

impl ChangedOwnerInputs {
    /// Reuses the effect plan's struct-patch diff, so an owner appears here on
    /// exactly the inputs that would produce its effect.
    pub fn diff(previous: &ApplicationEffectInputs, candidate: &ApplicationEffectInputs) -> Self {
        Self(
            ApplicationEffectPlan::diff(previous, candidate)
                .effects()
                .iter()
                .map(ApplicationEffect::kind)
                .collect(),
        )
    }

    pub fn is_empty(&self) -> bool {
        self.0.is_empty()
    }

    /// Read by the post-commit dispatch that hands each owner its new target
    /// (T7); the workflow itself only asks whether any owner moved.
    #[allow(dead_code)]
    pub fn contains(&self, kind: EffectKind) -> bool {
        self.0.contains(&kind)
    }

    #[allow(dead_code)]
    pub fn kinds(&self) -> &BTreeSet<EffectKind> {
        &self.0
    }
}

#[cfg(test)]
mod request_tests {
    use super::*;

    #[test]
    fn empty_clash_patch_does_not_name_runtime_fields() {
        use struct_patch::Patch;

        let patch = ClashConfig::new_empty_patch();
        assert!(!RequestedRuntimeFields::of_clash(&patch).names_any());
    }

    #[test]
    fn mips_options_are_runtime_inputs_only_for_the_active_stack() {
        use chimera_config::clash::config::tun_stack::{TunCongestionController, TunStack};
        let before = ClashConfig::default();
        let mut inactive = before.clone();
        inactive.tun_mips.congestion_controller = Some(TunCongestionController::Bbr3);
        assert_eq!(classify_clash(&before, &inactive), RuntimeImpact::None);

        let mut active = before;
        active.tun_stack = TunStack::Mips;
        let mut updated = active.clone();
        updated.tun_mips.congestion_controller = Some(TunCongestionController::Bbr3);
        assert_eq!(classify_clash(&active, &updated), RuntimeImpact::Reconcile);
        let patch: ClashConfigPatch = serde_json::from_value(serde_json::json!({
            "tun_mips": { "congestion_controller": "bbr3" }
        }))
        .unwrap();
        assert!(RequestedRuntimeFields::of_clash(&patch).names_any());
    }

    #[test]
    fn expand_include_all_flag_rebuilds_runtime_and_names_patch() {
        let before = ClashConfig::default();
        let mut after = before.clone();
        after.expand_include_all = false;
        assert_eq!(classify_clash(&before, &after), RuntimeImpact::Reconcile);
        assert_ne!(clash_target(&before), clash_target(&after));
        let patch: ClashConfigPatch = serde_yaml_ng::from_str(
            "expand_include_all: false
",
        )
        .expect("include-all setting patch must deserialize");
        assert!(RequestedRuntimeFields::of_clash(&patch).names_any());
    }

    #[test]
    fn tun_auto_route_changes_the_runtime_target() {
        let before = ClashConfig::default();
        let mut after = before.clone();
        after.tun.auto_route =
            chimera_config::clash::config::overrides::ManageableField::Managed(false);

        assert_eq!(classify_clash(&before, &after), RuntimeImpact::Reconcile);
        assert_ne!(clash_target(&before), clash_target(&after));
    }

    #[test]
    fn nested_tun_auto_route_patch_names_runtime_fields() {
        let patch: ClashConfigPatch =
            serde_yaml_ng::from_str("tun:\n  auto-route:\n    kind: managed\n    value: true\n")
                .expect("nested TUN auto-route patch must deserialize");

        assert!(RequestedRuntimeFields::of_clash(&patch).names_any());
    }

    #[test]
    fn empty_nested_tun_patch_does_not_name_runtime_fields() {
        let patch: ClashConfigPatch =
            serde_yaml_ng::from_str("tun: {}\n").expect("empty nested TUN patch must deserialize");

        assert!(!RequestedRuntimeFields::of_clash(&patch).names_any());
    }

    #[test]
    fn nested_manageable_override_patch_names_runtime_fields() {
        let patch: ClashConfigPatch =
            serde_yaml_ng::from_str("overrides:\n  unified-delay:\n    kind: unmanaged\n")
                .expect("nested manageable override patch must deserialize");

        assert!(RequestedRuntimeFields::of_clash(&patch).names_any());
    }
}

#[cfg(test)]
mod ref_profile_tests {
    use super::*;
    use chimera_config::profile::{
        ConfigDefinition, ProfileMetadata, ProfileSource, TransformDefinition,
    };
    use chimera_profile_transform::test_support;
    /// current -> `sel`; `sel` carries `scoped` as a scoped transform; `global`
    /// is a document-level transform; `spare` is reachable from neither.
    fn profiles() -> Profiles {
        let mut profiles = Profiles::default();
        profiles.append_item(test_support::file_config("sel", "sel.yaml", &["scoped"]));
        profiles.append_item(test_support::overlay("scoped", "scoped.yaml"));
        profiles.append_item(test_support::overlay("global", "global.yaml"));
        profiles.append_item(test_support::file_config("spare", "spare.yaml", &[]));
        profiles.current = Some(ProfileId("sel".into()));
        profiles.global_transforms = vec![ProfileId("global".into())];
        profiles
    }

    fn touching(file: &str) -> Vec<ManagedProfilePath> {
        vec![ManagedProfilePath::new(file).unwrap()]
    }

    fn replace_overlay_source(profiles: &mut Profiles, uid: &str, file: &str) {
        let item = profiles
            .items
            .get_mut(&ProfileId(uid.into()))
            .expect("item exists");
        item.definition = ProfileDefinition::Transform {
            transform: TransformDefinition::Overlay(chimera_config::profile::OverlayTransform {
                source: ProfileSource::Local {
                    binding: chimera_config::profile::LocalBinding::Managed {
                        materialized: test_support::managed(file),
                    },
                },
            }),
        };
    }

    #[test]
    fn a_definition_change_inside_the_current_closure_affects_the_runtime() {
        let previous = profiles();
        let mut candidate = profiles();
        replace_overlay_source(&mut candidate, "scoped", "scoped-v2.yaml");

        assert!(reaches_runtime(&previous, &candidate, &[]));
        assert_eq!(
            classify_profiles(&previous, &candidate, &MutationHints::default()),
            RuntimeImpact::Reconcile
        );
    }

    #[test]
    fn a_definition_change_outside_the_current_closure_does_not_affect_the_runtime() {
        let previous = profiles();
        let mut candidate = profiles();
        let item = candidate
            .items
            .get_mut(&ProfileId("spare".into()))
            .expect("spare exists");
        item.definition = ProfileDefinition::Config {
            config: ConfigDefinition::File(chimera_config::profile::FileConfig {
                source: ProfileSource::Local {
                    binding: chimera_config::profile::LocalBinding::Managed {
                        materialized: test_support::managed("spare-v2.yaml"),
                    },
                },
                transforms: Vec::new(),
            }),
        };

        assert!(!reaches_runtime(&previous, &candidate, &[]));
        assert_eq!(
            classify_profiles(&previous, &candidate, &MutationHints::default()),
            RuntimeImpact::None
        );
    }

    #[test]
    fn touched_content_on_a_closure_path_affects_the_runtime_without_a_document_change() {
        let profiles = profiles();

        // The document is byte-identical: only the file behind `scoped` changed.
        assert!(reaches_runtime(
            &profiles,
            &profiles,
            &touching("scoped.yaml")
        ));
        assert!(reaches_runtime(&profiles, &profiles, &touching("sel.yaml")));
        assert!(reaches_runtime(
            &profiles,
            &profiles,
            &touching("global.yaml")
        ));
    }

    #[test]
    fn touched_content_outside_the_closure_does_not_affect_the_runtime() {
        let profiles = profiles();
        assert!(!(reaches_runtime(&profiles, &profiles, &touching("spare.yaml"))));
    }

    #[test]
    fn a_current_selection_change_affects_the_runtime() {
        let previous = profiles();
        let mut candidate = profiles();
        candidate.current = Some(ProfileId("spare".into()));

        assert!(reaches_runtime(&previous, &candidate, &[]));
        assert_eq!(
            classify_profiles(&previous, &candidate, &MutationHints::default()),
            RuntimeImpact::Reconcile
        );
    }

    /// The closure is a set, so reordering the global transforms leaves it
    /// equal while changing the order they run in.
    #[test]
    fn reordering_the_global_transforms_affects_the_runtime() {
        let mut previous = profiles();
        previous.append_item(test_support::overlay("global2", "global2.yaml"));
        previous.global_transforms = vec![ProfileId("global".into()), ProfileId("global2".into())];
        let mut candidate = previous.clone();
        candidate.global_transforms.reverse();

        assert_eq!(
            current_profile_closure(&previous),
            current_profile_closure(&candidate),
            "the closure must be set-equal for this test to mean anything"
        );
        assert!(reaches_runtime(&previous, &candidate, &[]));
        assert_eq!(
            classify_profiles(&previous, &candidate, &MutationHints::default()),
            RuntimeImpact::Reconcile
        );
    }

    #[test]
    fn a_retained_field_change_affects_the_runtime() {
        let previous = profiles();
        let mut candidate = profiles();
        candidate.valid.push("new-clash-field".to_owned());

        assert!(reaches_runtime(&previous, &candidate, &[]));
        assert_eq!(
            classify_profiles(&previous, &candidate, &MutationHints::default()),
            RuntimeImpact::Reconcile
        );
    }

    /// Metadata is a plain save: no build stage reads it, not even for the
    /// profile that is running.
    #[test]
    fn profile_metadata_has_no_runtime_impact() {
        let previous = profiles();
        let mut candidate = profiles();
        candidate
            .items
            .get_mut(&ProfileId("sel".into()))
            .expect("sel exists")
            .metadata = ProfileMetadata {
            name: "renamed".to_owned(),
            desc: Some("a description".to_owned()),
            custom_name: true,
        };

        assert!(!reaches_runtime(&previous, &candidate, &[]));
        assert_eq!(
            classify_profiles(&previous, &candidate, &MutationHints::default()),
            RuntimeImpact::None
        );
    }

    /// Folders and the display order only arrange the list: moving the running
    /// profile into a folder, or reordering the items, builds the same config.
    #[test]
    fn folders_and_display_order_have_no_runtime_impact() {
        use chimera_config::profile::{FolderId, ProfileFolder, ProfileLayout};

        let previous = profiles();
        let mut candidate = profiles();
        candidate
            .apply_layout(ProfileLayout {
                folders: vec![ProfileFolder {
                    id: FolderId("f1".into()),
                    name: "Work".to_owned(),
                    members: vec![ProfileId("spare".into()), ProfileId("sel".into())],
                }],
                ungrouped: vec![],
            })
            .expect("the layout places every config");
        assert_ne!(
            previous.items.keys().collect::<Vec<_>>(),
            candidate.items.keys().collect::<Vec<_>>(),
            "the layout reordered the items"
        );

        assert!(!reaches_runtime(&previous, &candidate, &[]));
        assert_eq!(
            classify_profiles(&previous, &candidate, &MutationHints::default()),
            RuntimeImpact::None
        );
    }

    /// A proxy group arrangement is read by the sidebar and the tray only: it
    /// never reaches the config the core runs.
    #[test]
    fn a_proxy_group_layout_has_no_runtime_impact() {
        use chimera_config::profile::{FolderId, ProxyGroupFolder, ProxyGroupLayout};

        let previous = profiles();
        let mut candidate = profiles();
        candidate.proxy_group_layouts.insert(
            ProfileId("sel".into()),
            ProxyGroupLayout {
                folders: vec![ProxyGroupFolder {
                    id: FolderId("f1".into()),
                    name: "Streaming".to_owned(),
                    members: vec!["Netflix".to_owned()],
                }],
                ungrouped: vec!["Proxy".to_owned()],
            },
        );

        assert!(!reaches_runtime(&previous, &candidate, &[]));
        assert_eq!(
            classify_profiles(&previous, &candidate, &MutationHints::default()),
            RuntimeImpact::None
        );
    }

    #[test]
    fn an_unchanged_profiles_document_has_no_runtime_impact() {
        let profiles = profiles();
        assert!(!(reaches_runtime(&profiles, &profiles, &[])));
    }

    #[test]
    fn touched_request_and_source_admission_agree() {
        let profiles = profiles();
        for (path, expected) in [
            ("sel.yaml", RuntimeImpact::Reconcile),
            ("scoped.yaml", RuntimeImpact::Reconcile),
            ("global.yaml", RuntimeImpact::Reconcile),
            ("spare.yaml", RuntimeImpact::None),
        ] {
            let hints = MutationHints {
                touched: vec![TouchedContent {
                    path: ManagedProfilePath::new(path).unwrap(),
                    content_digest: None,
                    resource: None,
                }],
                ..Default::default()
            };
            assert_eq!(classify_profiles(&profiles, &profiles, &hints), expected);
        }
    }

    #[test]
    fn managed_dns_and_hosts_have_one_verdict_and_target() {
        use chimera_config::clash::config::dns::ControlSource;
        let previous = ClashConfig {
            dns_control: ControlSource::Chimera,
            hosts_control: ControlSource::Chimera,
            ..ClashConfig::default()
        };
        let mut dns = previous.clone();
        dns.dns.enable = !previous.dns.enable;
        let mut hosts = previous.clone();
        hosts
            .hosts
            .insert("test.lan".into(), OneOrMany::One("192.0.2.1".into()));
        for candidate in [dns, hosts] {
            assert!(reaches_runtime(&previous, &candidate, &[]));
            assert_eq!(
                classify_clash(&previous, &candidate),
                RuntimeImpact::Reconcile
            );
            assert_ne!(clash_target(&previous), clash_target(&candidate));
        }
        let previous = ClashConfig::default();
        let mut draft = previous.clone();
        draft.dns.enable = !previous.dns.enable;
        draft
            .hosts
            .insert("test.lan".into(), OneOrMany::One("192.0.2.1".into()));
        assert!(!reaches_runtime(&previous, &draft, &[]));
        assert_eq!(classify_clash(&previous, &draft), RuntimeImpact::None);
        assert_eq!(clash_target(&previous), clash_target(&draft));
    }
}

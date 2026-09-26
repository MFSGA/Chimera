//! Pure impact classification for one configuration mutation.
//!
//! Two questions that must not be collapsed into one:
//!
//! - does the candidate move a *runtime build input*, so the critical part of
//!   the mutation has to build, check and try it ([`RuntimeImpact`]);
//! - which *peripheral owners* were handed new inputs, so only those may be
//!   given a new desired target ([`ChangedOwnerInputs`]).
//!
//! Every input is a parameter and every output is data: nothing here reads
//! state, spawns work, or touches Tauri. The workflow composes the results.

use std::collections::{BTreeSet, HashSet};

use chimera_config::{
    application::{ChimeraAppConfig, ChimeraAppConfigPatch},
    clash::config::{
        ClashConfig, ClashConfigPatch, ClashControlChannel,
        clash_strategy::port::{ExternalControllerStrategy, PortStrategy},
        overrides::{ClashGuardOverrides, ClashGuardOverridesPatch},
        tun_stack::TunStack,
    },
    profile::{ManagedProfilePath, ProfileDefinition, ProfileId, Profiles},
};
use indexmap::IndexSet;

use crate::{
    client::effects::plan::{
        ApplicationEffect, ApplicationEffectInputs, ApplicationEffectPlan, EffectKind,
    },
    state::profiles::ProfilesActor,
};

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
pub(crate) enum RuntimeImpact {
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
pub(crate) enum ActivationIntent {
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
pub(crate) struct ContentDigest(String);

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
pub(crate) struct StagedResourceToken(String);

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
pub(crate) struct TouchedContent {
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
pub(crate) struct RequestedRuntimeFields {
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
        Self {
            named: patch.overrides.is_some()
                || patch.enable_clash_fields.is_some()
                || patch.enable_tun_mode.is_some()
                || patch.tun_stack.is_some()
                || patch.mixed_port.is_some()
                || patch.socks_port.is_some()
                || patch.http_port.is_some()
                || patch.external_controller.is_some()
                || patch.clash_control_channel.is_some()
                || patch.clash_ipc_disable_http_controller.is_some(),
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

/// Typed request intent that the committed candidate cannot express.
///
/// A pure diff of the source config loses the difference between "the user did
/// not ask" and "the user asked for the value that is already stored". The
/// second still carries a command, and dropping it would silently change
/// behaviour that users depend on (roadmap C7/D6).
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub(crate) struct MutationHints {
    pub requested_owners: Vec<crate::client::effects::plan::EffectKind>,
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

/// Which application-config fields the runtime build and the core lifecycle
/// read.
///
/// - `enable_service_mode` is the execution host the lifecycle follows
///   (`core_lifecycle/workflow.rs`);
/// - `core` picks the builtin transform table, the tun flavor, and the binary
///   that gets started (`enhance/runtime_builder.rs`, `RuntimeBuildPort::core_spec`);
/// - `enable_builtin_enhanced` gates that transform table.
///
/// Every other field of [`ChimeraAppConfig`] drives a peripheral owner or
/// nothing at all, which is what [`ChangedOwnerInputs`] reports instead.
#[derive(Debug, PartialEq, serde::Serialize)]
struct ApplicationRuntimeInputs<'a> {
    enable_service_mode: bool,
    core: &'a chimera_config::application::ClashCore,
    enable_builtin_enhanced: bool,
}

impl<'a> ApplicationRuntimeInputs<'a> {
    fn of(app: &'a ChimeraAppConfig) -> Self {
        Self {
            enable_service_mode: app.enable_service_mode,
            core: &app.core,
            enable_builtin_enhanced: app.enable_builtin_enhanced,
        }
    }
}

pub(crate) fn classify_application(
    previous: &ChimeraAppConfig,
    candidate: &ChimeraAppConfig,
) -> RuntimeImpact {
    // Widest verdict first: a host switch restarts the core with the freshly
    // built config, so it subsumes both of the others.
    if previous.enable_service_mode != candidate.enable_service_mode {
        RuntimeImpact::HostSwitch
    } else if previous.core != candidate.core {
        RuntimeImpact::CoreSwap
    } else if previous.enable_builtin_enhanced != candidate.enable_builtin_enhanced {
        RuntimeImpact::Reconcile
    } else {
        RuntimeImpact::None
    }
}

/// Every clash-config field the runtime build or the control channel reads.
///
/// Enumerated from the real consumers rather than from a field whitelist:
/// `RuntimeBuilder::build` reads `overrides`, `enable_clash_fields`,
/// `enable_tun_mode` and `tun_stack`; `SessionPortResolver::resolve` turns the
/// four port strategies into the bindings written into the generated config;
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
    enable_tun_mode: bool,
    tun_stack: TunStack,
    mixed_port: &'a PortStrategy,
    socks_port: Option<&'a PortStrategy>,
    http_port: Option<&'a PortStrategy>,
    external_controller: &'a ExternalControllerStrategy,
    control_channel: ClashControlChannel,
    disable_http_controller: bool,
}

impl<'a> ClashRuntimeInputs<'a> {
    fn of(clash: &'a ClashConfig) -> Self {
        Self {
            overrides: &clash.overrides,
            enable_clash_fields: clash.enable_clash_fields,
            enable_tun_mode: clash.enable_tun_mode,
            tun_stack: clash.tun_stack,
            mixed_port: &clash.mixed_port,
            socks_port: clash.socks_port.as_ref(),
            http_port: clash.http_port.as_ref(),
            external_controller: &clash.external_controller,
            control_channel: clash.clash_control_channel,
            disable_http_controller: clash.clash_ipc_disable_http_controller,
        }
    }
}

/// Runtime verdict for a clash-config candidate, overrides included.
///
/// One verdict for build inputs and control-channel inputs alike: they are
/// decided by the same candidate and settled by one reconcile (roadmap §6.2).
pub(crate) fn classify_clash(previous: &ClashConfig, candidate: &ClashConfig) -> RuntimeImpact {
    if ClashRuntimeInputs::of(previous) == ClashRuntimeInputs::of(candidate) {
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
pub(crate) fn classify_profiles(
    previous: &Profiles,
    candidate: &Profiles,
    hints: &MutationHints,
) -> RuntimeImpact {
    let before = ProfilesActor::current_closure(previous);
    let after = ProfilesActor::current_closure(candidate);

    // `global_transforms` is compared as a list, not through the closure: the
    // closure is a set, and reordering the global transforms leaves it equal
    // while changing the order they run in.
    if previous.current != candidate.current
        || previous.global_transforms != candidate.global_transforms
        || previous.valid != candidate.valid
        || before != after
    {
        return RuntimeImpact::Reconcile;
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
                    return RuntimeImpact::Reconcile;
                };
                if previous != candidate {
                    return RuntimeImpact::Reconcile;
                }
            }
            // The member appeared in or disappeared from the closure.
            _ => return RuntimeImpact::Reconcile,
        }
    }

    let closure_files = closure_files(candidate, &after);
    if hints
        .touched
        .iter()
        .any(|touched| closure_files.contains(&touched.path))
    {
        return RuntimeImpact::Reconcile;
    }

    RuntimeImpact::None
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
pub(crate) fn application_target(candidate: &ChimeraAppConfig) -> Option<String> {
    digest(&ApplicationRuntimeInputs::of(candidate))
}

pub(crate) fn clash_target(candidate: &ClashConfig) -> Option<String> {
    digest(&ClashRuntimeInputs::of(candidate))
}

/// The profiles projection is the runtime dependency closure of the candidate
/// document: the current selection, the global transforms, `valid`, and every
/// closure member with the definition the build would resolve for it.
///
/// [`MutationHints::touched`] is deliberately not part of it. Those hints
/// describe what *this request* changes, and identity has to be a property of
/// the desired runtime rather than of the request that asked for it: a deferred
/// content update hashing `touched = [(path, digest)]` and the later save of the
/// same committed document — which carries no hints at all — ask the core for
/// exactly the same thing, and a key that moved between them would lose the
/// outstanding target and open a fresh convergence budget for it (§9.2,
/// D11/V22, R16).
///
/// This is the structural projection. `RuntimeInputs` combines it with the
/// captured file contents so content changes remain distinct even when a
/// producer has not advanced a materialization stamp.
pub(crate) fn profiles_target(candidate: &Profiles) -> Option<String> {
    let closure = ProfilesActor::current_closure(candidate);
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
        .map(|bytes| crate::state::profiles::payload_digest(&bytes))
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
pub(crate) struct ChangedOwnerInputs(BTreeSet<EffectKind>);

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

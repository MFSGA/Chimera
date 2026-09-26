//! Pure command policy and the deferral decision.
//!
//! Two static questions, both answered from typed data and never from a
//! parameter the caller chose: what is this command allowed to do when its
//! critical part does not apply ([`CommandPolicy`]), and given what the Try
//! actually reported, may the commit still go ahead ([`disposition`]).

use super::impact::{ChangedOwnerInputs, RuntimeImpact};

/// What a command may do when the runtime part of it cannot be applied.
///
/// Assigned by [`policy_for`] from the command class and the classified impact.
/// The frontend cannot pick it: a request must not be able to declare itself
/// exempt from confirming that a core switch took effect.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum CommandPolicy {
    /// The target has to be confirmed in effect before the source config is
    /// committed. A refused elevation is a plain rejection, not a reason to
    /// keep prompting in the background.
    MustApply,
    /// The desired value may be committed unapplied, but only when every
    /// condition in [`disposition`] holds.
    AllowDeferredWhenSafe,
    /// Nothing to apply and no owner to notify: validate and commit.
    SaveOnly,
    /// Nothing critical to apply, but peripheral owners have a new target.
    /// Commit on validation and let them reconcile afterwards.
    SaveThenNotify,
    /// The user stopped the core. Save the checked target, start nothing, and
    /// run no retry loop against that intent.
    SavedInactive,
}

impl CommandPolicy {
    /// Whether this command may commit a desired value it could not apply.
    ///
    /// Only one policy may. `MustApply` refuses by definition; under the other
    /// three no critical Try was owed in the first place, so a failure from one
    /// is a contradiction rather than a licence to defer.
    pub fn allows_deferral(self) -> bool {
        matches!(self, Self::AllowDeferredWhenSafe)
    }
}

/// What kind of command produced the candidate.
///
/// The split is the one the policy table needs, and it follows the command, not
/// its diff: asking for a specific core, host or profile to be running is a
/// different promise from saving a value.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum CommandClass {
    /// `select_core`, `set_execution_host`, and profile activation or
    /// deactivation: the user named something that has to be running.
    ExplicitSwitch,
    /// Every other source-config write: clash and overrides parameters, managed
    /// content updates, profile bookkeeping, application settings.
    // Chosen by the domain actors, which move onto the participant in T6.
    #[allow(dead_code)]
    Save,
}

/// Whether the user wants a core running at all.
///
/// A stopped core is an intent, not a failure: an ordinary save must not start
/// one behind the user's back just because it has something to apply.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum CoreRunIntent {
    Running,
    StoppedByUser,
}

/// The static policy for one candidate.
pub(crate) fn policy_for(
    class: CommandClass,
    impact: RuntimeImpact,
    peripheral: &ChangedOwnerInputs,
    intent: CoreRunIntent,
) -> CommandPolicy {
    // An explicit switch owes a target whatever its diff says. Re-selecting the
    // profile that is already current, or re-picking the running core, produces
    // no impact at all, and treating that as a plain save would drop the
    // confirmation the command promised: an empty diff is not evidence that the
    // target ever converged.
    let critical = class == CommandClass::ExplicitSwitch || impact != RuntimeImpact::None;

    if !critical {
        // Nothing critical to apply, so the only question left is whether any
        // peripheral owner was handed a target to reconcile after the commit.
        return if peripheral.is_empty() {
            CommandPolicy::SaveOnly
        } else {
            CommandPolicy::SaveThenNotify
        };
    }

    if intent == CoreRunIntent::StoppedByUser {
        return CommandPolicy::SavedInactive;
    }

    match (class, impact) {
        // Decided by the command: it named something that has to be running.
        (CommandClass::ExplicitSwitch, _) => CommandPolicy::MustApply,
        // Which binary or host the user's traffic runs through is not something
        // to defer: committing it while the old one keeps running would leave
        // the app claiming a core it never started.
        (_, RuntimeImpact::CoreSwap | RuntimeImpact::HostSwitch) => CommandPolicy::MustApply,
        // A `Save` with no impact returned above, so this one really moved a
        // build input.
        (CommandClass::Save, _) => CommandPolicy::AllowDeferredWhenSafe,
    }
}

/// How a critical Try ended, as typed by the adapter that could observe it.
///
/// `CoreErrorKind::Internal`, wait timeouts and lost receipts belong in
/// [`Unknown`], never in [`Transient`]: none of them says what the core did.
///
/// [`Unknown`]: TryCauseKind::Unknown
/// [`Transient`]: TryCauseKind::Transient
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum TryCauseKind {
    /// The outcome could not be observed. Never enters the retryable branch.
    Unknown,
    /// A check or the core itself rejected this candidate. The same desired
    /// value cannot succeed later.
    Deterministic,
    /// Observed, finished, and permanent for now: a refused elevation, a
    /// missing binary, an unsupported platform.
    Permanent,
    /// Observed, finished, and typed transient by the adapter that watched it.
    Transient,
}

/// Whether the application knows what is running after the failure.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum BaselineAvailability {
    /// A known-good runtime is still running, or the core is confirmed stopped.
    Known,
    /// The real state could not be confirmed. A backend that answers
    /// "rolled back" has described its own request, not the running config.
    Unconfirmed,
}

/// The typed facts a deferral decision may read.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct TryFailureFacts {
    pub cause: TryCauseKind,
    pub baseline: BaselineAvailability,
    pub policy: CommandPolicy,
    /// The candidate holds an item a deterministic check rejected, so no later
    /// attempt at it can converge.
    pub candidate_has_invalid_item: bool,
}

/// What the commit decision may be after a critical Try failed.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum FailureDisposition {
    /// Every condition holds: commit the new desired value and mark the target
    /// deferred. What is applied stays at the baseline.
    Deferrable,
    /// Do not commit. The baseline is known, so the mutation is simply refused.
    Reject,
    /// The real state is unknown, so no commit decision can be derived from it
    /// at all. The operation has to be resolved before anything else runs.
    RecoveryRequired,
}

/// The whole conjunction, in one place.
///
/// Deferring means committing a value the core is not running, so it takes all
/// the typed failure, safe baseline, command policy and valid candidate.
pub(crate) fn disposition(facts: &TryFailureFacts) -> FailureDisposition {
    // An unobserved outcome decides nothing: the candidate may already run.
    if facts.cause == TryCauseKind::Unknown {
        return FailureDisposition::RecoveryRequired;
    }
    if facts.baseline == BaselineAvailability::Unconfirmed {
        return FailureDisposition::RecoveryRequired;
    }

    let deferrable = facts.cause == TryCauseKind::Transient
        && facts.policy.allows_deferral()
        && !facts.candidate_has_invalid_item;

    if deferrable {
        FailureDisposition::Deferrable
    } else {
        FailureDisposition::Reject
    }
}

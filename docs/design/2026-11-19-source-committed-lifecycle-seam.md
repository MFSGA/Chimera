# Ref source-commit notifications via the existing CoreLifecycle mailbox

## Baseline

Chimera `master` `9dd9e2bf`, upstream `ref` pinned at
`c4b35e04`. Upstream `RuntimeGate::committed(domain, version)` sends a
source-only notification to the one `RuntimeReconciler` actor; that actor
reads the latest source snapshots instead of applying the stale candidate
supplied by the sender. Chimera is still in the compatibility migration:
its `CoreLifecycle` mailbox remains the **only runtime mutation owner**,
while successful typed Clash/Application changes have an inline postcommit
apply and Profiles still uses a transactional participant.

## Production source notifications

`SourceCommitted { domain, version, retryable_deferred }` is delivered to
`CoreLifecycle`, which monotonically records independently observed source
versions for Application, Clash and Profiles. Sources call this only after
durable state publication:

- **Application typed patches**: notify after a successful source mutation,
  whether runtime-affecting or UI-only. Only a retryable postcommit runtime
  degradation asks for the existing dirty-window rebuild.
- **Clash typed/raw/override patches**: send the committed version once
  through the relevant typed or raw path; preserve the existing compatibility
  effects/legacy mirror reports. Retryable deferred applies use the same
  CoreLifecycle dirty window.
- **Profiles**: the existing `MutationCoordinator::finish` notifies with the
  committed version after participant settlement. The participant retains
  its own deferred retry policy; the new notification **does not** schedule a
  second retry or replace the participant.
- **Legacy Verge multi-domain patch**: notify Application and/or Clash
  only after all affected source domains and the legacy finalize step succeed.
  If compensation/rollback starts, no successful notification is emitted.
- **Hotkey-only typed Application patch**: report its committed source version
  without scheduling Core apply.

The old `request_runtime_rebuild()` API remains for unrelated recovery
events; the typed commit paths now use `source_committed()` instead.

## Safety and ordering

A normal notification is evidence of a source write, **not** an instruction
to apply the runtime a second time. A retryable-deferred notification asks
the existing mailbox to reconcile _the latest_ source target, never the
notification's captured config bytes. Even when the incoming version is
older than the stored watermark, its deferred signal is retained: the newer
document may still contain a runtime target not yet applied. The mailbox
keeps the same uncertain/shutdown fences and startup-owner recovery branch
as existing runtime dirtiness handling. The client also preserves the
previous `startup_recovery_needed()` check, including its Service readiness
generation boundary: a failed startup generation uses owner recovery instead
of racing ahead with an ordinary runtime rebuild.

These notifications are not yet the upstream actor's
`RuntimeGate::committed` implementation: the full actor would wake on
all changes and track the target's applied receipt, run intent and
conclusion itself. Here the compatibility owner still applies successful
changes inline, so replaying every commit would be incorrect.

## Verification

The new actor regression test verifies domain-separated monotonic
watermarks, absence of extra Core apply after successful notifications,
out-of-order retryable notifications, and coalescing of multiple retry
signals into one existing runtime rebuild. The project regression suite
passes **1007 tests, 0 failures, 1 ignored**. TypeScript bindings
freshness is covered by the test suite.

## Next migration

Replace Profiles' old participant runtime-first apply/compensation with
source-first commit plus independent candidate checks and notifications,
then move the application, startup, Core and Service commands together to
the upstream `RuntimeReconciler` actor. Do not start a second runtime
writer beside CoreLifecycle, and do not silently change explicit stop
intent or service owner verification semantics.

# Shared runtime candidate check — Clash, Application and Profiles

## Baseline

Chimera `master` at `40882b59`, with the pinned upstream `ref` at
`c4b35e04`. The upstream `RuntimeGate` has an independent
`check(RuntimeCandidate)` boundary followed by source-owned persistence
and `committed(domain, version)` notification. Chimera has not migrated
the entire gate or its single RuntimeReconciler actor yet.

## Migrated production boundary

The candidate check implementation now lives in
`backend/tauri/src/client/reconciler/check.rs`, and is used by all
three live candidate entry points:

- **Clash**: typed/raw/override patches freeze the committed Profiles
  document and managed file contents while supplying the proposed Clash
  document, then run the shared independent check before the Clash CAS.
- **Application**: the CoreLifecycle application patch freezes committed
  Profile inputs, combines them with the proposed application document,
  and invokes the same checker before committing the versioned source.
- **Profiles**: the existing transactional participant's validation calls
  the shared checker, preserving its `CheckRecord` for the legacy
  decision/receipt logic. The participant still governs the old commit
  transaction; this batch does not move its runtime apply ahead or behind
  the source commit.

The checker converts frozen input errors into the typed
`RuntimeCheckFailure::candidate_unavailable` evidence, selects the
actual run type from the proposed application's Service preference and
the supplied Service connectivity observation, and invokes
`CoreLifecyclePort::validate_profile_runtime` **without acquiring the
mutating CoreLifecycle command owner**.

For Clash and Application, `require_passed` requires a positive
`CheckRecord::Passed` before allowing the source mutation. A skipped,
unserviceable, rejected or unowed check cannot be silently treated as
approval. The old standalone checks in Clash and Application are removed;
there is no new writer or second reconcile loop.

## Regression coverage

The new tests exercise:

- Positive-only admission for every `CheckRecord` variant.
- Selection of Service versus Local validation from explicit candidate
  preferences and Service connectivity.
- Rejected Core checks and failed frozen-file content reads, without
  ever reaching the lower validator for incomplete inputs.
- No lifecycle reconcile, stop, binary switch, status read, or
  connection interruption from the independent check.

Existing Clash and Application tests continue to cover independent
checking before source commit, version conflicts, source persistence
failure, and committed-but-deferred runtime outcomes.

## Remaining work

This is the **check half** of the upstream `RuntimeGate`, not its
`committed` half. Applying an accepted committed target is still
handled by the existing CoreLifecycle/ApplicationWorkflow compatibility
owner. The next architectural migration must replace that owner's
Profile pre-commit runtime participant and reconcile notifications as
one coherent change; scheduling another runtime write loop beside it
would introduce races and violate the single-owner design.

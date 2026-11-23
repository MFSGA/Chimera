# Profiles document-only commit migration

## Upstream and scope

Chimera `master` baseline: `f9015f19`; pinned `ref`:
`c4b35e04`. Ref's `state/profiles/actor.rs::persist_candidate`
first decides whether a proposed document or managed content reaches the
active runtime dependency closure. Pure source changes use
`PersistentStateManager::replace_if_version` directly. A runtime-impacting
candidate must pass a separate runtime check before it is committed.

Chimera still uses a legacy `replace_if_version_with_participant` for
Profiles transactions. This batch migrates **only the document-only path**
of `run_state_write`. It does not change active config activation,
runtime-affecting definition changes, or any file-first materialization
operation, whose existing participant and compensation remain intact.

## Production behavior

1. Validate and revision-bump the candidate as before.
2. Classify the full current dependency closure using the existing
   `impact::classify_profiles(before, next, hints)`; the caller's
   `AffectsRule` alone is insufficient.
3. Take the direct path only if the command is a `Save`, no runtime field
   or activation was explicitly requested, no resource/file content was
   staged or touched, and the runtime impact is `None`.
4. Persist through `replace_if_version` under the Profiles content
   publication write fence; both early and publication-time CAS mismatches
   produce `ProfilesError::VersionConflict`, and persistence failures never
   advertise a committed source.
5. After a successful source commit, send the Profiles version to the
   existing single CoreLifecycle notification mailbox with a
   `RuntimeCommitStatus::Unchanged` receipt. No TCC operation ID or
   running Core apply is created for these pure source writes.
6. Keep all other writes on the existing participant path, including
   explicit re-selection of the current Profile even when the document
   is unchanged; run intent and activation promises remain owned by the
   compatibility workflow until RuntimeGate is fully ported.

## Validation

The new integration test selects and reselects an active Profile, renames
it, reorders profiles, and replaces the active definition. It confirms that
both explicit selections and touched active definitions still use the runtime
participant, whereas metadata and reorder changes commit and persist without
a participant. Existing suite exercises layouts, remote options, file-first
promotion/compensation, refresh and cleanup. The full `chimera-core` suite
passed 384 tests and the full Tauri library suite passed 1008 tests (one
ignored).

## Follow-up

The remaining Profiles resource operations need a ref-like independent
candidate check that freezes proposed managed bytes, followed by
atomic file promotion and source CAS under the content fence. The
compatibility TCC currently performs runtime work before publication;
replacing it must preserve explicit activation and stopped-by-user
semantics, and eliminate compensating runtime rollbacks only once the
single RuntimeReconciler can take responsibility for committed targets.

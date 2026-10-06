# Profile transform candidate rejection contract

- Contract ID: `profile-transform-candidate-rejection`
- Covered cases: rejected global and scoped chains with failing JavaScript;
  rejected global overlay YAML; rejected edits to an active script file; valid
  repairs clear the diagnostics and apply.
- Risk: the lenient transform executor can pass the previous config through
  after an error. If a Profile transaction treats that artifact as a valid
  candidate, it can commit broken source or chain state while the runtime still
  reflects an earlier config.
- Boundary: desktop UI E2E for chain selection, save, draft retention, and
  diagnostics. The invalid script body is injected with `save_profile_file`
  IPC, so that case proves the Profile transaction/file boundary and its UI
  feedback; it does not prove typing the broken body into the editor.
- Base suites: `profiles` and `critical`; the PR desktop workflow runs
  `critical` on Windows. A spec file or local typecheck alone is not desktop
  evidence.
- Runtime conditions: Tauri E2E app, WebdriverIO, Windows Edge/WebView2 in CI,
  local isolated E2E runtime directory and mixed port. The core must be running
  before runtime diagnostics are checked.
- Real dependencies: transform chain editor controls; Profile IPC and
  transaction actor; runtime builder and script/overlay executor; runtime
  diagnostics; managed profile files for the script-save cases.
- Setup and bypasses: fixture profiles and file bodies are created over IPC;
  read-only IPC observes Profile snapshots, file contents, runtime revision,
  and diagnostics. The application and runtime are real. No fake transform
  executor or mocked transaction is used.
- Initial state: the suite snapshots the selected Profile and global chain,
  creates uniquely named source/overlay/script Profiles, activates a local
  source, and waits for the core to run. Each failure case establishes its own
  relevant global or scoped baseline and verifies diagnostics are clear before
  the failed operation.
- Operation: choose and save an invalid global/scoped chain through the chain
  editor, or submit an invalid body to `save_profile_file` for an active script.
  Each operation is performed once outside polling.
- Authoritative result: `get_profiles` / `read_profile_file` confirms the
  stored chain or script content; `get_runtime_transform_diagnostics` confirms
  the applied revision, attempted revision, failure UID, scope, and error
  message.
- Current UI result: failed chain candidates remain visible as editor drafts
  with a row-level failure and a nonblocking in-app notice; unrelated errors
  retain the native error dialog. Rejected script saves keep the profile editor
  open and surface the attempt diagnostic. A refresh is not used to create the
  expected current-page state.
- Persistence and restart: rejected script edits must leave the managed file
  byte-for-byte unchanged. These cases do not claim cold-start persistence or
  restart recovery. Successful repair is checked in the active runtime and,
  for script edits, by reading the saved file.
- Failure and old-state invariants: a failed candidate advances the attempt
  revision but keeps the applied runtime revision and persisted source/chain
  unchanged. The diagnostic must identify the responsible transform. A valid
  repair must advance the applied revision and clear the failure.
- Waits: UI/editor visibility waits are bounded at 15 seconds; runtime,
  persistence, and diagnostic convergence waits are bounded at 30 seconds.
  Polling only reads state. WDIO and Tauri service timeouts remain bounded by
  the harness configuration.
- Diagnostics: WDIO failure output and the configured E2E log directory;
  profile names use a unique per-suite suffix and contain no user data.
- Resources and cleanup: the suite records the original Profile selection and
  global chain, registers cleanup before creating resources, restores state,
  clears test transforms, closes editor windows, and deletes only its uniquely
  named Profiles. Cleanup errors are aggregated with the primary test error.
- Regression sensitivity: treating a transform error as a successful
  passthrough candidate would change the stored chain/file or applied runtime
  revision, contradict the authoritative state assertions; dropping failure
  diagnostics would fail UID, scope, script type, and message assertions.
- Relevant rules: T01, T02, T03, T05, T06, T07, T09, T10, T13, and T14.
  No real network, system proxy, TUN authorization, or host networking claim is
  made by these tests.

## Local execution record

On macOS, the focused spec was attempted with a dedicated E2E config directory
and a free mixed port. WebKit did not display the transform-chain editor in six
cases, so this was a failed local desktop run, not a pass. The Windows Edge run
for head `bad04f694eb19cc3269750587a853dd6d4429653` built all platform binaries
but failed the critical suite: 14 spec files passed and 5 failed. Rejection
cases opened the native Tauri error dialog while retaining the draft; WebDriver
could not dismiss it, so the save task stayed pending and later UI checks timed
out. The error boundary now uses a nonblocking notice for a newly published
transform failure, and recovery checks wait for the draft to update before
saving. A new Windows Edge CI run is required to verify this correction; the
failed run is not a pass.

On head `fc89659efd53fc29b3ed73ccbc1d21dab25ebc60`, Windows Edge critical
finished with 18 spec files passed and one failed. The blocking-dialog and
recovery-ordering issues were cleared; the remaining failure was an incorrect
expectation that `data-transform-type` meant the overlay kind. The selector
contains the Profile discriminator (`transform`), while the diagnostic label
identifies the overlay as `Merge (YAML)`. The assertion now checks both
meanings separately. This run remains a failure; the updated Windows Edge suite
must pass before merge.

On head `10c9773e26c4308f2950d82e0fb39480e5b4f979`, Windows Edge critical
again finished with 18 spec files passed and one failed in that same case. The
remaining mismatch was its stale `YAML mapping` substring; the actual diagnostic
is `overlay document is not a mapping, skipped`. The assertion now checks the
shared `not a mapping` phrase. This run also remains a failure and does not
count as verification of the correction.

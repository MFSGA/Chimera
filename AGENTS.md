# AGENTS

## Project overview

- Chimera is a Tauri desktop app with a Rust backend and a React/Vite frontend.
- The repo is a pnpm workspace (`frontend/*`, `scripts`, `tauri-e2e`).

## Product goal and reference policy

- Keep shared functionality as close as possible to the implementation in local `ref/` (Clash Nyanpasu): architecture, business logic, directory structure, filenames, type names, function names, variable names, and interface contracts. Equivalent behavior alone does not complete alignment.
- Keep branding differences limited to actual product identity, such as Chimera names, package identifiers, application IDs, resources, and required distribution endpoints. Do not rename unrelated upstream concepts or reorganize shared code for personal preference.
- **Large-scale upstream copying is explicitly allowed and preferred when it keeps a cohesive feature aligned.** Copy complete related files/directories/call paths from the pinned local `ref/` implementation as needed, then apply a reviewed, consistent upstream-brand → Chimera-brand mapping across the copied scope. Do not artificially split a coherent migration merely to keep the diff small. Preserve non-brand symbol names, architecture, behavior, and license/copyright notices; inspect replacement matches so protocol fields, persisted data, third-party identifiers, and external APIs are not accidentally changed.
- Preserve the legacy UI as a supported product interface. It is an intentional extension, not obsolete code to delete during alignment. Keep its routes, window entry points, presentation, and user flows working.
- The main extension is E2E testing; the other extension is agent capability. Preserve both while aligning the shared implementation.
- The main UI, legacy UI, and agent must reuse the same application APIs and business implementation. Put necessary legacy presentation adapters and agent orchestration at their boundaries; do not maintain separate configuration, persistence, core, or service implementations for them.
- Existing differences outside branding, legacy UI, E2E, and agent are not automatically permanent exceptions. Record their purpose and convergence plan. Preserve working functionality and user data during migration; do not remove Chimera Client/Service support or other behavior merely to reduce a diff. Keep necessary compatibility in narrow adapters and distinguish it from the aligned shared implementation.
- Follow [the alignment guide](docs/ref-alignment-guide.md) for baseline selection, difference records, migration steps, and acceptance criteria. Keep this file and the guide consistent when the product goal changes.

## Codex / AI development governance

This repository also follows [AI development governance](docs/ai-development-governance.md),
the [independent review checklist](docs/ai-review-checklist.md), and the
[engineering lessons log](LESSONS.md). These rules **add to** the reference-alignment
guide, [testing standard](docs/testing/README.md), and existing safety boundaries;
they do not replace them. GitHub workflow realities are summarized in
[WORKFLOWS.md](.github/WORKFLOWS.md).

- For any nontrivial task, record objective, explicit non-goals, acceptance criteria,
  changed interfaces, risk level L0–L3, test plan, rollback, and any approval triggers.
  Use the [AI task issue template](.github/ISSUE_TEMPLATE/ai-task.md) for tracked work.
- Before coding, verify branch, Git state, relevant submodule SHAs, existing source
  and tests. Shared-feature tasks **must** follow the ref path/symbol mapping and
  preserve both main and legacy UI and the agent's shared application API.
  Work in isolated branches/worktrees when several agents or workstreams are active;
  never overwrite another developer's dirty tree or generated files.
- The coder proves implemented behavior through actually executed checks.
  An independent reviewer, with a fresh context, checks the real diff, reference
  mapping, tests, failure/recovery paths, host effects, and authorization.
  Self-review and a green build cannot substitute for applicable test evidence.
- **Do not idle waiting for remote CI after local tests pass.** Once an authorized
  PR/push has started checks, record the run link and `pending` status, then
  immediately continue useful work that does not depend on those results:
  independent review preparation, focused local verification, documentation,
  risk/rollback analysis, or another already-authorized task in an isolated
  worktree. Avoid repeated empty polling or holding the task open solely for CI.
  If no independent work remains, hand off the pending CI state and next steps.
  CI pending is **not** success: do not merge/release before required checks and
  approvals, do not promise background monitoring, and treat a discovered CI
  failure as a blocker requiring diagnosis before delivery/merge.
- **Consolidate fixup history only after the issue is confirmed resolved.**
  If one issue required multiple task-branch commits (implementation, CI fixes,
  test corrections), keep them while investigating. Once the final behavior and
  required checks are verified, squash/fixup only those related commits locally
  into a coherent commit (or logically independent commits) before final merge.
  Preserve the complete final diff, test evidence, authorship and review trail.
  Never rewrite `master`, protected/shared history, tags or other people's commits.
  A history rewrite on an owned feature branch requires explicit permission and
  coordination; if updating its remote, use `--force-with-lease`, never plain
  `--force`. Re-run required CI/review against the new commit SHA: results from
  replaced SHAs do not authorize merging the rewritten commit.
- Risk levels determine the required evidence and review, not a routine approval
  pause. The user's standing instruction authorizes ordinary, scoped code
  iterations—including L2 implementation, fixes, tests, independent review, and
  signed commits pushed to the task branch/PR—without asking for approval at each
  step. Continue through implementation and verification when the request is clear.
  L2 includes persisted configuration, core/service runtime, system proxy/TUN,
  IPC/agent mutations, security, dependency/sidecar pins, and shared behavior.
- Get approval before implementing a major architectural change: replacing a major
  subsystem, changing shared API/state-ownership boundaries, making a cross-cutting
  frontend/IPC/core/Service/agent redesign, or changing the security/authorization
  model or a persisted-data migration with broad compatibility impact. Resolve
  ordinary local design choices autonomously and keep them within the stated goal.
- L3 covers `AGENTS.md`, governance rules, CI/release, signing, secrets, update
  channels, privileges, and Git history rewriting; these changes still require
  maintainer review and approval. Separately retain task-specific authorization for
  host-network mutations, privilege elevation, destructive data operations,
  release/sign/publish/tag actions, remote workflow dispatch, and rewriting shared
  or protected history. A request to implement code does not authorize merging to a
  protected branch; merge only when the task asks for integration and all required
  checks and repository approvals pass. Never bypass those controls.
- The L0/L1 constrained-automation limitation applies to unattended automation,
  not interactive, user-directed code work. When scopes overlap, use the highest
  risk category for review and verification.
- All PRs should complete the [PR template](.github/pull_request_template.md):
  evidence and exact commands, coverage and omissions, review findings,
  reference SHA, change risk, and rollback. Do not claim CI enforces rules that
  are currently only documented.
- Test failures, regressions, and confirmed process mistakes belong in
  `LESSONS.md` with evidence and prevention tests; suspected issues go into
  an Issue, not an invented incident report. Normative changes require L3 review.
- Do not commit, push, merge, tag, publish, dispatch workflows, mutate host network
  or elevate privileges unless those actions have been authorized for the task.
  Do not bypass checks, disable tests, lower permission gates, or expose
  credentials to make an AI task pass. The documents here do **not** install
  autonomous coding, AI review, or auto-merge actions.

## Required workflow for shared functionality

1. Before developing, fixing, or refactoring shared functionality, read its local `ref/` implementation and relevant architecture guidance. Trace the UI/hook, IPC, application API, state ownership, persistence, and runtime path as applicable. This reference read is the default and does not require a separate user request.
2. Record `git -C ref rev-parse HEAD` and inspect `git -C ref status --short`. Map the touched reference paths and symbols to Chimera paths and symbols, including only necessary branding substitutions. Do not silently update `ref/` or replace the baseline with a different online revision. If the needed reference is unavailable, report the gap and avoid claiming alignment.
3. Prefer the reference implementation in corresponding files. For broad alignment, **copy the full cohesive upstream implementation first** (including related directories, types, state ownership, error handling, and tests), then consistently replace only required product-brand identifiers using an explicit mapping. Do not replace it with a locally designed equivalent, rename non-brand symbols, or split/reorganize reference functions just to satisfy local stylistic preferences. Generate bindings through the existing generator when the changed contract requires it.
4. Choose migration scope by **feature completeness and verifiability, not by an arbitrary small diff**: copying many related upstream files or whole modules in one pass is allowed. Check each brand replacement and preserve licenses, public contracts, stored user data, existing Chimera Client/Service support, legacy UI and agent integration. A compatibility bridge is a temporary migration step: document its reason, remaining differences, and removal condition. Only a legacy business bridge may be retired; the legacy UI itself remains supported.
5. Check the main and legacy UI entry points affected by shared changes, and the agent entry point when it uses that capability. Update relevant tests, register their execution entry points, and run checks appropriate to the change.
6. Report the reference commit, path/symbol mapping, retained differences, affected interfaces, actual test results, and remaining work. Label incomplete migrations as partial; matching behavior, filenames, or types alone is not proof that the implementation is aligned.

## Extension boundaries and verification

- Legacy UI: preserve its presentation and interaction model while adapting shared hooks and commands to the reference-aligned backend. Do not require every agent feature to appear in the legacy UI unless requested; preserve existing supported flows and test those affected by a shared change.
- E2E: keep harness and specs in `tauri-e2e/`. Prefer real user controls, then verify persistence, runtime state, network effects, or restart behavior when relevant. Keep production changes for testing limited to explicit boundaries such as feature-gated WebDriver support, isolated paths, and minimal stable selectors.
- Distinguish pure/unit tests, fixture-based UI/orchestration tests, and real desktop/runtime/network tests. Fixtures may support deterministic coverage, but must not be presented as proof of actual authorization, host mutation, or recovery. A skipped or unexecuted test is not a pass.
- Register new specs in the appropriate suite and make sure new unit tests have a runnable entry point. Verify which suite CI actually executes; a test file's existence, a typecheck, or clippy does not mean its assertions ran. Consult [the E2E README](tauri-e2e/README.md) for current commands and limits.
- Run shared behavior checks through both supported UI entry points where applicable, and verify affected supported cores/platforms. State any missing environment or coverage rather than claiming the full matrix passed.
- Isolate test data and restore test-owned state. File isolation does not isolate host networking. Use a dedicated runner or VM for host-affecting tests, respect existing authorization for system changes, and scope cleanup to test resources. Do not publish credentials, personal configurations, subscription URLs, or unsanitized artifacts.
- Agent: keep tool contracts, diagnostics, proposals, and orchestration in feature modules. Read and mutate through shared application APIs and transactions; do not create a parallel business layer. For mutating actions, preserve user confirmation, stale-state checks, meaningful execution/verification errors, auditability, and recovery semantics. Do not report completion from an API success alone when observable state must be verified.
- Do not copy a demonstrated reference defect for consistency. Add a reproducer, make the smallest correction, and record the exception and when it should be rechecked against ref.

## Key paths

- `backend/` Rust workspace; Tauri config lives in `backend/tauri/tauri.conf.json`.
- `frontend/chimera/` main app package (`chimera-ui`).
- `frontend/ui/` shared UI components.
- `frontend/interface/` shared interface/types package.
- `frontend/chimera/src/pages/(legacy)/` supported legacy UI routes.
- `tauri-e2e/` desktop E2E harness, suites, and supporting tests.
- `docs/` design notes and change logs.

## Setup

- Initialize the pinned reference and runtime submodules with `git submodule update --init --recursive`.
- During development, try to run with administrator/root privileges when possible (needed for TUN-related setup).
- Install JS deps with `pnpm install`.
- Rust toolchain is required for `backend/` (cargo, rustfmt, clippy).
- Run `pnpm prepare:check`.
- Ensure required binary resources are available before development runs.

## Common commands

- `pnpm dev:diff` runs Tauri dev + React devtools.
- `pnpm tauri:dev` runs Tauri dev only.
- `pnpm web:dev` runs the frontend app only.
- `pnpm build` builds the Tauri desktop app.
- `pnpm lint` runs oxlint/prettier/stylelint/clippy.
- `pnpm fmt` runs prettier + cargo fmt.
- `pnpm typecheck` checks the frontend packages and E2E TypeScript.
- `pnpm lint:frontend-boundaries` checks shared frontend IPC boundaries.
- `pnpm e2e:tauri:build` builds the desktop E2E binary.
- `pnpm --filter @chimera/tauri-e2e test:unit` runs the currently registered harness unit tests; it is not all unit tests.
- `pnpm --filter @chimera/tauri-e2e test:<suite>` runs a selected desktop suite; the default `test` runs smoke only.

## Code conventions

- JS/TS formatting via Prettier; lint via oxlint and Stylelint.
- Rust formatting via `cargo fmt`; lint via `cargo clippy`.
- Commit messages follow Conventional Commits (commitlint).

### code conduct

- For shared functionality, retain the reference's decomposition, naming, and conventions. Do not introduce a structural diff solely to shorten a function or add comments.
- For Chimera-only extensions, prefer functional style where practical and keep pure functions and side effects separate.
- For extension code, keep functions less than 50 lines when practical and split substantial logic into cohesive functions. Comment non-obvious execution order, invariants, and temporary compatibility boundaries.

## Agent notes

- Use `pnpm` (not npm/yarn). Update `pnpm-lock.yaml` when deps change.
- Keep changes scoped. Regenerate affected generated files through the established tools when required by a source or contract change; do not hand-edit them or regenerate unrelated outputs.
- Prefer workspace filters (example: `pnpm --filter=chimera-ui <script>`).
- Treat `ref/` as a read-only implementation baseline by default; exclude its dependencies and build output from broad searches. Do not modify or synchronize the reference checkout as a side effect of an application change.
- Keep migration records and normative guides versionable; do not leave the project's governing documents only in ignored local files. Keep personal evidence, caches, raw logs, and credentials ignored.
- Preserve unrelated worktree changes. A request to align a feature is not permission for an unrelated rewrite, legacy UI removal, or destructive data migration.

## Test authoring and review

- Before adding, changing, or reviewing tests, read and follow [the testing standard](docs/testing/README.md). For Tauri E2E, also read its [dependency source notes](docs/testing/upstream-evidence.md) and use the [test contract template](docs/testing/test-contract-template.md).
- The testing standard's MUST / MUST NOT rules are review requirements for new or changed tests. Existing tests are not automatically compliant examples. Record any bounded exception with its evidence, coverage limitation, and removal condition in the test contract; do not silently weaken assertions.
- Test-tool behavior must be checked against the versions in `pnpm-lock.yaml` and `backend/Cargo.lock`, using the testing dependencies' upstream documentation and source. Product `ref/` is not the source of truth for WebDriver, runner, assertion, or lifecycle behavior.
- Report the exact suites and checks executed, skipped or unverified coverage, and failures. A smoke run, page refresh, successful IPC return, or retry pass must not be presented as full E2E, cold-start persistence, real network success, or a clean first-run pass.

# AGENTS

## Project overview

- Chimera is a Tauri desktop app with a Rust backend and a React/Vite frontend.
- The repo is a pnpm workspace (`frontend/*`, `scripts`, `tauri-e2e`).

## Product goal and reference policy

- Keep shared functionality as close as possible to the implementation in local `ref/` (Clash Nyanpasu): architecture, business logic, directory structure, filenames, type names, function names, variable names, and interface contracts. Equivalent behavior alone does not complete alignment.
- Keep branding differences limited to actual product identity, such as Chimera names, package identifiers, application IDs, resources, and required distribution endpoints. Do not rename unrelated upstream concepts or reorganize shared code for personal preference.
- Preserve the legacy UI as a supported product interface. It is an intentional extension, not obsolete code to delete during alignment. Keep its routes, window entry points, presentation, and user flows working.
- The main extension is E2E testing; the other extension is agent capability. Preserve both while aligning the shared implementation.
- The main UI, legacy UI, and agent must reuse the same application APIs and business implementation. Put necessary legacy presentation adapters and agent orchestration at their boundaries; do not maintain separate configuration, persistence, core, or service implementations for them.
- Existing differences outside branding, legacy UI, E2E, and agent are not automatically permanent exceptions. Record their purpose and convergence plan. Preserve working functionality and user data during migration; do not remove Chimera Client/Service support or other behavior merely to reduce a diff. Keep necessary compatibility in narrow adapters and distinguish it from the aligned shared implementation.
- Follow [the alignment guide](docs/ref-alignment-guide.md) for baseline selection, difference records, migration steps, and acceptance criteria. Keep this file and the guide consistent when the product goal changes.

## Required workflow for shared functionality

1. Before developing, fixing, or refactoring shared functionality, read its local `ref/` implementation and relevant architecture guidance. Trace the UI/hook, IPC, application API, state ownership, persistence, and runtime path as applicable. This reference read is the default and does not require a separate user request.
2. Record `git -C ref rev-parse HEAD` and inspect `git -C ref status --short`. Map the touched reference paths and symbols to Chimera paths and symbols, including only necessary branding substitutions. Do not silently update `ref/` or replace the baseline with a different online revision. If the needed reference is unavailable, report the gap and avoid claiming alignment.
3. Prefer the reference implementation in corresponding files. Do not replace it with a locally designed equivalent, rename non-brand symbols, or split/reorganize reference functions just to satisfy local stylistic preferences. Generate bindings through the existing generator when the changed contract requires it.
4. Migrate the smallest complete call path, including state ownership and error handling. A compatibility bridge is a temporary migration step: document its reason, remaining differences, and removal condition. Only a legacy business bridge may be retired; the legacy UI itself remains supported.
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

- During development, try to run with administrator/root privileges when possible (needed for TUN-related setup).
- Install JS deps with `pnpm install`.
- Rust toolchain is required for `backend/` (cargo, rustfmt, clippy).
- Run `pnpm check`.
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

# Ref sync and alignment task — 2026-11-23

## Objective and acceptance criteria

Fast-forward the application branch to the fetched `origin/master` tip, update
the pinned local `ref/` submodule to the fetched `origin/main` tip, compare the
restored local changes against that revision, keep the DNS/settings UI scoped to
the current ref, and align shared behavior before committing on
`codex/ref-alignment-sync-optimize`.

Acceptance criteria:

- The application branch includes the fetched `origin/master` tip
  `04c668aeb4cdc1a26fbee46cafa5349d2e5a2fe9` before local changes are committed.
- `ref/` is pinned at `4607a7379f0aa060daf55b369dc8103dda929d82`, and its recursive
  submodules and worktree are clean.
- The DNS/Hosts page, settings navigation, and route crumbs present in the
  reference remain available. The current `04c668a` baseline and candidate
  contain no Chimera-only applied-DNS summary panel, so this commit does not
  claim a deletion for it.
- DNS draft/save, YAML editing, DNS tools, runtime candidate validation, and
  generated IPC bindings use one shared application path and preserve existing
  profile-owned data until takeover is enabled.
- Remaining reference differences are recorded with a convergence condition.
- Applicable local checks run and their exact results are recorded before the
  task-branch commit.

## Non-goals

- Do not remove or disable the supported legacy UI, agent, Chimera Client/Service,
  or existing user configuration.
- Do not change system proxy, TUN, service installation, privilege, release,
  signing, branch protection, or protected-branch history.
- Do not push, merge, or publish this task branch.
- Do not copy ref dependencies or build output into Chimera.

## Baselines and mapping

- Application remote baseline after final fast-forward: `origin/master` and
  `HEAD` at `04c668aeb4cdc1a26fbee46cafa5349d2e5a2fe9`. The task branch first reached
  `a59a13e12fb66a9b03078588c78180c99a50eee8`, then the remote advanced by eight
  commits while the candidate was being prepared. The staged changes were saved
  in a recovery stash, the branch was fast-forwarded, and DNS/UI work was merged
  with the new upstream TUN/MIPS capability matrix before review. Before commit,
  `origin/master` advanced six more commits from `baa10cab` to `04c668a`; the
  work was saved again, fast-forwarded, and restored. The new upstream changes
  touch CoreLifecycle Service-readiness evidence and its diagnostic records;
  they do not overlap the DNS/runtime changes. Their source records in
  `LESSONS.md` and `docs/ref-alignment-guide.md` were preserved during conflict
  resolution. The old `master` was `c2a1e099653c2364fd59deb88d77a1685b407ef7`.
- Prior ref gitlink: `c4b35e04870a9caa53f21d10af6fcfde4a6f4c01`.
- Current ref: `4607a7379f0aa060daf55b369dc8103dda929d82`, matching
  `origin/main`; `git -C ref status --short` is clean. Recursive submodules:
  `enable-loopback` `f1b9f8693a328ca0a6094951035b81fe302b0848`, nested
  `windows-rs` `d1574fd27371a9b08bc5508f70c43ccb4f84c543`, runtime
  `0bf12e53530847d429d6bc98670162626806a41c`, and utils
  `43fbd911467f1563dc47d9ee1261ab972022d716`.
- Existing user work was restored from `codex-pre-sync-2026-11-23`; both it and
  `codex-pre-remote-refresh-2026-11-23` plus
  `codex-pre-latest-remote-refresh-2026-11-23` remain as recovery stashes.

| Reference path / symbol | Chimera path / symbol | Decision |
| --- | --- | --- |
| `ref/frontend/nyanpasu/src/pages/(main)/main/settings/clash/dns/` | `frontend/chimera/src/pages/(main)/main/settings/clash/dns/` | Keep and align the upstream page; adapt shared query/typed settings hooks and product branding. |
| `ref/backend/nyanpasu-config/src/clash/config/dns.rs`, runtime DNS merge | `backend/chimera-config/src/clash/config/dns.rs`, `features.rs`, runtime executor and builder | Preserve upstream data flow; retain the Chimera core feature matrix and compatibility boundary. |
| `ref/backend/nyanpasu-core/src/actor_v2/api.rs`, `ref/backend/nyanpasu-rpc/src/commands/proxy/dns.rs` | `backend/chimera-core/src/actor_v2/api.rs`, `backend/tauri/src/client/clash_api.rs`, `backend/tauri/src/ipc.rs` | Reuse the instance-bound shared core API and generate the Tauri/TypeScript contract. |
| `ref/frontend/nyanpasu/src/pages/(main)/main/settings/_modules/settings-navigate.tsx`, `components/page-title.tsx`, route crumbs | corresponding Chimera settings navigation, `components/page-title.tsx`, `components/router/route-crumbs.ts` | Keep the nested settings navigation and route labels that exist upstream. |
| `ref/frontend/nyanpasu/src/pages/(main)/main/logs/` | Chimera `main/logs/` and `logs-layout.tsx` | Align the shared viewer layout and responsive log handling; source switching remains partial because the local app/file/service log APIs are absent. |
| No matching ref page; no panel in `04c668a` baseline | No applied-DNS summary panel in the candidate | User preference was to omit unique extension UI; no file deletion was needed in this commit. |

## Changed interfaces and risk

- `ClashConfig` gains typed DNS/Hosts settings and independent control selectors;
  the runtime candidate consumes them through the existing builder and mutation
  transaction. `ControlSource::Chimera` accepts the older serialized `nyanpasu`
  spelling as an alias.
- Tauri exposes generated commands for core DNS query and DNS/Fake-IP cache
  flushes through the shared, instance-bound Clash API client.
- The main settings route adds the DNS page and route metadata. Existing settings
  pages use a shared `PageTitle` component; no legacy route is removed.
- Settings item navigation still uses the ref `useLinkProps` for its target and
  click behavior. In the current Tauri route tree its resolved `aria-current`
  stayed absent after a real route click, so selection presentation now uses
  `useMatchRoute` for `data-active` and `aria-current`. The old behavior and the
  scoped correction were both exercised by the existing desktop layout spec;
  the matching ref dependency install is stale, so this narrow adapter needs
  rechecking against a clean ref install before it can be removed.
- Risk: L2, because persisted Clash configuration, runtime rebuild decisions,
  IPC, and running-core actions are involved. No major architecture or
  authorization-model change is planned.

## Test plan

- Format and diff checks: format changed TypeScript/JSON; run a targeted Rust
  formatter check and `git diff --check`. The repository-wide formatter may
  expose unrelated vendored/submodule formatting differences.
- Rust: focused existing `chimera-config`, Core, and Tauri checks covering DNS
  configuration, runtime transaction behavior, MIPS selection, and generated
  bindings. Avoid redundant or host-affecting settings suites.
- Frontend: regenerate Paraglide messages and Specta bindings, then run Chimera
  and E2E TypeScript checks and frontend boundary lint.
- Desktop: build the E2E app and run only the DNS settings, settings navigation,
  and corrected settings layout specs needed for this diff. Do not run the
  aggregate `settings` suite because it includes the host-proxy/system-tools
  path; report that boundary and any focused failures.
- Independent fresh-context diff review after implementation.

## Rollback and approval triggers

- Roll back with a revert of the task commit; restore the previous `ref` gitlink
  to `c4b35e04870a9caa53f21d10af6fcfde4a6f4c01` if the ref pin itself must be
  reverted. The pre-sync and pre-remote-refresh stashes remain available as
  additional recovery copies.
- The user explicitly requested remote/ref synchronization and a commit. No
  push, protected-branch update, host-network mutation, privilege elevation, or
  destructive user-data operation is authorized or planned.
- Stop for maintainer approval only if review shows a major shared API/state
  ownership redesign, security/authorization change, or broad persisted-data
  migration is necessary.

## Verification record

### Superseded checks on the `a59a13e` candidate

These results were collected before the remote advanced to `baa10cab`; they are
historical diagnostics and do not verify the final candidate.

- Reference sync: app branch is at `origin/master` / `HEAD`
  `a59a13e12fb66a9b03078588c78180c99a50eee8`; `ref/` is pinned at
  `4607a7379f0aa060daf55b369dc8103dda929d82`; `git -C ref status --short` is
  clean and recursive submodule SHAs are recorded above/in the alignment guide.
- Rust tests: `cargo test --manifest-path backend/Cargo.toml -p chimera-config
  --lib` **280/280**; targeted `chimera` tests
  `profile_owned_dns_draft_persists_without_core_check_or_apply`,
  `dns_takeover_checks_full_candidate_before_atomic_source_commit`, and
  `dns_and_hosts_drafts_do_not_rebuild_until_takeover` **1/1 each**. The
  runtime ownership order regression also passed **1/1**.
- Generated/frontend checks: `pnpm --filter=chimera-ui generate:i18n`,
  `pnpm typecheck:chimera`, `pnpm typecheck:interface`, `pnpm typecheck:e2e`,
  `pnpm lint:frontend-boundaries`, `pnpm --filter=chimera-ui test:route-crumbs`
  (**5/5**), and `pnpm --filter=chimera-ui test:dns-yaml` (**1/1**) passed.
- Formatting: Prettier successfully formatted 54 changed frontend/E2E files;
  `rustfmt --edition 2024 --check` passed for the 15 touched Rust files;
  `git diff --check` and `git diff --cached --check` passed.
- Desktop build: `pnpm e2e:tauri:build` passed with
  `CARGO_PROFILE_DEV_DEBUG=0` and `CARGO_INCREMENTAL=0`. It built the production
  frontend and the feature-gated Windows E2E app; Cargo emitted existing
  unused/dead-code warnings.
- Focused desktop flows: `dns-settings-main.e2e.ts` **5/5** passed. The initial
  combined run exposed a test setup bug in `settings-page-navigation.e2e.ts`;
  after selecting the main window before resizing and waiting for the hub route
  to settle, the standalone spec passed **3/3**. It covers desktop/mobile
  settings navigation and the legacy composite entry.

### Checks on the pre-latest `baa10cab` candidate

- Rust: `cargo test --manifest-path backend/Cargo.toml -p chimera-config --lib`
  **289/289**; targeted Core MIPS selection **1/1**; targeted DNS/Hosts draft
  rebuild classification **1/1**; Tauri binding freshness **1/1**. The
  generated TypeScript bindings were refreshed through the ignored Specta
  updater test.
- Frontend: `pnpm typecheck:chimera`, `pnpm typecheck:interface`,
  `pnpm typecheck:e2e`, and `pnpm lint:frontend-boundaries` passed. The route
  crumbs unit check passed **5/5** and DNS YAML parser check passed **1/1**.
  The 15 changed Rust files passed targeted `rustfmt --check`.
- Desktop: after clearing only ignored `backend/target/e2e` build output, the
  frontend plus Windows E2E build passed with
  `CARGO_PROFILE_DEV_DEBUG=0`, `CARGO_PROFILE_DEV_STRIP=symbols`,
  `CARGO_PROFILE_BUILD_OVERRIDE_DEBUG=0`, and `CARGO_INCREMENTAL=0`. Cargo
  reports existing warnings. The binary was rebuilt from this candidate at
  `backend/target/e2e/debug/chimera.exe`.
- Desktop flows: DNS settings **5/5**, settings route navigation **3/3**, and
  the complete existing settings layout spec **6/6** passed against the rebuilt
  `baa10cab` binary on Windows/Edge 157. The layout spec fixed the window,
  Chinese locale, light theme, font readiness, and route-settled state; it
  exercises visible navigation clicks and verifies active states and page
  controls. These results do not verify the later `04c668a` Rust changes.
- A first attempt to rebuild after the active-state change failed because the
  drive was full. The E2E cache was confirmed ignored and contained only build
  outputs; after clearing it and reducing debug output, the fresh build and
  layout run passed. The user's installed Chimera process was left untouched.

### Final checks on the `04c668a` worktree

- Before staging/commit, `git ls-remote origin refs/heads/master` and both
  `HEAD`/`origin/master` returned `04c668aeb4cdc1a26fbee46cafa5349d2e5a2fe9`.
  `ref/` remained at `4607a7379f0aa060daf55b369dc8103dda929d82` and clean.
- The targeted Hosts renderer regression
  `cargo test --manifest-path backend/Cargo.toml -p chimera-config --lib
  hosts_values_are_filtered_per_core_without_losing_saved_draft` passed
  **1/1**. This covers a valid first IP followed by an alias for a core that
  reads only the first value. The full 289-test package suite was not repeated
  after this localized change.
- `pnpm typecheck:chimera`, `pnpm typecheck:interface`, `pnpm typecheck:e2e`,
  `pnpm lint:frontend-boundaries`, and
  `pnpm --filter @chimera/tauri-e2e exec node --test ./spec-suites.test.ts`
  passed; suite registration was **2/2**. All changed Rust files passed
  `rustfmt --edition 2024 --check`.
- A clean `pnpm e2e:tauri:build` passed from the final worktree, including the
  Hosts fix. The first incremental retry ran out of disk space; only ignored
  `backend/target/e2e` artifacts were cleared before the successful clean
  build. The final binary was `backend/target/e2e/debug/chimera.exe`.
- Against that binary on Windows/Edge 157, the registered DNS settings spec
  passed **5/5**, settings navigation passed **3/3**, and the existing settings
  layout spec passed **6/6**. The flows use visible controls and do not cover
  host proxy/TUN changes or real DNS network resolution.
- Independent review found that a cleanup failure could skip later theme,
  language, or window restoration in the affected specs. Each `after` hook now
  attempts owned cleanup steps independently and aggregates cleanup errors.
  E2E typecheck passed after the correction; the DNS settings, navigation, and
  layout specs were rerun on the same final application binary and passed
  **5/5**, **3/3**, and **6/6**, respectively.
- The six remote Service-readiness/CoreLifecycle commits compile in the final
  application build; their unit suite was not rerun in this task. No full
  Tauri/CoreLifecycle suite, aggregate settings suite, or system-affecting
  suite was run.
- `git diff --check` and `git diff --cached --check` passed on the final staged
  diff.

### Superseded failures and unverified coverage

- `cargo fmt --manifest-path backend/Cargo.toml --all -- --check` reports
  formatting differences in existing vendored WebDriver and runtime-submodule
  files. The check did not modify them; the touched Rust files passed the
  targeted formatter check above.
- Before the selection correction, the settings layout spec observed the route
  URL and page content changing while every sidebar item remained inactive.
  After the `useMatchRoute` presentation adapter and bounded visibility waits,
  the same spec passed **6/6**. Initial failure-time PNG/HTML remain under the
  ignored `tauri-e2e/tauri-e2e/.tmp/artifacts/ref-sync-final-2026-11-23/` and
  `ref-sync-active-fix-full-2026-11-23/` directories. The ref checkout's
  installed router packages are stale (`1.170.18`), while its lockfile pins
  `1.170.46`; the UI adapter remains a narrow local compatibility difference
  until the locked ref runtime can be compared.
- An earlier aggregate `test:settings` run used a stale pre-sync binary and
  reported **15/20 specs passed**; it is not evidence for the current candidate.
  The current aggregate was not run because it includes system-tools/host-proxy
  paths and this task does not authorize host-network changes.
- Real DNS resolution, Service/TUN lifecycle, system proxy behavior, cold-start
  persistence, full legacy DNS UI, and cross-platform behavior were not tested.

### Review and commit

- Independent fresh-context review of the final `04c668a` candidate passed with
  no blocking findings after the E2E cleanup corrections.
- This task record is committed with the task changes on
  `codex/ref-alignment-sync-optimize`; no push or merge is included.

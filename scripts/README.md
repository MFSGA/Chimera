# Chimera repository scripts

The `prepare:check` command verifies and prepares the binaries required for the desktop build. From the repository root, use `pnpm run prepare:check` (Node.js 24). It may download resources or replace sidecars; do not run it against a live installation merely to execute unit tests.

The other available verification tasks are `pnpm run typecheck:prepare`, `pnpm run test:prepare:node`, and `pnpm run test:manifest`. The existing Deno test catalog is retained during the remaining scripts migration.

## CI changed-path classification: Node.js upstream alignment

**Reference:** local `ref/scripts/src/ci/changed-parts.ts` at `ref@a01c6e3e8`. **Risk:** L3 (CI routing); no change to CI triggers, permissions, branch protection, or release jobs.

The `changes` job now invokes `pnpm run check:changed-parts "$BASE"` on Node.js 24. Its implementation uses the upstream `shared/command.ts` and `shared/repo-paths.ts` while **preserving Chimera's ordered `CHANGE_RULES`**, including root `package.json` and lockfile triggering all three parts, `tauri-e2e/**` triggering frontend, and `backend/**` triggering backend. No new dependencies or `pnpm-lock.yaml` changes are required.

Run `pnpm run test:changed-parts` (`node:test`) and `pnpm run typecheck:ci` for this specific module. The unit tests cover path classification, selection union, Git base handling, exact multiline `GITHUB_OUTPUT` appending, and failure on an invalid Git base. The existing three-platform `test_prepare` matrix also runs these tests on script changes; real resource downloads remain gated by backend changes.

**Non-goals:** do not modify product runtime, regenerate manifests, retag, publish, run actual sidecar downloads, or change Deno-backed release and nightly tools. **Rollback:** restore `changed-parts.ts`, `changed-parts_test.ts`, the root task entries, `scripts/tsconfig.ci.json` and the corresponding `ci.yaml` steps. Preserve unrelated `pnpm-lock.yaml` and `ref/` edits.

## Data-slot generator: Node.js upstream migration

**Reference:** `ref/scripts/src/generate/extract-data-slots.ts` at `ref@a01c6e3e8`. **Risk:** L1 for the script migration; regenerating production output is a separate behavior-changing action.

- The public `pnpm run generate:data-slots` task now invokes Node.js 24. The existing `scripts/extract-data-slots.ts` entrypoint is kept in place pending the separate directory-layout cleanup, so no other caller needs a path migration.
- Use upstream `node:fs/promises`, `node:path` and `shared/repo-paths.ts`. The only implementation substitution is Node 24's built-in `glob` instead of upstream `globby`, avoiding a new dependency and changes to the pre-existing dirty lockfile.
- Preserve the Chimera-specific `frontend/chimera/src/**/*.tsx` scan scope, exact double-quoted `data-slot` match, sort/deduplication, output path and generated source banner. Do **not** silently widen to the upstream `frontend/*/src/**/*.tsx` glob or `frontend/nyanpasu/` output path.
- `pnpm run test:data-slots` uses disposable fixture directories to verify the filtering, deduplication, ordering and byte-exact generator output. `pnpm run typecheck:data-slots` checks the Node entry and tests; both are included in the cross-platform `test_prepare` scripts matrix.
- **Existing generated file is intentionally not rewritten:** `frontend/chimera/src/generated/data-slots.gen.ts` contains roughly 22 entries, while the current TSX sources already contain roughly 446 distinct double-quoted slots. This apparent pre-existing stale generated artifact needs an independent impact review because the generated list affects custom CSS and Monaco suggestions. Tests use temporary directories; no production artifact was regenerated.

**Rollback:** revert the generator, its Node tests and scoped TS config, root scripts and corresponding CI test steps. Preserve unrelated worktree changes, including `pnpm-lock.yaml`, `ref/` and the current generated TS file.

## Manifest resolver and generator: Node.js upstream alignment

**Reference:** `ref/scripts/src/manifest/manifest.ts` and `ref/scripts/src/manifest/generate-latest-version.ts` at `ref@a01c6e3e8`. **Risk:** L2 for version/asset selection and L3 for associated CI maintenance workflow.

- The root `pnpm run generate:manifest:latest-version` task now invokes Node 24. The existing Chimera paths (`scripts/manifest.ts`, `scripts/generate-latest-version.ts`) remain compatible with callers; `manifest.ts` uses the already migrated shared logger and `process.env`, and the generator uses upstream Node filesystem APIs and `WORKSPACE_ROOT`.
- Preserve all **seven** existing resolver names, the complete Chimera/Clash/Mihomo architecture mapping, `manifest_version: 1`, and the JSON output contract. `ref`'s Meow resolver and reduced platform matrix are not suitable for Chimera's releases.
- Preserve the previous no-op behavior: if current versions and asset mappings are unchanged after removing `updated_at`, do not write the file. On resolver rejection, the existing manifest stays untouched. The injectable resolver and temporary `workspaceRoot` parameters in the generator exist solely for offline verification of the same production path.
- `pnpm run test:manifest:node` checks seven names, architecture mappings, successful writes, byte-stable unchanged content, timestamp updates only when needed, failure recovery, and branded Client/Alpha release metadata using mocked HTTP. `pnpm run typecheck:manifest` validates the Node 24 graph; both are wired into the cross-platform `test_prepare` CI job when scripts change.
- In `.github/workflows/daily.yml`, only the `master` job drops obsolete Deno setup; the `dev` job keeps its Deno environment because it checks out a different branch that may still run the old script. No permissions, tokens, checkout refs, push destinations, commit logic, or release behavior were changed.
- **Non-goals:** no live GitHub release fetch, no writing `manifest/version.json` during this migration, no automated update or publishing, and no replacement of Chimera Client/Service with ref's asset names.

**Rollback:** restore the two scripts, root command, scoped Node tests/tsconfig, and related CI checks. If needed, restore the `master` daily-job Deno setup. Do not reset the pre-existing `pnpm-lock.yaml`, `ref/`, or generated frontend slots.

## Nightly preparation and updater: Node.js alignment

**Reference:** `ref/scripts/src/release/prepare-nightly.ts` and `ref/scripts/src/updater/updater-nightly.ts` at `ref@a01c6e3e8`. **Risk:** L2 for local bundle configuration; L3 for updater publication and workflow wiring.

- `pnpm run prepare:nightly` now uses Node.js 24 and the upstream `shared/command.ts`, logger, and `node:fs/promises` APIs. The existing Chimera `--nsis`, `--msi`, `--linux`, `--disable-updater`, and `--fixed-webview` flags, WebView2 local path selection, and separate fixed-WebView updater URLs are retained. Native JSON merging matches the prior lodash merge for the relevant Tauri config graph without introducing new dependencies or touching the dirty lockfile.
- `pnpm run updater:nightly` now also uses Node 24, Octokit already declared in the scripts workspace, and `node:util` argument parsing. Preserve Chimera's normal/fixed-WebView manifest filenames and `gh-proxy.com` URL construction. The Git pre-release tag fallback now uses the full commit SHA truncated to seven characters, consistent with build preparation. `latest.json` build metadata must carry the expected SHA and a valid semantic version.
- Unlike ref, Chimera still uses `updater-manifest.ts` for asset matching and strict required-target/signature validation. Release lookup errors other than a real 404 now fail instead of creating a potentially incorrect release. Releasing and replacing assets still requires the original authenticated GitHub workflow.
- `pnpm run test:nightly:node` runs four fixture repo cases, including Linux build versions, fixed WebView2, missing runtime and missing Git. `pnpm run test:updater:nightly:node` runs fully offline payload and API-simulation tests, including bad signatures, duplicate/missing targets, fallback Git tag hash and 403/404 release lookup behavior. `pnpm run typecheck:nightly` and `pnpm run typecheck:updater:nightly` validate the two Node entrypoints.
- The cross-platform CI script matrix now executes those Node checks when scripts change. `.github/workflows/deps-create-updater.yaml` runs updater checks before publishing and drops only obsolete Deno setup; `test:nightly-build` retains the separate legacy Deno macOS updater-rename fixture until that module migrates.
- **Not performed:** no real GitHub Release mutation, signing, CDN upload, updater publication, or host installer test. These are fixture-level tests, not proof that the production release workflow has completed. L3 workflow changes require review before merging.

**Rollback:** restore `prepare-nightly.ts`, `updater-nightly.ts`, their tests and scoped tsconfigs, root commands, and related CI job steps. Preserve `pnpm-lock.yaml`, `ref/`, `manifest/version.json`, `frontend/chimera/src/generated/data-slots.gen.ts`, and unrelated working files.

---

# Prepare resource check: Node.js upstream alignment

**Reference:** local `ref@` `5a2e2baba3bdf2c3db3b9ffe1c941ac45dfa9ba5` (unmodified).
**Base:** `master@` `e07255bb`. **Risk:** L2 (resource downloads, integrity, and sidecar placement).

## Objective and ref mapping

- Align `scripts/src/prepare/check.ts`, `download.ts`, `binaries.ts`, `progress.ts`, and `resources.ts` to upstream Node.js entrypoints and Node filesystem/process APIs while preserving Chimera-specific resource contracts.
- Reuse `ref/scripts/src/shared/` source for `command.ts`, `repo-paths.ts`, `logger.ts`, `path-exists.ts`, and `errors.ts`.
- Keep root `pnpm run prepare:check` as the public task, now invoking Node directly; add the targeted `typecheck:prepare` and `test:prepare:node` tasks.
- Keep source ownership, install destinations and existing Core/Service architecture unchanged. No updates to `ref/` or any existing `pnpm-lock.yaml` modifications.

## Necessary Chimera divergences

- Preserve `chimera-client`, `chimera-client-alpha`, `chimera-service` asset naming, release endpoints, manifest schemas, checksum URLs, and source-based version lookup. Do not copy upstream Meow or Nyanpasu Service asset list.
- Preserve `tasks.ts` and `retry.ts` specialized retry policy, including rolling Alpha 404 and bounded 408/429/5xx delays; retain Wintun verification and existing country.mmdb/geoip/geosite resources.
- Port the existing Chimera atomic sidecar installer from `scripts/deno/utils/sidecar-integrity.ts` into `scripts/src/prepare/sidecar-integrity.ts` using Node streams/filesystem; preserve checksum validation **before** installing and version-stamp write after successful installation.
- Preserve the sidecar cache policy. Unlike upstream, a version-managed Chimera target must have a valid installed binary _and_ matching version stamp. Failed download, unpack or checksum must not replace the previous installed file.
- Keep the existing Deno test suite temporarily for migration cross-checks. Node-specific tests use `node:test` and `scripts/tsconfig.prepare.json`, scoped to the migrated module graph, until the remaining scripts and tests are migrated.

## Verification and limits

- `pnpm run typecheck:prepare` checks Node 24 type-stripping compatibility of the executable modules.
- `pnpm run test:prepare:node` validates Node module entry, streamed file output, ZIP/gzip extraction, interrupted download cache preservation, checksum rejection, version-stamp success and cache reuse.
- `pnpm run test:manifest` confirms all 33 existing prepare/manifest tests still pass on Deno.
- Run `pnpm exec prettier --check` and `git diff --check` for the touched files.
- **Live CDN fetching, privileged system changes, bundled Windows/Mac/Linux application E2E and actual installer launches have not been run.** Fixtures and unit tests do not substitute for those checks.
- Rollback this task's `scripts/src/prepare/` Node adapter files, `scripts/src/shared/`, `scripts/tsconfig.prepare.json`, root package task entries and this document only. Preserve any unrelated `pnpm-lock.yaml` and `ref/` work.

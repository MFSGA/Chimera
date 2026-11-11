# Chimera repository scripts

The `prepare:check` command verifies and prepares the binaries required for the desktop build. From the repository root, use `pnpm run prepare:check` (Node.js 24). It may download resources or replace sidecars; do not run it against a live installation merely to execute unit tests.

The other available verification tasks are `pnpm run typecheck:prepare`, `pnpm run test:prepare:node`, and `pnpm run test:manifest`. The existing Deno test catalog is retained during the remaining scripts migration.

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

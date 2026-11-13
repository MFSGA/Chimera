import { mkdir, readFile, writeFile } from 'node:fs/promises';
import * as path from 'node:path';
import {
  resolveChimeraClient,
  resolveChimeraClientAlpha,
  resolveClashPremium,
  resolveClashRs,
  resolveClashRsAlpha,
  resolveMihomo,
  resolveMihomoAlpha,
  type LatestVersionResolver,
} from './manifest.ts';
import type { ArchMapping } from './src/prepare/types.ts';
import { colorize, consola } from './src/shared/logger.ts';
import { WORKSPACE_ROOT } from './src/shared/repo-paths.ts';

// === Constants ===

const MANIFEST_VERSION = 1;

// === Types ===

type SupportedCore =
  | 'mihomo'
  | 'mihomo_alpha'
  | 'clash_rs'
  | 'chimera_client'
  | 'chimera_client_alpha'
  | 'clash_rs_alpha'
  | 'clash_premium';

export interface ManifestVersion {
  manifest_version: number;
  latest: Record<SupportedCore, string>;
  arch_template: Record<SupportedCore, ArchMapping>;
  updated_at?: string;
}

type Resolver = () => LatestVersionResolver;

const resolvers: readonly Resolver[] = [
  resolveMihomo,
  resolveMihomoAlpha,
  resolveClashRs,
  resolveChimeraClient,
  resolveChimeraClientAlpha,
  resolveClashPremium,
  resolveClashRsAlpha,
];

export function buildVersionManifest(
  results: Awaited<LatestVersionResolver>[],
): ManifestVersion {
  const manifest: ManifestVersion = {
    manifest_version: MANIFEST_VERSION,
    latest: {} as Record<SupportedCore, string>,
    arch_template: {} as Record<SupportedCore, ArchMapping>,
  };

  for (const result of results) {
    manifest.latest[result.name as SupportedCore] = result.version;
    manifest.arch_template[result.name as SupportedCore] = result.archMapping;
  }
  return manifest;
}

// The production entrypoint uses the upstream resolver order and Node file APIs.
// Optional dependencies allow fixture-based verification without GitHub requests
// or mutations to the checked-in manifest/version.json.
export async function generateLatestVersion(
  options: {
    workspaceRoot?: string;
    resolve?: readonly Resolver[];
    now?: () => string;
  } = {},
): Promise<{ changed: boolean; manifest: ManifestVersion }> {
  const workspaceRoot = options.workspaceRoot ?? WORKSPACE_ROOT;
  const manifestDir = path.join(workspaceRoot, 'manifest');
  const manifestPath = path.join(manifestDir, 'version.json');

  consola.start(colorize`{cyan Resolving} latest versions`);
  const results = await Promise.all(
    (options.resolve ?? resolvers).map((r) => r()),
  );
  consola.success('Resolved latest versions');
  consola.start('Generating manifest');

  const manifest = buildVersionManifest(results);
  await mkdir(manifestDir, { recursive: true });

  // Ref semantics: timestamp alone must not trigger a rewrite.
  let previousManifest: Partial<ManifestVersion> = {};
  try {
    previousManifest = JSON.parse(await readFile(manifestPath, 'utf8'));
    delete previousManifest.updated_at;
  } catch {
    // The manifest may not exist yet.
  }

  if (JSON.stringify(previousManifest) === JSON.stringify(manifest)) {
    consola.success('No changes, skip writing manifest');
    return { changed: false, manifest };
  }

  manifest.updated_at = (options.now ?? (() => new Date().toISOString()))();
  consola.success('Generated manifest');
  await writeFile(manifestPath, JSON.stringify(manifest, null, 2) + '\n');
  consola.success('Manifest written');
  return { changed: true, manifest };
}

if (import.meta.main) {
  await generateLatestVersion();
}

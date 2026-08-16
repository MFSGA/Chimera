import { randomUUID } from 'node:crypto';
import fs from 'node:fs';
import path from 'node:path';

export const DEFAULT_RUNTIME_MAX_AGE_MS = 24 * 60 * 60 * 1000;

/** Resolve one isolated runtime directory for a complete E2E run. */
export function resolveRuntimeDirectory(
  runtimeRootDirectory: string,
  override?: string,
  runId: string = randomUUID(),
): string {
  if (override) {
    return path.resolve(override);
  }

  return path.join(runtimeRootDirectory, runId);
}

function isDirectChild(rootDirectory: string, candidate: string): boolean {
  const root = path.resolve(rootDirectory);
  const target = path.resolve(candidate);
  const relative = path.relative(root, target);
  return (
    relative.length > 0 &&
    relative !== '..' &&
    !relative.startsWith(`..${path.sep}`) &&
    !path.isAbsolute(relative) &&
    !relative.includes(path.sep)
  );
}

/**
 * Remove one generated E2E runtime directory without ever deleting the root,
 * an outside path, a nested arbitrary path, or a symlink.
 */
export function cleanupRuntimeDirectory(
  runtimeRootDirectory: string,
  runtimeDirectory: string,
): boolean {
  if (!isDirectChild(runtimeRootDirectory, runtimeDirectory)) return false;
  if (!fs.existsSync(runtimeDirectory)) return false;

  const stat = fs.lstatSync(runtimeDirectory);
  if (!stat.isDirectory() || stat.isSymbolicLink()) return false;

  fs.rmSync(runtimeDirectory, {
    recursive: true,
    force: true,
    maxRetries: 4,
    retryDelay: 250,
  });
  return true;
}

export interface RuntimePruneOptions {
  olderThanMs?: number;
  now?: number;
  exclude?: readonly string[];
}

/** Remove stale direct-child runtime directories left behind by interrupted runs. */
export function pruneRuntimeDirectories(
  runtimeRootDirectory: string,
  options: RuntimePruneOptions = {},
): string[] {
  const root = path.resolve(runtimeRootDirectory);
  if (!fs.existsSync(root)) return [];

  const rootStat = fs.lstatSync(root);
  if (!rootStat.isDirectory() || rootStat.isSymbolicLink()) return [];

  const olderThanMs = options.olderThanMs ?? DEFAULT_RUNTIME_MAX_AGE_MS;
  const cutoff = (options.now ?? Date.now()) - Math.max(0, olderThanMs);
  const excluded = new Set(
    (options.exclude ?? []).map((entry) => path.resolve(entry)),
  );
  const removed: string[] = [];

  for (const entry of fs.readdirSync(root, { withFileTypes: true })) {
    const candidate = path.join(root, entry.name);
    if (excluded.has(path.resolve(candidate))) continue;
    if (!entry.isDirectory() || entry.isSymbolicLink()) continue;

    const stat = fs.lstatSync(candidate);
    if (stat.mtimeMs > cutoff) continue;

    if (cleanupRuntimeDirectory(root, candidate)) {
      removed.push(candidate);
    }
  }

  return removed;
}

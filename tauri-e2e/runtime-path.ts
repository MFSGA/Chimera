import { randomUUID } from 'node:crypto';
import path from 'node:path';

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

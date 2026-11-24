import { mkdtemp, rm } from 'node:fs/promises';
import { tmpdir } from 'node:os';
import * as path from 'node:path';

export async function withTempFetch(
  run: (root: string) => Promise<void>,
): Promise<void> {
  const root = await mkdtemp(path.join(tmpdir(), 'chimera-prepare-'));
  const originalFetch = globalThis.fetch;
  let testFailed = false;
  let testError: unknown;

  try {
    await run(root);
  } catch (error) {
    testFailed = true;
    testError = error;
  }

  const cleanupErrors: unknown[] = [];
  try {
    globalThis.fetch = originalFetch;
  } catch (error) {
    cleanupErrors.push(error);
  }
  try {
    await rm(root, { recursive: true, force: true });
  } catch (error) {
    cleanupErrors.push(error);
  }

  if (testFailed && cleanupErrors.length > 0) {
    throw new AggregateError(
      [testError, ...cleanupErrors],
      'test and temporary-resource cleanup both failed',
    );
  }
  if (testFailed) throw testError;
  if (cleanupErrors.length === 1) throw cleanupErrors[0];
  if (cleanupErrors.length > 1) {
    throw new AggregateError(
      cleanupErrors,
      'temporary-resource cleanup failed',
    );
  }
}

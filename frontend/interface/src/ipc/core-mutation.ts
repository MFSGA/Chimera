import { unwrapResult } from '../utils/index.js';
import type { commands } from './bindings.js';

export type RestartSidecarCommand = Pick<typeof commands, 'restartSidecar'>;

export async function restartCoreSidecar(
  coreCommands: RestartSidecarCommand,
  onSettled?: () => void,
): Promise<void> {
  try {
    unwrapResult(await coreCommands.restartSidecar());
  } finally {
    // RPC failure may happen after CoreActor accepted the submission.
    // Refresh its authoritative projection on both success and failure.
    onSettled?.();
  }
}

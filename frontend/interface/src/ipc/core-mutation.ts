import { unwrapResult } from '../utils/index.js';
import type { commands } from './bindings.js';

export type RestartSidecarCommand = Pick<typeof commands, 'restartSidecar'>;

export async function restartCoreSidecar(
  coreCommands: RestartSidecarCommand,
): Promise<void> {
  unwrapResult(await coreCommands.restartSidecar());
}

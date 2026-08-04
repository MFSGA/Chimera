import { unwrapResult } from '../utils';
import type { commands } from './bindings';

export type RestartSidecarCommand = Pick<typeof commands, 'restartSidecar'>;

export async function restartCoreSidecar(
  coreCommands: RestartSidecarCommand,
): Promise<void> {
  unwrapResult(await coreCommands.restartSidecar());
}

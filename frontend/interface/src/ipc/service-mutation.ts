import { unwrapResult } from '../utils';
import type { commands } from './bindings';

export type ServiceType =
  'install' | 'uninstall' | 'start' | 'stop' | 'restart';

export type ServiceMutationCommands = Pick<
  typeof commands,
  | 'installService'
  | 'uninstallService'
  | 'startService'
  | 'stopService'
  | 'restartService'
>;

export async function executeServiceMutation(
  serviceCommands: ServiceMutationCommands,
  type: ServiceType,
): Promise<void> {
  switch (type) {
    case 'install':
      unwrapResult(await serviceCommands.installService());
      return;
    case 'uninstall':
      unwrapResult(await serviceCommands.uninstallService());
      return;
    case 'start':
      unwrapResult(await serviceCommands.startService());
      return;
    case 'stop':
      unwrapResult(await serviceCommands.stopService());
      return;
    case 'restart':
      unwrapResult(await serviceCommands.restartService());
      return;
    default:
      throw new Error(`Unsupported service mutation: ${String(type)}`);
  }
}

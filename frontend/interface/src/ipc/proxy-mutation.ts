import { unwrapResult } from '../utils';
import type { commands } from './bindings';

export type SelectProxyCommand = Pick<typeof commands, 'selectProxy'>;

export async function selectProxyAndRefresh(
  proxyCommands: SelectProxyCommand,
  groupName: string,
  proxyName: string,
  refetch: () => Promise<unknown>,
): Promise<void> {
  unwrapResult(await proxyCommands.selectProxy(groupName, proxyName));
  await refetch();
}

import { unwrapResult } from '../utils';
import { commands } from './bindings';

export async function flushSystemDnsCache(): Promise<void> {
  unwrapResult(await commands.flushSystemDnsCache());
}

import type {
  DelayHistory,
  NativeProxies_Serialize,
  Proxy_Serialize,
} from './bindings.js';

/** The default health-check source is an opaque key, never a test URL. */
export const DEFAULT_DELAY_KEY = '';

export type DelayKeySource = { delayKey: string };
export type DelayKeyedProxies = NativeProxies_Serialize & {
  /** Complete source keys, including hidden groups filtered out of the UI. */
  delayKeys?: Readonly<Record<string, string>>;
};

export function collectDelayKeys(
  proxies: NativeProxies_Serialize,
): Record<string, string> {
  return Object.fromEntries(
    [...(proxies.global ? [proxies.global] : []), ...proxies.groups].map(
      (group) => [group.name, group.delayKey],
    ),
  );
}

const delayKeysBySnapshot = new WeakMap<
  NativeProxies_Serialize,
  Readonly<Record<string, string>>
>();

export function groupDelayKey(
  proxies: DelayKeyedProxies | undefined,
  group: string | undefined,
): string {
  if (!proxies || group === undefined) return DEFAULT_DELAY_KEY;
  let keys = proxies.delayKeys ?? delayKeysBySnapshot.get(proxies);
  if (!keys) {
    keys = collectDelayKeys(proxies);
    delayKeysBySnapshot.set(proxies, keys);
  }
  return Object.hasOwn(keys, group) ? keys[group] : DEFAULT_DELAY_KEY;
}

/** The same source precedence as ref's nodeDelayHistory. */
export function nodeDelayHistory(
  node: Proxy_Serialize,
  delayKey: string,
): DelayHistory[] {
  return node.extra?.[delayKey]?.history ?? node.history;
}

export function latestDelay(
  node: Proxy_Serialize | undefined,
  delayKey: string,
): number | undefined {
  return node ? nodeDelayHistory(node, delayKey).at(-1)?.delay : undefined;
}

export type ProxyChain = {
  path: string[];
  leaf?: Proxy_Serialize;
  parent: string;
};

/** Follow the selected member until a non-group leaf, missing node or cycle. */
export function resolveProxyChain(
  start: string,
  proxies: NativeProxies_Serialize,
): ProxyChain {
  const visited = new Set([start]);
  const path: string[] = [];
  let parent = start;
  let name = proxies.nodes[start]?.now;

  while (name && !visited.has(name)) {
    const node = proxies.nodes[name];
    path.push(name);
    if (!node) return { path, parent };
    if (!node.all) return { path, leaf: node, parent };
    visited.add(name);
    parent = name;
    name = node.now;
  }

  return { path, parent };
}

export type NativeMemberState = {
  delay: number | undefined;
  leaf?: string;
  history: DelayHistory[];
};

const NO_HISTORY: DelayHistory[] = [];

/**
 * Mirror ref's memberState: a nested group reports its resolved leaf's latest
 * sample, using the directly containing group's opaque delay source key.
 */
export function nativeMemberState(
  member: string,
  group: DelayKeySource,
  proxies: DelayKeyedProxies,
): NativeMemberState {
  const node = proxies.nodes[member];
  if (!node?.all) {
    const history = node ? nodeDelayHistory(node, group.delayKey) : NO_HISTORY;
    return { delay: history.at(-1)?.delay, history };
  }

  const chain = resolveProxyChain(member, proxies);
  const history = chain.leaf
    ? nodeDelayHistory(chain.leaf, groupDelayKey(proxies, chain.parent))
    : NO_HISTORY;
  return { delay: history.at(-1)?.delay, leaf: chain.leaf?.name, history };
}

export function nativeMemberDelay(
  member: string,
  group: DelayKeySource,
  proxies: DelayKeyedProxies,
): number | undefined {
  return nativeMemberState(member, group, proxies).delay;
}

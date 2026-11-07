import type { NativeProxies_Serialize } from './bindings.js';

/** Mirrors ref's bound for testing members of an automatically pinned group. */
export const PINNED_GROUP_DELAY_CONCURRENCY = 8;

export type NativeGroupDelayApi = {
  /** An uncached controller read; a pin may have changed in another window. */
  refresh: () => Promise<NativeProxies_Serialize>;
  groupDelay: (
    group: string,
    url: string | null,
    expected: string | null,
  ) => Promise<Record<string, number>>;
  proxyDelay: (
    name: string,
    provider: string | null,
    url: string | null,
    expected: string | null,
  ) => Promise<number>;
};

/**
 * Use one group request unless the latest controller snapshot has a pin.
 * Mihomo/Meow group healthchecks can clear fixed selections, so pinned
 * groups test members individually without changing their selection.
 *
 * Every member is submitted at most once, with bounded concurrency.
 * Individual failures become zero delay; failure to read the controller
 * snapshot is fatal, because guessing the pin state would be destructive.
 */
export async function testNativeGroupDelay(
  groupName: string,
  api: NativeGroupDelayApi,
): Promise<Record<string, number>> {
  const graph = await api.refresh();
  const group =
    graph.global?.name === groupName
      ? graph.global
      : graph.groups.find((item) => item.name === groupName);

  if (!group) {
    throw new Error(
      `Proxy group "${groupName}" is missing from the current controller`,
    );
  }

  const url = group.testUrl ?? null;
  const expected = group.expectedStatus ?? null;

  if (!group.fixed) {
    return api.groupDelay(groupName, url, expected);
  }

  const delays: Record<string, number> = {};
  const names = group.all;
  let next = 0;
  const worker = async () => {
    while (next < names.length) {
      const name = names[next]!;
      next += 1;
      const node = graph.nodes[name];
      const provider = node?.provider ?? node?.['provider-name'] ?? null;
      try {
        delays[name] = await api.proxyDelay(name, provider, url, expected);
      } catch {
        // The controller may record failed tests as zero. Never retry a
        // request after an ambiguous network failure.
        delays[name] = 0;
      }
    }
  };

  await Promise.all(
    Array.from(
      { length: Math.min(PINNED_GROUP_DELAY_CONCURRENCY, names.length) },
      () => worker(),
    ),
  );
  return delays;
}

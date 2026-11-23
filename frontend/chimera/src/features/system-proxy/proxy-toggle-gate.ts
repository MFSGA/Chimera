/** One write per setting across all shortcut instances and settings pages.
 * No attempt is made while the persisted value is still unknown. */
export function createProxyToggleGate() {
  const pending = new Set<string>();
  return {
    async run(
      key: 'system' | 'tun',
      current: boolean | undefined,
      commit: (next: boolean) => Promise<unknown>,
      refresh: () => Promise<unknown>,
    ): Promise<boolean> {
      if (current === undefined || pending.has(key)) return false;
      pending.add(key);
      try {
        await commit(!current);
        // The next click must not reuse a stale value left in the query cache.
        await refresh();
        return true;
      } finally {
        pending.delete(key);
      }
    },
  };
}

export const proxyToggleGate = createProxyToggleGate();

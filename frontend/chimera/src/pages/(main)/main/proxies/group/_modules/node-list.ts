import {
  nativeMemberDelay,
  type NativeProxies_Serialize,
  type Proxy_Serialize,
  type ProxyGroup,
} from '@chimera/interface';

/** Main Proxies compatibility export with ref's resolved member semantics. */
export { nativeMemberDelay as memberDelay } from '@chimera/interface';

export const NODE_SORTS = ['default', 'name', 'delay'] as const;
export type NodeSort = (typeof NODE_SORTS)[number];
export type NodeView = { sort: NodeSort; hideUnavailable: boolean };

export const DEFAULT_NODE_VIEW: NodeView = {
  sort: 'default',
  hideUnavailable: false,
};
export const NODE_VIEW_KV_KEY = 'proxies-node-view';

const collator = new Intl.Collator(undefined, {
  numeric: true,
  sensitivity: 'base',
});

/** Coerce the persisted view to known options, as in ref. */
export function toNodeView(value: unknown): NodeView {
  const stored = (value ?? {}) as Partial<Record<keyof NodeView, unknown>>;
  return {
    sort: NODE_SORTS.includes(stored.sort as NodeSort)
      ? (stored.sort as NodeSort)
      : DEFAULT_NODE_VIEW.sort,
    hideUnavailable:
      typeof stored.hideUnavailable === 'boolean'
        ? stored.hideUnavailable
        : DEFAULT_NODE_VIEW.hideUnavailable,
  };
}

const searchTerms = (query: string) =>
  query.toLowerCase().split(/\s+/).filter(Boolean);

export function matchesNodeSearch(node: Proxy_Serialize, query: string) {
  const text = `${node.name} ${node.type}`.toLowerCase();
  return searchTerms(query).every((term) => text.includes(term));
}

const delayRank = (delay: number | undefined) =>
  delay === undefined
    ? Number.MAX_SAFE_INTEGER - 1
    : delay <= 0
      ? Number.MAX_SAFE_INTEGER
      : delay;

/** Ref's stable node view over Chimera's native proxy graph. */
export function visibleMembers({
  group,
  proxies,
  query,
  view,
  delayOf,
}: {
  group: ProxyGroup;
  proxies: NativeProxies_Serialize;
  query: string;
  view: NodeView;
  /** Reuse the page's single computed member-state snapshot, like ref. */
  delayOf?: (name: string) => number | undefined;
}): string[] {
  const members: { name: string; delay: number | undefined }[] = [];
  const needsDelay = view.hideUnavailable || view.sort === 'delay';

  for (const name of group.all) {
    const node = proxies.nodes[name];
    if (!node || !matchesNodeSearch(node, query)) continue;

    const delay = needsDelay
      ? delayOf
        ? delayOf(name)
        : nativeMemberDelay(name, group, proxies)
      : undefined;
    if (
      view.hideUnavailable &&
      name !== group.now &&
      delay !== undefined &&
      delay <= 0
    ) {
      continue;
    }
    members.push({ name, delay });
  }

  switch (view.sort) {
    case 'name':
      return members.map(({ name }) => name).sort(collator.compare);
    case 'delay':
      return members
        .map((member, index) => ({ ...member, index }))
        .sort(
          (a, b) =>
            delayRank(a.delay) - delayRank(b.delay) || a.index - b.index,
        )
        .map(({ name }) => name);
    default:
      return members.map(({ name }) => name);
  }
}

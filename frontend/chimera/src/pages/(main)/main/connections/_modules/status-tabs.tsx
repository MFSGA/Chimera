import {
  useClashConnections,
  useTrafficActiveConnectionIds,
  useTrafficReport,
  useTrafficSummary,
  type TrafficRange,
  type TrafficScope,
} from '@chimera/interface';
import { cn } from '@chimera/ui';
import { ToggleGroup as ToggleGroupPrimitive } from 'radix-ui';
import * as m from '@/paraglide/messages';
import { toTrafficFilters } from '../../_modules/traffic-filters';
import type { ConnectionsSelection } from './connections-filters';

function CountBadge({ count }: { count?: number }) {
  if (count === undefined) {
    return null;
  }

  return (
    <span
      className={cn(
        'ml-1.5 inline-block rounded-full px-1.5 text-[11px] leading-4 tabular-nums',
        'bg-on-surface/8 text-on-surface-variant',
        'group-data-[state=on]:bg-on-secondary-container/12',
        'group-data-[state=on]:text-on-secondary-container',
      )}
    >
      {count.toLocaleString()}
    </span>
  );
}

function StatusTabs({
  value,
  onValueChange,
  activeCount,
  closedCount,
}: {
  value: TrafficScope;
  onValueChange: (value: TrafficScope) => void;
  activeCount?: number;
  closedCount?: number;
}) {
  return (
    <ToggleGroupPrimitive.Root
      type="single"
      className="bg-surface-variant/40 dark:bg-surface-variant/15 h-9 w-auto shrink-0 rounded-full p-0.5"
      value={value}
      onValueChange={(next) => {
        if (next === 'all' || next === 'active' || next === 'closed') {
          onValueChange(next);
        }
      }}
      aria-label={m.connections_scope_label()}
    >
      {(
        [
          ['all', m.connections_tab_all(), undefined],
          ['active', m.connections_tab_active(), activeCount],
          ['closed', m.connections_tab_closed(), closedCount],
        ] as const
      ).map(([scope, label, count]) => (
        <ToggleGroupPrimitive.Item
          key={scope}
          value={scope}
          data-slot="connections-scope-tab"
          data-scope={scope}
          className={cn(
            'group h-8 cursor-pointer rounded-full px-4 text-xs font-medium whitespace-nowrap outline-hidden',
            'text-on-surface-variant data-[state=on]:bg-secondary-container',
            'data-[state=on]:text-on-secondary-container',
          )}
        >
          {label}
          {scope !== 'all' && <CountBadge count={count} />}
        </ToggleGroupPrimitive.Item>
      ))}
    </ToggleGroupPrimitive.Root>
  );
}

/** Fetches route-aware counts separately so samples do not rerender the table. */
export default function ConnectionsStatusTabs({
  value,
  onValueChange,
  selection,
}: {
  value: TrafficScope;
  onValueChange: (value: TrafficScope) => void;
  selection: ConnectionsSelection;
}) {
  const { data: samples } = useClashConnections();
  const { data: summary } = useTrafficSummary();
  const filters = toTrafficFilters(selection.filters);
  const filtered = selection.range !== undefined || filters.length > 0;

  const { data: ids } = useTrafficActiveConnectionIds(filters, {
    enabled: filtered,
  });

  const { data: report } = useTrafficReport(
    {
      query: { range: selection.range ?? 'all', scope: 'closed', filters },
      metric: 'connections',
      rankings: [],
      ranking_limit: 1,
      topology: null,
    },
    { enabled: filtered },
  );

  const activeCount = filtered
    ? ids?.length
    : samples?.at(-1)?.connections?.length;
  const closedCount = filtered
    ? report?.total.connections
    : summary?.closed_connections;

  return (
    <StatusTabs
      value={value}
      onValueChange={onValueChange}
      activeCount={activeCount}
      closedCount={closedCount}
    />
  );
}

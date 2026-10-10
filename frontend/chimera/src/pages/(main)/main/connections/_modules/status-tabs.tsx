import {
  useClashConnections,
  useTrafficActiveConnectionIds,
  useTrafficReport,
  useTrafficSummary,
  type TrafficScope,
} from '@chimera/interface';
import { cn } from '@chimera/ui';
import {
  SegmentedButton,
  SegmentedButtonItem,
} from '@/components/ui/segmented-button';
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
    <SegmentedButton
      variant="tabs"
      size="sm"
      className="w-auto shrink-0"
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
        <SegmentedButtonItem
          key={scope}
          value={scope}
          data-slot="connections-scope-tab"
          data-scope={scope}
          className={cn('flex-none whitespace-nowrap')}
        >
          {label}
          {scope !== 'all' && <CountBadge count={count} />}
        </SegmentedButtonItem>
      ))}
    </SegmentedButton>
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

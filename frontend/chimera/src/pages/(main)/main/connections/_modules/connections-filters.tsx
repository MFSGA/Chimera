import type { TrafficRange } from '@chimera/interface';
import * as m from '@/paraglide/messages';
import FilterChips, { type FilterChipItem } from '../../_modules/filter-chips';
import {
  dimensionName,
  rangeName,
  type SearchFilter,
} from '../../_modules/traffic-filters';

export interface ConnectionsSelection {
  range?: TrafficRange;
  filters: SearchFilter[];
}

/** Shows route filters as removable chips, matching the reference toolbar. */
export default function ConnectionsFilters({
  selection,
  onSelectionChange,
  className,
}: {
  selection: ConnectionsSelection;
  onSelectionChange: (selection: ConnectionsSelection) => void;
  className?: string;
}) {
  const { range, filters } = selection;
  const items: FilterChipItem[] = filters.map((filter) => {
    const dimension = dimensionName(filter.d);
    const title = m.connections_filter_remove({
      dimension,
      value: filter.v,
    });

    return {
      key: filter.d,
      label: `${dimension}: ${filter.v}`,
      title,
      onRemove: () =>
        onSelectionChange({
          range,
          filters: filters.filter((candidate) => candidate !== filter),
        }),
    };
  });

  if (range !== undefined) {
    const dimension = m.connections_range_label();
    const value = rangeName(range);
    items.push({
      key: 'range',
      label: `${dimension}: ${value}`,
      title: m.connections_filter_remove({ dimension, value }),
      onRemove: () => onSelectionChange({ range: undefined, filters }),
    });
  }

  return (
    <FilterChips
      className={className}
      data-slot="connections-filters"
      items={items}
      onClearAll={() => onSelectionChange({ range: undefined, filters: [] })}
    />
  );
}

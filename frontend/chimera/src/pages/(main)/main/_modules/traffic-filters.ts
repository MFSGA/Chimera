import type {
  Dimension,
  TrafficFilter,
  TrafficRange,
} from '@chimera/interface';
import { z } from 'zod';
import * as m from '@/paraglide/messages';

export const RANGES = [
  'last_hour',
  'last6_hours',
  'last24_hours',
  'last7_days',
  'last30_days',
  'all',
] as const satisfies readonly TrafficRange[];

export const DIMENSIONS = [
  'origin',
  'process',
  'source',
  'inbound',
  'target',
  'protocol',
  'rule',
  'chain',
  'exit',
  'profile',
  'source_region',
  'destination_region',
  'destination_basis',
] as const satisfies readonly Dimension[];

export const searchFilterSchema = z.object({
  d: z.enum(DIMENSIONS),
  v: z.string(),
});

export type SearchFilter = z.infer<typeof searchFilterSchema>;

/** Adds a filter; another value of the same dimension is replaced. */
export function setFilter(
  filters: SearchFilter[],
  dimension: Dimension,
  value: string,
): SearchFilter[] {
  const next = { d: dimension, v: value };

  return filters.some((filter) => filter.d === dimension)
    ? filters.map((filter) => (filter.d === dimension ? next : filter))
    : [...filters, next];
}

/** The filter the row stands for: set it, or remove it if it is already set. */
export function toggleFilter(
  filters: SearchFilter[],
  dimension: Dimension,
  value: string,
): SearchFilter[] {
  return filters.some((filter) => filter.d === dimension && filter.v === value)
    ? filters.filter((filter) => filter.d !== dimension)
    : setFilter(filters, dimension, value);
}

/** The URL's short filters as the backend's filters. */
export const toTrafficFilters = (
  filters: readonly SearchFilter[],
): TrafficFilter[] => filters.map(({ d, v }) => ({ dimension: d, value: v }));

export const dimensionName = (dimension: Dimension) =>
  ({
    origin: m.connections_dimension_origin,
    process: m.connections_dimension_process,
    source: m.connections_dimension_source,
    inbound: m.connections_dimension_inbound,
    target: m.connections_dimension_target,
    protocol: m.connections_dimension_protocol,
    rule: m.connections_dimension_rule,
    chain: m.connections_dimension_chain,
    exit: m.connections_dimension_exit,
    profile: m.connections_dimension_profile,
    source_region: m.connections_dimension_source_region,
    destination_region: m.connections_dimension_destination_region,
    destination_basis: m.connections_dimension_destination_basis,
  })[dimension]();

export const rangeName = (range: TrafficRange) =>
  ({
    last_hour: m.connections_range_last_hour,
    last6_hours: m.connections_range_last6_hours,
    last24_hours: m.connections_range_last24_hours,
    last7_days: m.connections_range_last7_days,
    last30_days: m.connections_range_last30_days,
    all: m.connections_range_all,
  })[range]();

/** Identifies a rule the way its displayed label and traffic key do. */
export const ruleLabel = (type: string, payload: string) =>
  payload ? `${type},${payload}` : type;

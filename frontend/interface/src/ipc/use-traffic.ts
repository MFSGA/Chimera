import {
  useInfiniteQuery,
  useQuery,
  type UseQueryOptions,
} from '@tanstack/react-query';
import { unwrapResult } from '../utils';
import {
  commands,
  type ClosedPage,
  type Dimension,
  type Metric,
  type ReportRequest,
  type TrafficFilter,
  type TrafficQuery,
  type TrafficRange,
  type TrafficReport,
  type TrafficSummary,
  type UsageCursor,
  type UsageGroup,
  type UsagePage,
} from './bindings';

const TRAFFIC_SUMMARY_QUERY_KEY = 'traffic-summary';

export function useTrafficSummary() {
  return useQuery({
    queryKey: [TRAFFIC_SUMMARY_QUERY_KEY],
    queryFn: async () => unwrapResult(await commands.getTrafficSummary()),
    refetchInterval: 5_000,
  });
}

export function useTrafficReport(
  request: ReportRequest,
  options?: Pick<UseQueryOptions<TrafficReport, Error>, 'enabled'>,
) {
  return useQuery({
    queryKey: ['traffic-report', request],
    queryFn: async () =>
      unwrapResult(await commands.queryTrafficReport(request)),
    ...options,
  });
}

export function useTrafficActiveConnectionIds(
  filters: TrafficFilter[],
  options?: Pick<UseQueryOptions<string[], Error>, 'enabled'>,
) {
  return useQuery({
    queryKey: ['traffic-active-connection-ids', filters],
    queryFn: async () =>
      unwrapResult(await commands.queryTrafficActiveConnectionIds(filters)),
    ...options,
  });
}

export function useTrafficClosedConnections({
  range,
  filters,
  limit = 100,
  enabled = true,
}: {
  range: TrafficRange;
  filters: TrafficFilter[];
  limit?: number;
  enabled?: boolean;
}) {
  return useInfiniteQuery({
    queryKey: ['traffic-closed-connections', range, filters, limit],
    enabled,
    initialPageParam: null as ClosedPage['next'],
    queryFn: async ({ pageParam }) =>
      unwrapResult(
        await commands.queryTrafficClosedConnections(
          range,
          filters,
          pageParam,
          limit,
        ),
      ),
    getNextPageParam: (page) => page.next,
  });
}

export function useTrafficUsage(
  query: TrafficQuery,
  dimension: Dimension,
  metric: Metric,
  limit = 100,
) {
  return useInfiniteQuery({
    queryKey: ['traffic-usage', query, dimension, metric, limit],
    initialPageParam: null as UsageCursor | null,
    queryFn: async ({ pageParam }) =>
      unwrapResult(
        await commands.queryTrafficUsage(
          query,
          dimension,
          metric,
          pageParam,
          limit,
        ),
      ),
    getNextPageParam: (page: UsagePage) => page.next,
  });
}

export function useTrafficUsageByKeys(
  query: TrafficQuery,
  dimension: Dimension,
  keys: string[],
) {
  return useQuery<UsageGroup[]>({
    queryKey: ['traffic-usage-by-keys', query, dimension, keys],
    queryFn: async () =>
      unwrapResult(
        await commands.queryTrafficUsageByKeys(query, dimension, keys),
      ),
  });
}

import assert from 'node:assert/strict';
import { test } from 'node:test';
import { QueryClient, QueryObserver } from '@tanstack/react-query';
import { commands, type ReportRequest, type TrafficReport } from '../src/ipc';
import { trafficReportQueryOptions } from '../src/ipc/use-traffic';

const request: ReportRequest = {
  query: { range: 'all', scope: 'closed', filters: [] },
  metric: 'connections',
  rankings: [],
  ranking_limit: 1,
  topology: null,
};

test('traffic reports retry transient errors when no polling interval exists', async () => {
  const original = commands.queryTrafficReport;
  let calls = 0;
  commands.queryTrafficReport = async () => {
    calls += 1;
    if (calls === 1) throw new Error('temporary IPC failure');
    return { status: 'ok', data: {} as TrafficReport };
  };

  const queryClient = new QueryClient({
    defaultOptions: { queries: { retry: 1, retryDelay: 0 } },
  });
  const observer = new QueryObserver(
    queryClient,
    trafficReportQueryOptions(request),
  );

  try {
    const result = await observer.refetch();
    assert.equal(result.isSuccess, true);
    assert.equal(calls, 2);
  } finally {
    observer.destroy();
    queryClient.clear();
    commands.queryTrafficReport = original;
  }
});

test('pausing a polled traffic report disables focus, reconnect, and retry refetches', () => {
  const options = trafficReportQueryOptions(request, {
    refetchInterval: false,
  });

  assert.equal(options.refetchOnWindowFocus, false);
  assert.equal(options.refetchOnReconnect, false);
  assert.equal(options.retry, false);
});

test('active polling leaves transient retry to the next polling interval', () => {
  const options = trafficReportQueryOptions(request, {
    refetchInterval: 5_000,
  });

  assert.equal(options.refetchInterval, 5_000);
  assert.equal(options.retry, false);
  assert.equal(options.refetchOnWindowFocus, true);
  assert.equal(options.refetchOnReconnect, true);
});

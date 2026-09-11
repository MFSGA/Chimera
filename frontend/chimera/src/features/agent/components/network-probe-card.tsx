import type {
  AgentNetworkProbeRequest,
  AgentNetworkProbeResult,
} from '@chimera/interface';
import { PublicRounded } from '@mui/icons-material';
import { useState } from 'react';
import { Button } from '@/components/ui/button';
import { Card, CardContent, CardHeader } from '@/components/ui/card';
import { Input } from '@/components/ui/input';
import * as m from '@/paraglide/messages';

export function NetworkProbeCard({
  loading,
  result,
  onProbe,
}: {
  loading: boolean;
  result: AgentNetworkProbeResult | undefined;
  onProbe: (request: AgentNetworkProbeRequest) => void;
}) {
  const [url, setUrl] = useState('');
  const [expectedStatus, setExpectedStatus] = useState('');
  const [timeoutMs, setTimeoutMs] = useState('');
  const target = url.trim();
  const parsedExpectedStatus = expectedStatus
    ? Number.parseInt(expectedStatus, 10)
    : null;
  const parsedTimeoutMs = timeoutMs ? Number.parseInt(timeoutMs, 10) : null;
  const validExpectedStatus =
    parsedExpectedStatus === null ||
    (parsedExpectedStatus >= 100 && parsedExpectedStatus <= 599);
  const validTimeout =
    parsedTimeoutMs === null ||
    (parsedTimeoutMs >= 1_000 && parsedTimeoutMs <= 10_000);

  return (
    <Card variant="outline" data-slot="agent-network-probe-card">
      <CardHeader className="text-base">
        <PublicRounded />
        {m.agent_check_network()}
      </CardHeader>
      <CardContent className="gap-3">
        <p className="text-on-surface-variant text-sm">
          {m.agent_readonly_notice()}
        </p>
        <div className="grid gap-2 md:grid-cols-[minmax(0,1fr)_8rem_8rem_auto] md:items-end">
          <Input
            className="min-w-0"
            data-slot="agent-network-probe-url"
            label="URL"
            value={url}
            variant="outlined"
            onChange={(event) => setUrl(event.target.value)}
          />
          <Input
            data-slot="agent-network-probe-expected-status"
            label="Expected HTTP"
            max="599"
            min="100"
            type="number"
            value={expectedStatus}
            variant="outlined"
            onChange={(event) => setExpectedStatus(event.target.value)}
          />
          <Input
            data-slot="agent-network-probe-timeout"
            label="Timeout (ms)"
            max="10000"
            min="1000"
            step="500"
            type="number"
            value={timeoutMs}
            variant="outlined"
            onChange={(event) => setTimeoutMs(event.target.value)}
          />
          <Button
            data-slot="agent-network-probe-submit"
            disabled={!target || !validExpectedStatus || !validTimeout}
            loading={loading}
            variant="flat"
            onClick={() =>
              onProbe({
                url: target,
                expected_status: parsedExpectedStatus,
                timeout_ms: parsedTimeoutMs,
              })
            }
          >
            <PublicRounded />
            {m.agent_check_network()}
          </Button>
        </div>
        {result && (
          <div
            className="bg-surface-variant/25 grid gap-2 rounded-2xl p-3 text-sm sm:grid-cols-2"
            data-slot="agent-network-probe-result"
          >
            <span>HTTP {result.status}</span>
            <span>{result.latency_ms} ms</span>
            {result.expected_status !== null && (
              <span className="sm:col-span-2">
                Expected HTTP {result.expected_status}:{' '}
                {result.matches_expected_status ? 'matched' : 'did not match'}
              </span>
            )}
          </div>
        )}
      </CardContent>
    </Card>
  );
}

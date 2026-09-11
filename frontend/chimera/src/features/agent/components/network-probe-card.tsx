import type { AgentNetworkProbeResult } from '@chimera/interface';
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
  onProbe: (url: string) => void;
}) {
  const [url, setUrl] = useState('');
  const target = url.trim();

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
        <div className="flex flex-col gap-2 sm:flex-row sm:items-end">
          <Input
            className="min-w-0 flex-1"
            data-slot="agent-network-probe-url"
            label="URL"
            value={url}
            variant="outlined"
            onChange={(event) => setUrl(event.target.value)}
          />
          <Button
            data-slot="agent-network-probe-submit"
            disabled={!target}
            loading={loading}
            variant="flat"
            onClick={() => onProbe(target)}
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
          </div>
        )}
      </CardContent>
    </Card>
  );
}

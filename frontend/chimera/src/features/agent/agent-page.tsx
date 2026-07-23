import {
  useAgent,
  type AgentActionRequest,
  type AgentProposal,
} from '@chimera/interface';
import {
  ContentCopyRounded,
  HealthAndSafetyRounded,
  RefreshRounded,
  SmartToyRounded,
} from '@mui/icons-material';
import { writeText } from '@tauri-apps/plugin-clipboard-manager';
import { useState } from 'react';
import { Notice } from '@/components/base';
import { Button } from '@/components/ui/button';
import { Card, CardContent, CardHeader } from '@/components/ui/card';
import { AppContentScrollArea } from '@/components/ui/scroll-area';
import * as m from '@/paraglide/messages';
import { ActionPanel } from './components/action-panel';
import { FindingList, ProbeFailureList } from './components/finding-list';
import { ProposalDialog } from './components/proposal-dialog';
import { SnapshotSummary } from './components/snapshot-summary';
import { presentHealth } from './model/presenter';

export function AgentPage() {
  const agent = useAgent();
  const [proposal, setProposal] = useState<AgentProposal | null>(null);
  const snapshot = agent.snapshot.data;

  const propose = async (action: AgentActionRequest) => {
    try {
      const nextProposal = await agent.propose.mutateAsync(action);
      if (!nextProposal) {
        Notice.error(m.agent_error_title());
        return;
      }
      setProposal(nextProposal);
    } catch {
      Notice.error(m.agent_error_title());
    }
  };

  const cancelProposal = async () => {
    const current = proposal;
    setProposal(null);
    if (current) await agent.cancel.mutateAsync(current.id).catch(() => false);
  };

  const executeProposal = async () => {
    if (!proposal) return;
    try {
      await agent.execute.mutateAsync({
        proposalId: proposal.id,
        digest: proposal.digest,
      });
      setProposal(null);
      Notice.success(m.agent_action_success());
    } catch (error) {
      setProposal(null);
      if (error !== 'agent_confirmation_declined') {
        Notice.error(m.agent_error_title());
      }
    }
  };

  const copyContext = async () => {
    if (!snapshot) return;
    try {
      await writeText(JSON.stringify(snapshot, null, 2));
      Notice.success(m.agent_context_copied());
    } catch {
      Notice.error(m.agent_error_title());
    }
  };

  return (
    <AppContentScrollArea
      className="h-full overflow-hidden"
      data-slot="agent-page-scroll-area"
    >
      <main className="container mx-auto flex min-h-full w-full max-w-7xl flex-col gap-5 p-4 md:p-6">
        <header className="flex flex-col gap-3 md:flex-row md:items-center md:justify-between">
          <div>
            <div className="flex items-center gap-3">
              <SmartToyRounded className="text-primary size-8" />
              <h1 className="text-2xl font-semibold">{m.agent_title()}</h1>
            </div>
            <p className="text-on-surface-variant mt-2 max-w-3xl text-sm">
              {m.agent_subtitle()}
            </p>
          </div>
          <Button
            className="self-start"
            loading={agent.snapshot.isFetching}
            variant="flat"
            onClick={() => void agent.snapshot.refetch()}
          >
            {snapshot ? <RefreshRounded /> : <HealthAndSafetyRounded />}
            {snapshot ? m.agent_refresh() : m.agent_start_diagnostics()}
          </Button>
        </header>

        <PrivacyCard />

        {agent.snapshot.isError && <ErrorCard />}
        {snapshot && (
          <>
            <HealthCard snapshot={snapshot} onCopy={copyContext} />
            <SnapshotSummary snapshot={snapshot} />
            <div className="grid gap-4 lg:grid-cols-2">
              <FindingList findings={snapshot.findings} />
              <ProbeFailureList failures={snapshot.probe_failures} />
            </div>
            <ActionPanel
              snapshot={snapshot}
              pending={agent.propose.isPending}
              onPropose={(action) => void propose(action)}
            />
            <ContextPreview snapshot={snapshot} />
          </>
        )}
      </main>
      <ProposalDialog
        proposal={proposal}
        executing={agent.execute.isPending}
        onCancel={() => void cancelProposal()}
        onConfirm={() => void executeProposal()}
      />
    </AppContentScrollArea>
  );
}

function PrivacyCard() {
  return (
    <Card variant="raised">
      <CardHeader className="text-base">
        <HealthAndSafetyRounded />
        {m.agent_privacy_title()}
      </CardHeader>
      <CardContent className="text-on-surface-variant text-sm">
        <p>{m.agent_privacy_description()}</p>
        <p className="font-medium">{m.agent_privacy_safe_context()}</p>
      </CardContent>
    </Card>
  );
}

function ErrorCard() {
  return (
    <Card variant="outline" className="border-error text-error">
      <CardContent>{m.agent_error_title()}</CardContent>
    </Card>
  );
}

function HealthCard({
  snapshot,
  onCopy,
}: {
  snapshot: NonNullable<ReturnType<typeof useAgent>['snapshot']['data']>;
  onCopy: () => void;
}) {
  return (
    <Card variant="basic">
      <CardContent className="flex-row items-center justify-between gap-4">
        <div>
          <p className="text-on-surface-variant text-sm">
            {m.agent_health_title()}
          </p>
          <p className="mt-1 text-xl font-semibold">
            {presentHealth(snapshot.health)}
          </p>
          <p className="text-on-surface-variant mt-1 text-xs">
            {m.agent_captured_at()}:{' '}
            {new Date(snapshot.captured_at).toLocaleString()}
          </p>
        </div>
        <Button variant="stroked" onClick={onCopy}>
          <ContentCopyRounded />
          {m.agent_copy_context()}
        </Button>
      </CardContent>
    </Card>
  );
}

function ContextPreview({
  snapshot,
}: {
  snapshot: NonNullable<ReturnType<typeof useAgent>['snapshot']['data']>;
}) {
  return (
    <Card variant="outline">
      <CardContent>
        <details>
          <summary className="cursor-pointer font-medium">
            {m.agent_context_preview()}
          </summary>
          <p className="text-on-surface-variant my-3 text-sm">
            {m.agent_context_description()}
          </p>
          <pre className="bg-surface-variant/30 overflow-x-auto rounded-2xl p-3 text-xs break-all whitespace-pre-wrap">
            {JSON.stringify(snapshot, null, 2)}
          </pre>
        </details>
      </CardContent>
    </Card>
  );
}

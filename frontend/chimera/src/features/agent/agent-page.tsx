import {
  useAgent,
  type AgentActionRequest,
  type AgentIntentResolution,
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
import { AutonomyCard } from './components/autonomy-card';
import { BridgeCard } from './components/bridge-card';
import { FindingList, ProbeFailureList } from './components/finding-list';
import { HistoryCard } from './components/history-card';
import { IntentCard } from './components/intent-card';
import { IssueGuidanceCard } from './components/issue-guidance-card';
import { ProposalDialog } from './components/proposal-dialog';
import { SnapshotSummary } from './components/snapshot-summary';
import { inspectHostConnectivityCard } from './model/host-connectivity-navigation';
import { resolveExecutedReadOnlyIntent } from './model/intent-routing';
import { presentAgentError, presentHealth } from './model/presenter';
import {
  isPrivacySafeSnapshot,
  serializePrivacySafeSnapshot,
} from './model/privacy-safe-context';

export function AgentPage() {
  const agent = useAgent();
  const [proposal, setProposal] = useState<AgentProposal | null>(null);
  const [intentResolution, setIntentResolution] =
    useState<AgentIntentResolution | null>(null);
  const snapshot = agent.snapshot.data;
  const privacySafeSnapshot = snapshot
    ? serializePrivacySafeSnapshot(snapshot)
    : null;

  const propose = async (action: AgentActionRequest) => {
    try {
      const nextProposal = await agent.propose.mutateAsync(action);
      if (!nextProposal) {
        Notice.error(m.agent_error_title());
        return;
      }
      setProposal(nextProposal);
    } catch (error) {
      Notice.error(presentAgentError(error));
    }
  };

  const resolveIntent = async (text: string) => {
    try {
      const result = await agent.executeReadOnlyIntent.mutateAsync(text);
      if (!result) {
        Notice.error(m.agent_error_title());
        return;
      }
      setIntentResolution(resolveExecutedReadOnlyIntent(result));
      if (result.status === 'host_connectivity') {
        await inspectHostConnectivity();
      }
    } catch (error) {
      Notice.error(presentAgentError(error));
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
        Notice.error(presentAgentError(error));
      }
    }
  };

  const refreshDiagnostics = async () => {
    await agent.snapshot.refetch();
    await agent.history.refetch();
  };

  const inspectHostConnectivity = () =>
    inspectHostConnectivityCard({
      refresh: () => agent.snapshot.refetch(),
      schedule: requestAnimationFrame,
      findCard: () => document.getElementById('agent-host-connectivity-card'),
    });

  const copyContext = async () => {
    if (!privacySafeSnapshot) {
      Notice.error(m.agent_context_privacy_blocked());
      return;
    }
    try {
      await writeText(privacySafeSnapshot);
      Notice.success(m.agent_context_copied());
    } catch (error) {
      Notice.error(presentAgentError(error));
    }
  };

  const authorizeAutonomy = async () => {
    try {
      const result = await agent.authorizeAutonomy.mutateAsync({
        schema_version: 1,
        scope: 'current_desktop_session',
        allowlist: ['reconnect_telemetry'],
        duration_seconds: 10 * 60,
        max_actions: 3,
      });
      if (result.status === 'rejected') {
        Notice.error(m.agent_error_title());
      }
    } catch (error) {
      Notice.error(presentAgentError(error));
    }
  };

  const revokeAutonomy = async () => {
    try {
      await agent.revokeAutonomy.mutateAsync();
    } catch (error) {
      Notice.error(presentAgentError(error));
    }
  };

  const clearHistory = async () => {
    try {
      await agent.clearHistory.mutateAsync();
      Notice.success(m.agent_history_cleared());
    } catch (error) {
      if (error !== 'agent_confirmation_declined') {
        Notice.error(presentAgentError(error));
      }
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
            onClick={() => void refreshDiagnostics()}
          >
            {snapshot ? <RefreshRounded /> : <HealthAndSafetyRounded />}
            {snapshot ? m.agent_refresh() : m.agent_start_diagnostics()}
          </Button>
        </header>

        <PrivacyCard />
        <AutonomyCard
          policy={agent.autonomy.data}
          authorizing={agent.authorizeAutonomy.isPending}
          revoking={agent.revokeAutonomy.isPending}
          onAuthorize={() => void authorizeAutonomy()}
          onRevoke={() => void revokeAutonomy()}
        />
        <BridgeCard />
        <IntentCard
          resolution={intentResolution}
          resolving={agent.executeReadOnlyIntent.isPending}
          disabled={
            agent.propose.isPending ||
            agent.executeReadOnlyIntent.isPending ||
            agent.snapshot.isFetching
          }
          onResolve={(text) => void resolveIntent(text)}
          onDiagnose={() => void refreshDiagnostics()}
          onHostConnectivity={() => void inspectHostConnectivity()}
          onPropose={(action) => void propose(action)}
        />

        {agent.snapshot.isError && <ErrorCard error={agent.snapshot.error} />}
        {snapshot && (
          <>
            <HealthCard
              snapshot={snapshot}
              canCopy={privacySafeSnapshot !== null}
              onCopy={copyContext}
            />
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
        <HistoryCard
          history={agent.history.data}
          loading={agent.history.isFetching}
          clearing={agent.clearHistory.isPending}
          onRefresh={() => void agent.history.refetch()}
          onClear={() => void clearHistory()}
        />
        <IssueGuidanceCard snapshot={snapshot} />
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

function ErrorCard({ error }: { error: unknown }) {
  return (
    <Card variant="outline" className="border-error text-error">
      <CardContent>{presentAgentError(error)}</CardContent>
    </Card>
  );
}

function HealthCard({
  snapshot,
  canCopy,
  onCopy,
}: {
  snapshot: NonNullable<ReturnType<typeof useAgent>['snapshot']['data']>;
  canCopy: boolean;
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
        <Button disabled={!canCopy} variant="stroked" onClick={onCopy}>
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
  const serialized = serializePrivacySafeSnapshot(snapshot);

  return (
    <Card variant="outline">
      <CardContent>
        {isPrivacySafeSnapshot(snapshot) && serialized ? (
          <details>
            <summary className="cursor-pointer font-medium">
              {m.agent_context_preview()}
            </summary>
            <p className="text-on-surface-variant my-3 text-sm">
              {m.agent_context_description()}
            </p>
            <pre className="bg-surface-variant/30 overflow-x-auto rounded-2xl p-3 text-xs break-all whitespace-pre-wrap">
              {serialized}
            </pre>
          </details>
        ) : (
          <p className="text-error text-sm">
            {m.agent_context_privacy_blocked()}
          </p>
        )}
      </CardContent>
    </Card>
  );
}

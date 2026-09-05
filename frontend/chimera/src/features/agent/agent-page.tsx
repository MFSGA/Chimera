import {
  useAgent,
  type AgentActionRequest,
  type AgentProposal,
} from '@chimera/interface';
import {
  HealthAndSafetyRounded,
  RefreshRounded,
  SmartToyRounded,
} from '@mui/icons-material';
import { writeText } from '@tauri-apps/plugin-clipboard-manager';
import { useState } from 'react';
import { Notice } from '@/components/base';
import { Button } from '@/components/ui/button';
import { Card, CardContent } from '@/components/ui/card';
import { AppContentScrollArea } from '@/components/ui/scroll-area';
import * as m from '@/paraglide/messages';
import { DiagnosisOverview } from './components/diagnosis-overview';
import { ProposalDialog } from './components/proposal-dialog';
import { TechnicalDetails } from './components/technical-details';

function AgentHeader({
  hasSnapshot,
  refreshing,
  onRefresh,
}: {
  hasSnapshot: boolean;
  refreshing: boolean;
  onRefresh: () => void;
}) {
  return (
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
      {hasSnapshot && (
        <Button
          className="self-start"
          loading={refreshing}
          variant="flat"
          onClick={onRefresh}
        >
          <RefreshRounded />
          {m.agent_refresh()}
        </Button>
      )}
    </header>
  );
}

function WelcomeCard({
  loading,
  onStart,
}: {
  loading: boolean;
  onStart: () => void;
}) {
  return (
    <Card variant="raised">
      <CardContent className="items-start gap-4 p-5 md:p-6">
        <div>
          <h2 className="text-xl font-semibold">{m.agent_intro_title()}</h2>
          <p className="text-on-surface-variant mt-2 max-w-3xl text-sm">
            {m.agent_intro_description()}
          </p>
        </div>
        <div className="bg-secondary-container/45 flex items-start gap-3 rounded-2xl p-3 text-sm">
          <HealthAndSafetyRounded className="mt-0.5 size-5 shrink-0" />
          <span>{m.agent_readonly_notice()}</span>
        </div>
        <Button loading={loading} variant="flat" onClick={onStart}>
          <HealthAndSafetyRounded />
          {m.agent_check_network()}
        </Button>
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
      <main className="container mx-auto flex min-h-full w-full max-w-5xl flex-col gap-5 p-4 md:p-6">
        <AgentHeader
          hasSnapshot={Boolean(snapshot)}
          refreshing={agent.snapshot.isFetching}
          onRefresh={() => void agent.snapshot.refetch()}
        />

        {agent.snapshot.isError && <ErrorCard />}
        {!snapshot ? (
          <WelcomeCard
            loading={agent.snapshot.isFetching}
            onStart={() => void agent.snapshot.refetch()}
          />
        ) : (
          <>
            <DiagnosisOverview
              snapshot={snapshot}
              pending={agent.propose.isPending}
              onPropose={(action) => void propose(action)}
            />
            <TechnicalDetails
              snapshot={snapshot}
              pending={agent.propose.isPending}
              onCopy={() => void copyContext()}
              onPropose={(action) => void propose(action)}
            />
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

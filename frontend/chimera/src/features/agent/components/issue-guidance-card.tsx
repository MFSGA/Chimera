import {
  commands,
  openThat,
  type AgentNetworkSnapshot,
} from '@chimera/interface';
import { BugReportRounded, OpenInNewRounded } from '@mui/icons-material';
import { useState } from 'react';
import { Notice } from '@/components/base';
import { Button } from '@/components/ui/button';
import { Card, CardContent, CardHeader } from '@/components/ui/card';
import * as m from '@/paraglide/messages';
import { formatEnvInfos } from '@/utils';
import { projectPrivacySafeIssueSnapshot } from '../model/privacy-safe-context';
import { buildAgentIssueUrl } from './issue-guidance';

export function IssueGuidanceCard({
  snapshot,
}: {
  snapshot: AgentNetworkSnapshot | null | undefined;
}) {
  const [opening, setOpening] = useState(false);

  const openIssue = async () => {
    setOpening(true);
    let envInfos: string = m.agent_issue_env_unavailable();

    try {
      const envs = await commands.collectEnvs();
      if (envs.status === 'ok') {
        envInfos = formatEnvInfos(envs.data)
          .split('\n')
          .map((value) => `> ${value}`)
          .join('\n');
      }
    } catch {
      // Environment collection is best effort. Never attach the raw failure.
    }

    try {
      await openThat(
        buildAgentIssueUrl({
          actual: m.agent_issue_actual_prefill(),
          envInfos,
          snapshot: projectPrivacySafeIssueSnapshot(snapshot),
        }),
      );
    } catch {
      Notice.error(m.agent_issue_open_failed());
    } finally {
      setOpening(false);
    }
  };

  return (
    <Card variant="outline">
      <CardHeader className="text-base">
        <BugReportRounded />
        {m.agent_issue_title()}
      </CardHeader>
      <CardContent>
        <p className="text-on-surface-variant text-sm">
          {m.agent_issue_description()}
        </p>
        <p className="text-on-surface-variant text-xs">
          {m.agent_issue_privacy_notice()}
        </p>
        <Button
          className="self-start"
          loading={opening}
          variant="stroked"
          onClick={() => void openIssue()}
        >
          <OpenInNewRounded />
          {m.agent_issue_button()}
        </Button>
      </CardContent>
    </Card>
  );
}

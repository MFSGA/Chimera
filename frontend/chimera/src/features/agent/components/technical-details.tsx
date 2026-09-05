import type {
  AgentActionRequest,
  AgentNetworkSnapshot,
} from '@chimera/interface';
import { ContentCopyRounded, ExpandMoreRounded } from '@mui/icons-material';
import { Button } from '@/components/ui/button';
import { Card, CardContent } from '@/components/ui/card';
import * as m from '@/paraglide/messages';
import { ActionPanel } from './action-panel';
import { ProbeFailureList } from './finding-list';
import { SnapshotSummary } from './snapshot-summary';

export function TechnicalDetails({
  snapshot,
  pending,
  onPropose,
  onCopy,
}: {
  snapshot: AgentNetworkSnapshot;
  pending: boolean;
  onPropose: (action: AgentActionRequest) => void;
  onCopy: () => void;
}) {
  return (
    <Card variant="outline">
      <CardContent>
        <details>
          <summary className="flex cursor-pointer list-none items-center justify-between gap-3 font-medium">
            <span>{m.agent_technical_details()}</span>
            <ExpandMoreRounded className="size-5" />
          </summary>
          <p className="text-on-surface-variant mt-2 text-sm">
            {m.agent_technical_details_description()}
          </p>

          <div className="mt-4 flex flex-col gap-4">
            <SnapshotSummary snapshot={snapshot} />
            <ProbeFailureList failures={snapshot.probe_failures} />
            <ActionPanel
              snapshot={snapshot}
              pending={pending}
              onPropose={onPropose}
            />

            <div className="flex justify-end">
              <Button variant="stroked" onClick={onCopy}>
                <ContentCopyRounded />
                {m.agent_copy_context()}
              </Button>
            </div>

            <details>
              <summary className="cursor-pointer text-sm font-medium">
                {m.agent_context_preview()}
              </summary>
              <p className="text-on-surface-variant my-3 text-sm">
                {m.agent_context_description()}
              </p>
              <pre className="bg-surface-variant/30 overflow-x-auto rounded-2xl p-3 text-xs break-all whitespace-pre-wrap">
                {JSON.stringify(snapshot, null, 2)}
              </pre>
            </details>
          </div>
        </details>
      </CardContent>
    </Card>
  );
}

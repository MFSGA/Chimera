import type { AgentProposal } from '@chimera/interface';
import { Button } from '@/components/ui/button';
import {
  Card,
  CardContent,
  CardFooter,
  CardHeader,
} from '@/components/ui/card';
import { Modal, ModalContent, ModalTitle } from '@/components/ui/modal';
import * as m from '@/paraglide/messages';
import { presentImpact } from '../model/presenter';

const presentRisk = (proposal: AgentProposal) =>
  proposal.risk === 'traffic_change'
    ? m.agent_risk_traffic_change()
    : m.agent_risk_host_network_change();

export function ProposalDialog({
  proposal,
  executing,
  onCancel,
  onConfirm,
}: {
  proposal: AgentProposal | null;
  executing: boolean;
  onCancel: () => void;
  onConfirm: () => void;
}) {
  return (
    <Modal
      open={proposal !== null}
      onOpenChange={(open) => !open && !executing && onCancel()}
    >
      <ModalContent>
        {proposal && (
          <Card className="mx-4 max-h-[85dvh] w-[min(40rem,calc(100vw-2rem))] overflow-y-auto">
            <CardHeader divider>
              <ModalTitle>{m.agent_confirmation_title()}</ModalTitle>
            </CardHeader>
            <CardContent>
              <p className="text-on-surface-variant text-sm">
                {m.agent_confirmation_description()}
              </p>
              <section className="flex flex-col gap-2">
                <h3 className="font-medium">{m.agent_risk()}</h3>
                <p className="bg-tertiary-container text-on-tertiary-container rounded-2xl p-3">
                  {presentRisk(proposal)}
                </p>
              </section>
              <section className="flex flex-col gap-2">
                <h3 className="font-medium">{m.agent_changes()}</h3>
                {proposal.changes.map((change) => (
                  <div
                    className="bg-surface-variant/30 grid grid-cols-[minmax(0,1fr)_auto_minmax(0,1fr)] items-center gap-2 rounded-2xl p-3 text-sm"
                    key={change.field}
                  >
                    <span className="truncate">{change.before}</span>
                    <span aria-hidden>→</span>
                    <span className="truncate text-right">{change.after}</span>
                  </div>
                ))}
              </section>
              <section className="flex flex-col gap-2">
                <h3 className="font-medium">{m.agent_impacts()}</h3>
                <ul className="list-disc pl-5 text-sm">
                  {proposal.impacts.map((impact) => (
                    <li key={impact}>{presentImpact(impact)}</li>
                  ))}
                </ul>
              </section>
              <p className="text-on-surface-variant text-xs">
                {m.agent_expires_at()}:{' '}
                {new Date(proposal.expires_at).toLocaleTimeString()}
              </p>
            </CardContent>
            <CardFooter divider>
              <Button loading={executing} variant="flat" onClick={onConfirm}>
                {m.agent_confirm_execute()}
              </Button>
              <Button disabled={executing} onClick={onCancel}>
                {m.agent_cancel()}
              </Button>
            </CardFooter>
          </Card>
        )}
      </ModalContent>
    </Modal>
  );
}

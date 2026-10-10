import type {
  AgentActionRequest,
  AgentIntentResolution,
} from '@chimera/interface';
import { SendRounded } from '@mui/icons-material';
import { useState, type FormEvent } from 'react';
import { Button } from '@/components/ui/button';
import { Card, CardContent, CardHeader } from '@/components/ui/card';
import { Input } from '@/components/ui/input';
import * as m from '@/paraglide/messages';
import { routeAgentIntent } from '../model/intent-routing';

const unsupportedText: Record<
  Extract<AgentIntentResolution, { status: 'unsupported' }>['reason'],
  () => string
> = {
  empty_input: m.agent_intent_error_empty,
  input_too_long: m.agent_intent_error_too_long,
  no_matching_intent: m.agent_intent_error_unsupported,
};

export function IntentCard({
  resolution,
  resolving,
  disabled,
  onResolve,
  onPropose,
}: {
  resolution: AgentIntentResolution | null;
  resolving: boolean;
  disabled: boolean;
  onResolve: (text: string) => void;
  onPropose: (action: AgentActionRequest) => void;
}) {
  const [text, setText] = useState('');
  const routed =
    resolution?.status === 'resolved'
      ? routeAgentIntent(resolution.intent)
      : null;

  const submit = (event: FormEvent<HTMLFormElement>) => {
    event.preventDefault();
    onResolve(text);
  };

  return (
    <Card variant="raised" data-slot="agent-intent-card">
      <CardHeader className="text-base">{m.agent_intent_title()}</CardHeader>
      <CardContent>
        <p className="text-on-surface-variant text-sm">
          {m.agent_intent_description()}
        </p>
        <form
          className="mt-3 flex flex-col gap-3 md:flex-row"
          onSubmit={submit}
        >
          <Input
            maxLength={160}
            label={m.agent_intent_placeholder()}
            value={text}
            onChange={(event) => setText(event.target.value)}
          />
          <Button
            className="md:self-center"
            disabled={disabled}
            loading={resolving}
            type="submit"
            variant="flat"
          >
            <SendRounded />
            {m.agent_intent_submit()}
          </Button>
        </form>
        {routed && (
          <div className="bg-surface-variant/30 mt-3 rounded-2xl p-3 text-sm">
            <p>
              {routed.kind === 'diagnose'
                ? m.agent_intent_diagnose_executed()
                : m.agent_intent_resolved()}
            </p>
            {routed.kind === 'proposal' && (
              <Button
                className="mt-2"
                disabled={disabled}
                onClick={() => onPropose(routed.action)}
              >
                {m.agent_intent_continue()}
              </Button>
            )}
          </div>
        )}
        {resolution?.status === 'unsupported' && (
          <p className="text-error mt-3 text-sm">
            {unsupportedText[resolution.reason]()}
          </p>
        )}
      </CardContent>
    </Card>
  );
}

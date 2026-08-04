import type {
  AgentActionRequest,
  AgentClarificationChoice,
  AgentIntent,
  AgentIntentResolution,
} from '@chimera/interface';
import { SendRounded } from '@mui/icons-material';
import { useState, type FormEvent } from 'react';
import { Button } from '@/components/ui/button';
import { Card, CardContent, CardHeader } from '@/components/ui/card';
import { Input } from '@/components/ui/input';
import * as m from '@/paraglide/messages';

const intentAction = (intent: AgentIntent): AgentActionRequest | null => {
  switch (intent.intent) {
    case 'diagnose':
      return null;
    case 'set_tun_enabled':
      return { action: 'set_tun_enabled', enabled: intent.enabled };
    case 'set_system_proxy_enabled':
      return { action: 'set_system_proxy_enabled', enabled: intent.enabled };
    case 'set_service_mode':
      return { action: 'set_service_mode', enabled: intent.enabled };
    case 'set_routing_mode':
      return { action: 'set_routing_mode', mode: intent.mode };
    case 'start_core':
      return { action: 'start_core' };
    case 'restart_core':
      return { action: 'restart_core' };
    case 'reconnect_telemetry':
      return { action: 'reconnect_telemetry' };
    case 'control_service':
      if (intent.operation === 'start') return { action: 'start_service' };
      if (intent.operation === 'stop') return { action: 'stop_service' };
      return { action: 'restart_service' };
    case 'repair_system_proxy_endpoint':
      return { action: 'repair_system_proxy_endpoint' };
    case 'disable_stale_system_proxy':
      return { action: 'disable_stale_system_proxy' };
  }
};

const clarificationLabel = (choice: AgentClarificationChoice) => {
  switch (choice.code) {
    case 'enable_tun':
      return m.agent_intent_choice_enable_tun();
    case 'use_global_routing':
      return m.agent_intent_choice_global_routing();
    case 'diagnose_network':
      return m.agent_intent_choice_diagnose();
  }
};

const unsupportedMessage = (
  resolution: Extract<AgentIntentResolution, { status: 'unsupported' }>,
) => {
  switch (resolution.reason) {
    case 'empty_input':
      return m.agent_intent_error_empty();
    case 'input_too_long':
      return m.agent_intent_error_too_long();
    case 'no_matching_intent':
      return m.agent_intent_error_unsupported();
  }
};

export function IntentCard({
  resolution,
  resolving,
  disabled,
  onResolve,
  onDiagnose,
  onPropose,
}: {
  resolution: AgentIntentResolution | null;
  resolving: boolean;
  disabled: boolean;
  onResolve: (text: string) => void;
  onDiagnose: () => void;
  onPropose: (action: AgentActionRequest) => void;
}) {
  const [text, setText] = useState('');

  const executeIntent = (intent: AgentIntent) => {
    const action = intentAction(intent);
    if (action) onPropose(action);
    else onDiagnose();
  };

  const submit = (event: FormEvent) => {
    event.preventDefault();
    onResolve(text);
  };

  return (
    <Card variant="raised">
      <CardHeader className="text-base">{m.agent_intent_title()}</CardHeader>
      <CardContent>
        <p className="text-on-surface-variant text-sm">
          {m.agent_intent_description()}
        </p>
        <form className="flex flex-col gap-3 md:flex-row" onSubmit={submit}>
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

        {resolution?.status === 'resolved' && (
          <div className="bg-surface-variant/30 rounded-2xl p-3 text-sm">
            <p>{m.agent_intent_resolved()}</p>
            <Button
              className="mt-2"
              disabled={disabled}
              onClick={() => executeIntent(resolution.intent)}
            >
              {m.agent_intent_continue()}
            </Button>
          </div>
        )}

        {resolution?.status === 'needs_clarification' && (
          <div className="flex flex-col gap-2">
            <p className="text-sm">{m.agent_intent_clarification()}</p>
            <div className="flex flex-wrap gap-2">
              {resolution.choices.map((choice) => (
                <Button
                  disabled={disabled}
                  key={choice.code}
                  variant="stroked"
                  onClick={() => executeIntent(choice.intent)}
                >
                  {clarificationLabel(choice)}
                </Button>
              ))}
            </div>
          </div>
        )}

        {resolution?.status === 'unsupported' && (
          <p className="text-error text-sm">{unsupportedMessage(resolution)}</p>
        )}
      </CardContent>
    </Card>
  );
}

import { commands, unwrapResult, type DnsResponse } from '@chimera/interface';
import { Button } from '@chimera/ui/button';
import { Input } from '@chimera/ui/input';
import {
  Select,
  SelectContent,
  SelectItem,
  SelectTrigger,
  SelectValue,
} from '@chimera/ui/select';
import { useState } from 'react';
import { useLockFn } from '@/hooks/use-lock-fn';
import * as m from '@/paraglide/messages';
import { formatError } from '@/utils';
import { message } from '@/utils/notification';
import {
  ItemContainer,
  ItemLabel,
  ItemLabelDescription,
  ItemLabelText,
  SettingsCard,
  SettingsCardContent,
  SettingsGroup,
  SettingsLabel,
} from '../../../_modules/settings-card';

const RECORD_TYPES = ['A', 'AAAA', 'CNAME', 'HTTPS', 'MX', 'TXT'] as const;

/** Asks the running core's resolver, so the answer reflects its DNS config. */
function DnsQueryCard() {
  const [name, setName] = useState('');
  const [recordType, setRecordType] = useState<string>('A');
  const [response, setResponse] = useState<DnsResponse | null>(null);
  const [pending, setPending] = useState(false);

  const handleQuery = useLockFn(async () => {
    setPending(true);
    try {
      setResponse(
        unwrapResult(await commands.clashApiQueryDns(name.trim(), recordType)),
      );
    } catch (error) {
      message(`${m.settings_clash_dns_query_failed()}\n${formatError(error)}`, {
        title: m.common_error(),
        kind: 'error',
      });
    } finally {
      setPending(false);
    }
  });

  const answers = response?.Answer ?? [];
  return (
    <SettingsCard data-slot="dns-query-card">
      <SettingsCardContent className="flex flex-col gap-3">
        <ItemLabel>
          <ItemLabelText>{m.settings_clash_dns_query_label()}</ItemLabelText>
          <ItemLabelDescription>
            {m.settings_clash_dns_query_description()}
          </ItemLabelDescription>
        </ItemLabel>
        <div
          className="flex flex-wrap items-center gap-2"
          data-slot="dns-query-form"
        >
          <Input
            className="min-w-48 flex-1"
            aria-label={m.settings_clash_dns_query_label()}
            value={name}
            placeholder="example.com"
            onChange={(event) => setName(event.target.value)}
            onKeyDown={(event) => {
              if (event.key === 'Enter' && name.trim()) handleQuery();
            }}
          />
          <Select
            variant="chip"
            value={recordType}
            onValueChange={setRecordType}
          >
            <SelectTrigger aria-label={m.settings_clash_dns_query_type_label()}>
              <SelectValue>{recordType}</SelectValue>
            </SelectTrigger>
            <SelectContent>
              {RECORD_TYPES.map((type) => (
                <SelectItem key={type} value={type}>
                  {type}
                </SelectItem>
              ))}
            </SelectContent>
          </Select>
          <Button
            variant="flat"
            disabled={!name.trim() || pending}
            loading={pending}
            onClick={handleQuery}
          >
            {m.settings_clash_dns_query_button()}
          </Button>
        </div>
        {response && (
          <ul
            className="flex flex-col gap-1 font-mono text-sm select-text"
            data-slot="dns-query-answers"
          >
            {answers.length === 0 && (
              <li className="text-on-surface-variant">
                {m.settings_clash_dns_query_no_answer({
                  status: response.Status,
                })}
              </li>
            )}
            {answers.map((record, index) => (
              <li key={index}>
                {record.name} {record.TTL} {record.data}
              </li>
            ))}
          </ul>
        )}
      </SettingsCardContent>
    </SettingsCard>
  );
}

function FlushButton({
  label,
  description,
  flush,
}: {
  label: string;
  description: string;
  flush: () => Promise<unknown>;
}) {
  const handleFlush = useLockFn(async () => {
    try {
      await flush();
      message(m.settings_clash_dns_flush_succeeded(), { kind: 'info' });
    } catch (error) {
      message(`${m.settings_clash_dns_flush_failed()}\n${formatError(error)}`, {
        title: m.common_error(),
        kind: 'error',
      });
    }
  });

  return (
    <SettingsCard data-slot="dns-flush-card">
      <SettingsCardContent asChild>
        <Button
          className="text-on-surface! h-auto w-full rounded-none px-5 text-left text-base"
          onClick={handleFlush}
        >
          <ItemContainer>
            <ItemLabel>
              <ItemLabelText>{label}</ItemLabelText>
              <ItemLabelDescription>{description}</ItemLabelDescription>
            </ItemLabel>
          </ItemContainer>
        </Button>
      </SettingsCardContent>
    </SettingsCard>
  );
}

export default function DnsTools() {
  return (
    <div className="px-4 pb-4" data-slot="dns-tools">
      <SettingsLabel>{m.settings_clash_dns_tools_label()}</SettingsLabel>
      <SettingsGroup>
        <DnsQueryCard />
        <FlushButton
          label={m.settings_clash_dns_flush_dns_cache_label()}
          description={m.settings_clash_dns_flush_dns_cache_description()}
          flush={async () =>
            unwrapResult(await commands.clashApiFlushDnsCache())
          }
        />
        <FlushButton
          label={m.settings_clash_dns_flush_fake_ip_cache_label()}
          description={m.settings_clash_dns_flush_fake_ip_cache_description()}
          flush={async () =>
            unwrapResult(await commands.clashApiFlushFakeIpCache())
          }
        />
      </SettingsGroup>
    </div>
  );
}

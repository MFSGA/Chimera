import {
  useClashSettings,
  useDnsFeatures,
  useSetting,
  type ClashConfigPatch_Serialize,
  type ControlSource,
} from '@chimera/interface';
import { Button } from '@chimera/ui/button';
import { Switch } from '@chimera/ui/switch';
import { createFileRoute } from '@tanstack/react-router';
import { PageTitle } from '@/components/page-title';
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
} from '../../_modules/settings-card';
import {
  DnsDraftProvider,
  useDnsDraft,
  useSavedDns,
} from './_modules/dns-draft';
import DnsGeneralSection from './_modules/dns-general-section';
import DnsSaveBar from './_modules/dns-save-bar';
import DnsTools from './_modules/dns-tools';
import DnsUpstreamSection from './_modules/dns-upstream-section';
import HostsSection from './_modules/hosts-section';

export const Route = createFileRoute('/(main)/main/settings/clash/dns/')({
  component: RouteComponent,
  staticData: {
    crumb: {
      title: () => m.dns_title(),
      parent: () => ({ to: '/main/settings' }),
    },
  },
});

function ControlSwitch({
  field,
  label,
  description,
  value,
  pending,
  onChange,
}: {
  field: 'dns_control' | 'hosts_control';
  label: string;
  description: string;
  value: ControlSource | undefined;
  pending: boolean;
  onChange: (
    field: 'dns_control' | 'hosts_control',
    value: ControlSource,
  ) => void;
}) {
  return (
    <SettingsCard data-slot="dns-control-switch-card">
      <SettingsCardContent>
        <ItemContainer>
          <ItemLabel>
            <ItemLabelText>{label}</ItemLabelText>
            <ItemLabelDescription>{description}</ItemLabelDescription>
          </ItemLabel>
          <Switch
            aria-label={label}
            checked={value === 'chimera'}
            data-slot={
              field === 'dns_control'
                ? 'dns-control-dns-switch'
                : 'dns-control-hosts-switch'
            }
            disabled={pending || !value}
            loading={pending}
            onCheckedChange={(checked) =>
              onChange(field, checked ? 'chimera' : 'profile')
            }
          />
        </ItemContainer>
      </SettingsCardContent>
    </SettingsCard>
  );
}

function DnsControlSection({
  dnsControl,
  hostsControl,
}: {
  dnsControl: ControlSource | undefined;
  hostsControl: ControlSource | undefined;
}) {
  const { draft, dirty } = useDnsDraft();
  const { upsert } = useClashSettings();
  const patchControl = useLockFn(
    async (field: 'dns_control' | 'hosts_control', value: ControlSource) => {
      try {
        // A toggle must never activate an older DNS snapshot or discard dirty
        // edits when the query cache refreshes. Commit the two draft tables and
        // selector together in one typed patch.
        const patch: ClashConfigPatch_Serialize = dirty
          ? { [field]: value, dns: draft.dns, hosts: draft.hosts }
          : { [field]: value };
        await upsert.mutateAsync(patch);
      } catch (error) {
        message(`${m.dns_control_failed()}\n${formatError(error)}`, {
          title: m.common_error(),
          kind: 'error',
        });
      }
    },
  );

  return (
    <div data-slot="dns-control-settings">
      <SettingsLabel>{m.dns_control()}</SettingsLabel>
      <SettingsGroup>
        <ControlSwitch
          field="dns_control"
          label={m.dns_control_dns()}
          description={m.dns_control_dns_description()}
          value={dnsControl}
          pending={upsert.isPending}
          onChange={patchControl}
        />
        <ControlSwitch
          field="hosts_control"
          label={m.dns_control_hosts()}
          description={m.dns_control_hosts_description()}
          value={hostsControl}
          pending={upsert.isPending}
          onChange={patchControl}
        />
      </SettingsGroup>
    </div>
  );
}

function RouteComponent() {
  const core = useSetting('core');
  const { query, upsert } = useClashSettings();
  const { saved } = useSavedDns();
  const featureQuery = useDnsFeatures(core.value);
  const features = featureQuery.data;
  const supported = features?.keys.kind === 'values';

  const disableTakeover = useLockFn(async () => {
    try {
      await upsert.mutateAsync({
        dns_control: 'profile',
        hosts_control: 'profile',
      });
    } catch (error) {
      message(`${m.dns_control_failed()}\n${formatError(error)}`, {
        title: m.common_error(),
        kind: 'error',
      });
    }
  });

  const takeoverActive =
    query.data?.dns_control === 'chimera' ||
    query.data?.hosts_control === 'chimera';

  return (
    <>
      <PageTitle />
      <div className="space-y-4 px-4 pb-4" data-slot="dns-settings-page">
        <p className="text-on-surface-variant text-sm">
          {m.dns_takeover_description()}
        </p>
        {(query.isPending ||
          core.value === undefined ||
          featureQuery.isPending) && <p role="status">{m.inspect_loading()}</p>}
        {(query.isError || featureQuery.isError) && (
          <p role="alert" className="text-error">
            {m.dns_read_failed()}
          </p>
        )}
        {featureQuery.isSuccess && !supported && core.value !== undefined && (
          <SettingsCard>
            <SettingsCardContent className="flex flex-col gap-3">
              <p role="status">{m.dns_core_unsupported()}</p>
              {takeoverActive && (
                <Button
                  variant="stroked"
                  disabled={upsert.isPending}
                  onClick={disableTakeover}
                >
                  {m.dns_return_to_profile()}
                </Button>
              )}
            </SettingsCardContent>
          </SettingsCard>
        )}
        {supported && features && query.data && (
          <>
            {saved ? (
              <DnsDraftProvider saved={saved} features={features}>
                <DnsControlSection
                  dnsControl={query.data.dns_control}
                  hostsControl={query.data.hosts_control}
                />
                {query.data.dns_control !== 'chimera' && (
                  <p
                    className="text-on-surface-variant text-sm"
                    data-slot="dns-profile-control-notice"
                  >
                    {m.dns_profile_controls_notice()}
                  </p>
                )}
                <DnsGeneralSection />
                <DnsUpstreamSection />
                <HostsSection hostsControl={query.data.hosts_control} />
                <DnsSaveBar />
              </DnsDraftProvider>
            ) : (
              <p role="status" className="text-on-surface-variant text-sm">
                {m.dns_read_missing()}
              </p>
            )}
          </>
        )}
      </div>
      <DnsTools />
    </>
  );
}

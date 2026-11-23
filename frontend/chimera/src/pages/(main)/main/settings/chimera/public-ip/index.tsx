import { createFileRoute } from '@tanstack/react-router';
import { PageTitle } from '@/components/page-title';
import * as m from '@/paraglide/messages';
import LocalIpProbeSwitch from '../_modules/local-ip-probe-switch';
import IpLookupProviderList from '../_modules/public-ip/ip-lookup-provider-list';
import StunServerList from '../_modules/public-ip/stun-server-list';
import { SettingsGroup } from '../../_modules/settings-card';

export const Route = createFileRoute(
  '/(main)/main/settings/chimera/public-ip/',
)({
  component: RouteComponent,
  staticData: {
    crumb: {
      title: () => m.settings_label_public_ip(),
      parent: () => ({ to: '/main/settings' }),
    },
  },
});

function RouteComponent() {
  return (
    <>
      <PageTitle legacySlot="settings-title" />
      <div className="space-y-4 px-4 pb-4" data-slot="public-ip-settings">
        <SettingsGroup>
          <LocalIpProbeSwitch />
        </SettingsGroup>
        <StunServerList />
        <IpLookupProviderList />
      </div>
    </>
  );
}

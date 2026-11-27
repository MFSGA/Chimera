import { useSetting } from '@chimera/interface';
import { createFileRoute } from '@tanstack/react-router';
import { PageTitle } from '@/components/page-title';
import { m } from '@/paraglide/messages';
import { SettingsGroup, SettingsLabel } from '../../_modules/settings-card';
import NetworkStatisticWidgetIconConfig from './_modules/network-statistic-widget-icon-config';
import NetworkStatisticWidgetLogoFollowStateSwitch from './_modules/network-statistic-widget-logo-follow-state-switch';
import NetworkStatisticWidgetLogoSourceSelector from './_modules/network-statistic-widget-logo-source-selector';
import NetworkStatisticWidgetSelector from './_modules/network-statistic-widget-selector';

export const Route = createFileRoute('/(main)/main/settings/chimera/widget/')({
  component: RouteComponent,
  staticData: {
    crumb: {
      title: () => m.settings_label_widget(),
      parent: () => ({ to: '/main/settings' }),
    },
  },
});

function RouteComponent() {
  const { value: widget } = useSetting('network_statistic_widget');
  const { value: logo } = useSetting('network_statistic_widget_logo');

  // The logo settings only matter while a widget shows.
  const enabled = widget?.kind === 'enabled';

  return (
    <>
      <PageTitle legacySlot="settings-title" />

      <div className="space-y-4 px-4 pb-4">
        <div data-slot="widget-settings-container">
          <SettingsLabel>
            {m.settings_chimera_network_statistic_widget_label()}
          </SettingsLabel>

          <SettingsGroup>
            <NetworkStatisticWidgetSelector />

            {enabled && <NetworkStatisticWidgetLogoFollowStateSwitch />}

            {enabled && <NetworkStatisticWidgetLogoSourceSelector />}
          </SettingsGroup>

          {enabled && logo?.source === 'widget' && (
            <NetworkStatisticWidgetIconConfig />
          )}
        </div>
      </div>
    </>
  );
}

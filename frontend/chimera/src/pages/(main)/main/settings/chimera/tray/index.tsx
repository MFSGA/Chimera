import { useSetting } from '@chimera/interface';
import { createFileRoute } from '@tanstack/react-router';
import { PageTitle } from '@/components/page-title';
import { isMacOS, OS } from '@/consts';
import * as m from '@/paraglide/messages';
import TrayIconConfig from '../_modules/tray-icon-config';
import TrayProxiesSelector from '../_modules/tray-proxies-selector';
import WindowCloseBehaviorCards from '../_modules/window-close-behavior';
import { SettingsGroup, SettingsLabel } from '../../_modules/settings-card';
import TrayMenuModeSelector from './_modules/tray-menu-mode';

export const Route = createFileRoute('/(main)/main/settings/chimera/tray/')({
  component: RouteComponent,
  staticData: {
    crumb: {
      title: () => m.settings_label_tray(),
      parent: () => ({ to: '/main/settings' }),
    },
  },
});

function RouteComponent() {
  const { value: menuMode } = useSetting('tray_menu_mode');
  const webview = OS !== 'linux' && menuMode === 'webview';
  return (
    <>
      <PageTitle legacySlot="settings-title" />
      <div className="space-y-4 px-4 pb-4">
        <div data-slot="tray-settings-container">
          <SettingsLabel>{m.settings_chimera_tray()}</SettingsLabel>
          <SettingsGroup>
            {OS !== 'linux' && <TrayMenuModeSelector />}
            {!webview && <TrayProxiesSelector />}
          </SettingsGroup>
          {!isMacOS && <TrayIconConfig />}
        </div>
        <div data-slot="window-settings-container">
          <SettingsLabel>{m.settings_chimera_window_close()}</SettingsLabel>
          <SettingsGroup>
            <WindowCloseBehaviorCards showTrayMenu={webview} />
          </SettingsGroup>
        </div>
      </div>
    </>
  );
}

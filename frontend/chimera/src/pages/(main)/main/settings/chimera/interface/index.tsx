import { createFileRoute } from '@tanstack/react-router';
import { PageTitle } from '@/components/page-title';
import * as m from '@/paraglide/messages';
import LightenAnimationEffectsSwitch from '../_modules/lighten-animation-effects-switch';
import { SettingsGroup, SettingsLabel } from '../../_modules/settings-card';
import CustomCssCard from '../../user-interface/_modules/custom-css-card';
import LanguageSelector from '../../user-interface/_modules/language-selector';
import ThemeColorConfig from '../../user-interface/_modules/theme-color-config';
import ThemeModeSelector from '../../user-interface/_modules/theme-mode-selector';

export const Route = createFileRoute(
  '/(main)/main/settings/chimera/interface/',
)({
  component: RouteComponent,
  staticData: {
    crumb: {
      title: () => m.settings_user_interface_title(),
      parent: () => ({ to: '/main/settings' }),
    },
  },
  head: () => ({
    meta: [{ title: m.settings_user_interface_title() }],
  }),
});

const LanguageSettings = () => (
  <div data-slot="language-settings-container">
    <SettingsLabel>{m.settings_user_interface_language_group()}</SettingsLabel>
    <SettingsGroup>
      <LanguageSelector />
    </SettingsGroup>
  </div>
);

const ThemeModeSettings = () => (
  <div data-slot="theme-mode-settings-container">
    <SettingsLabel>
      {m.settings_user_interface_theme_mode_group()}
    </SettingsLabel>
    <SettingsGroup>
      <ThemeModeSelector />
      <ThemeColorConfig />
      <LightenAnimationEffectsSwitch />
    </SettingsGroup>
  </div>
);

const CustomCssSettings = () => (
  <div className="space-y-4" data-slot="custom-css-settings-container">
    <SettingsLabel>
      {m.settings_user_interface_custom_css_group()}
    </SettingsLabel>
    <SettingsGroup>
      <CustomCssCard />
    </SettingsGroup>
  </div>
);

function RouteComponent() {
  return (
    <>
      <PageTitle legacySlot="settings-title" />
      <div className="space-y-4 px-4 pb-4">
        <LanguageSettings />
        <ThemeModeSettings />
        <CustomCssSettings />
      </div>
    </>
  );
}

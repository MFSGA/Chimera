import * as m from '@/paraglide/messages';
import { SettingsGroup, SettingsLabel } from '../../_modules/settings-card';
import CustomCssCard from './custom-css-card';
import LanguageSelector from './language-selector';
import ThemeColorConfig from './theme-color-config';
import ThemeModeSelector from './theme-mode-selector';

const LanguageSettings = () => (
  <section data-slot="language-settings-container">
    <SettingsLabel>{m.settings_user_interface_language_group()}</SettingsLabel>
    <SettingsGroup>
      <LanguageSelector />
    </SettingsGroup>
  </section>
);

const ThemeModeSettings = () => (
  <section data-slot="theme-mode-settings-container">
    <SettingsLabel>
      {m.settings_user_interface_theme_mode_group()}
    </SettingsLabel>
    <SettingsGroup>
      <ThemeModeSelector />
      <ThemeColorConfig />
    </SettingsGroup>
  </section>
);

const CustomCssSettings = () => (
  <section data-slot="custom-css-settings-container">
    <SettingsLabel>
      {m.settings_user_interface_custom_css_group()}
    </SettingsLabel>
    <SettingsGroup>
      <CustomCssCard />
    </SettingsGroup>
  </section>
);

export default function UserInterfaceSettings() {
  return (
    <div className="space-y-4">
      <LanguageSettings />
      <ThemeModeSettings />
      <CustomCssSettings />
    </div>
  );
}

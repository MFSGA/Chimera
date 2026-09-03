import { useProxyMode, type ProxyMode } from '@chimera/interface';
import type { PropsWithChildren } from 'react';
import { useLanguage } from '@/components/providers/language-provider';
import {
  ThemeMode,
  useExperimentalThemeContext,
} from '@/components/providers/theme-provider';
import {
  DropdownMenu,
  DropdownMenuCheckboxItem,
  DropdownMenuContent,
  DropdownMenuRadioGroup,
  DropdownMenuRadioItem,
  DropdownMenuSeparator,
  DropdownMenuSub,
  DropdownMenuSubContent,
  DropdownMenuSubTrigger,
  DropdownMenuTrigger,
} from '@/components/ui/dropdown-menu';
import { CircularProgress } from '@/components/ui/progress';
import {
  useSystemProxyAction,
  useTunModeAction,
} from '@/features/system-proxy/use-proxy-settings';
import * as m from '@/paraglide/messages';
import { locales, type Locale } from '@/paraglide/runtime';

const LanguageSelector = () => {
  const { language, setLanguage } = useLanguage();

  return (
    <DropdownMenuSub>
      <DropdownMenuSubTrigger>
        {m.header_settings_action_language()}
      </DropdownMenuSubTrigger>
      <DropdownMenuSubContent>
        <DropdownMenuRadioGroup
          value={language}
          onValueChange={(value) => void setLanguage(value as Locale)}
        >
          {locales.map((locale) => (
            <DropdownMenuRadioItem key={locale} value={locale}>
              {m.language({}, { locale })}
            </DropdownMenuRadioItem>
          ))}
        </DropdownMenuRadioGroup>
      </DropdownMenuSubContent>
    </DropdownMenuSub>
  );
};

const ThemeModeSelector = () => {
  const { themeMode, setThemeMode } = useExperimentalThemeContext();
  const labels = {
    [ThemeMode.LIGHT]: m.settings_user_interface_theme_mode_light(),
    [ThemeMode.DARK]: m.settings_user_interface_theme_mode_dark(),
    [ThemeMode.SYSTEM]: m.settings_user_interface_theme_mode_system(),
  } satisfies Record<ThemeMode, string>;

  return (
    <DropdownMenuSub>
      <DropdownMenuSubTrigger>
        {m.header_settings_action_theme_mode()}
      </DropdownMenuSubTrigger>
      <DropdownMenuSubContent>
        <DropdownMenuRadioGroup
          value={themeMode}
          onValueChange={(value) => void setThemeMode(value as ThemeMode)}
        >
          {Object.entries(labels).map(([mode, label]) => (
            <DropdownMenuRadioItem key={mode} value={mode}>
              {label}
            </DropdownMenuRadioItem>
          ))}
        </DropdownMenuRadioGroup>
      </DropdownMenuSubContent>
    </DropdownMenuSub>
  );
};

const ProxySettings = () => {
  const systemProxy = useSystemProxyAction();
  const tunMode = useTunModeAction();
  const proxyMode = useProxyMode();
  const labels = {
    global: m.settings_system_proxy_global_mode_label(),
    direct: m.settings_system_proxy_direct_mode_label(),
    rule: m.settings_system_proxy_rule_mode_label(),
    script: m.settings_system_proxy_script_mode_label(),
  } satisfies Record<ProxyMode, string>;
  const activeMode = Object.entries(proxyMode.value).find(
    ([, enabled]) => enabled,
  )?.[0];

  return (
    <DropdownMenuSub>
      <DropdownMenuSubTrigger data-slot="proxy-settings-trigger">
        {m.header_settings_action_proxy_settings()}
      </DropdownMenuSubTrigger>
      <DropdownMenuSubContent>
        <DropdownMenuCheckboxItem
          className="group relative"
          checked={systemProxy.isActive}
          onCheckedChange={() => void systemProxy.execute()}
          data-loading={String(systemProxy.isPending)}
        >
          <span className="text-nowrap">
            {m.settings_system_proxy_system_proxy_label()}
          </span>
          <CircularProgress
            className="invisible size-4 group-data-[loading=true]:visible"
            indeterminate
          />
        </DropdownMenuCheckboxItem>
        <DropdownMenuCheckboxItem
          className="group relative"
          checked={tunMode.isActive}
          onCheckedChange={() => void tunMode.execute()}
          data-loading={String(tunMode.isPending)}
        >
          <span className="text-nowrap">
            {m.settings_system_proxy_tun_mode_label()}
          </span>
          <CircularProgress
            className="invisible size-4 group-data-[loading=true]:visible"
            indeterminate
          />
        </DropdownMenuCheckboxItem>
        <DropdownMenuSeparator />
        <DropdownMenuRadioGroup
          value={activeMode}
          onValueChange={(value) => void proxyMode.upsert(value as ProxyMode)}
        >
          {Object.keys(proxyMode.value).map((mode) => (
            <DropdownMenuRadioItem
              key={mode}
              className="[&>div]:pl-1.5"
              value={mode}
              data-proxy-mode={mode}
            >
              {labels[mode as ProxyMode]}
            </DropdownMenuRadioItem>
          ))}
        </DropdownMenuRadioGroup>
      </DropdownMenuSubContent>
    </DropdownMenuSub>
  );
};

export default function HeaderSettingsAction({ children }: PropsWithChildren) {
  return (
    <DropdownMenu>
      <DropdownMenuTrigger asChild>{children}</DropdownMenuTrigger>
      <DropdownMenuContent>
        <LanguageSelector />
        <ThemeModeSelector />
        <ProxySettings />
      </DropdownMenuContent>
    </DropdownMenu>
  );
}

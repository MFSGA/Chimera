import { useClashSetting, type ProxyChangeBreakMode } from '@chimera/interface';
import { Button } from '@chimera/ui/button';
import ArrowForwardIosRounded from '~icons/material-symbols/arrow-forward-ios-rounded';
import {
  DropdownMenu,
  DropdownMenuCheckboxItem,
  DropdownMenuContent,
  DropdownMenuTrigger,
} from '@/components/ui/dropdown-menu';
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
} from '../../_modules/settings-card';

export default function BreakWhenProxyChangeSwitch() {
  const breakWhenProxyChange = useClashSetting('break_connection');
  const value = breakWhenProxyChange.value?.on_proxy_change ?? 'off';

  const handleChange = useLockFn(async (mode: ProxyChangeBreakMode) => {
    try {
      await breakWhenProxyChange.upsert({ on_proxy_change: mode });
    } catch (error) {
      await message(
        `${m.settings_proxies_break_change_update_failed()}\n${formatError(error)}`,
        { kind: 'error' },
      );
    }
  });

  const options = {
    off: m.settings_chimera_enhance_break_when_proxy_change_none(),
    proxy_group: m.settings_chimera_enhance_break_when_proxy_change_chain(),
    all: m.settings_chimera_enhance_break_when_proxy_change_all(),
  } satisfies Record<ProxyChangeBreakMode, string>;

  return (
    <SettingsCard data-slot="break-when-proxy-change-selector">
      <DropdownMenu align="end">
        <DropdownMenuTrigger asChild>
          <SettingsCardContent
            data-slot="break-when-proxy-change-selector-trigger"
            asChild
          >
            <Button className="text-on-surface! h-auto w-full rounded-none px-5 text-left text-base">
              <ItemContainer>
                <ItemLabel>
                  <ItemLabelText>
                    {m.settings_chimera_enhance_break_when_proxy_change_label()}
                  </ItemLabelText>

                  <ItemLabelDescription>
                    {m.settings_chimera_enhance_break_when_proxy_change_description()}
                    {' · '}
                    {options[value]}
                  </ItemLabelDescription>
                </ItemLabel>

                <ArrowForwardIosRounded />
              </ItemContainer>
            </Button>
          </SettingsCardContent>
        </DropdownMenuTrigger>

        <DropdownMenuContent sideOffset={-16} alignOffset={16}>
          {Object.entries(options).map(([key, label]) => (
            <DropdownMenuCheckboxItem
              checked={value === key}
              disabled={breakWhenProxyChange.isPending}
              key={key}
              onSelect={() => void handleChange(key as ProxyChangeBreakMode)}
            >
              {label}
            </DropdownMenuCheckboxItem>
          ))}
        </DropdownMenuContent>
      </DropdownMenu>
    </SettingsCard>
  );
}

import { useSetting, type BreakWhenProxyChange } from '@chimera/interface';
import ArrowForwardIosRounded from '~icons/material-symbols/arrow-forward-ios-rounded';
import { Button } from '@/components/ui/button';
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
  const breakWhenProxyChange = useSetting('break_when_proxy_change');
  const value = breakWhenProxyChange.value ?? 'none';

  const handleChange = useLockFn(async (mode: BreakWhenProxyChange) => {
    try {
      await breakWhenProxyChange.upsert(mode);
    } catch (error) {
      await message(
        `${m.settings_proxies_break_change_update_failed()}\n${formatError(error)}`,
        { kind: 'error' },
      );
    }
  });

  const options = {
    none: m.settings_chimera_enhance_break_when_proxy_change_none(),
    chain: m.settings_chimera_enhance_break_when_proxy_change_chain(),
    all: m.settings_chimera_enhance_break_when_proxy_change_all(),
  } satisfies Record<BreakWhenProxyChange, string>;

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
              onSelect={() => void handleChange(key as BreakWhenProxyChange)}
            >
              {label}
            </DropdownMenuCheckboxItem>
          ))}
        </DropdownMenuContent>
      </DropdownMenu>
    </SettingsCard>
  );
}

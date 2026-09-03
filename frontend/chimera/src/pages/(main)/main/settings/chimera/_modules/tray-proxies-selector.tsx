import { useSetting, type ProxiesSelectorMode } from '@chimera/interface';
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
import {
  ItemContainer,
  ItemLabel,
  ItemLabelDescription,
  ItemLabelText,
  SettingsCard,
  SettingsCardContent,
} from '../../_modules/settings-card';

export default function TrayProxiesSelector() {
  const { value, upsert } = useSetting('clash_tray_selector');

  const handleChange = useLockFn(async (mode: ProxiesSelectorMode) => {
    await upsert(mode);
  });

  const messages = {
    normal: m.settings_chimera_tray_type_normal(),
    hidden: m.settings_chimera_tray_type_hidden(),
    submenu: m.settings_chimera_tray_type_submenu(),
  } satisfies Record<ProxiesSelectorMode, string>;

  return (
    <SettingsCard data-slot="tray-proxies-selector">
      <DropdownMenu align="end">
        <DropdownMenuTrigger asChild>
          <SettingsCardContent
            data-slot="tray-proxies-selector-trigger"
            asChild
          >
            <Button className="text-on-surface! h-auto w-full rounded-none px-5 text-left text-base">
              <ItemContainer>
                <ItemLabel>
                  <ItemLabelText>
                    {m.settings_chimera_tray_type()}
                  </ItemLabelText>

                  <ItemLabelDescription>
                    {value ? messages[value] : null}
                  </ItemLabelDescription>
                </ItemLabel>

                <ArrowForwardIosRounded />
              </ItemContainer>
            </Button>
          </SettingsCardContent>
        </DropdownMenuTrigger>

        <DropdownMenuContent sideOffset={-16} alignOffset={16}>
          {Object.entries(messages).map(([key, label]) => (
            <DropdownMenuCheckboxItem
              checked={value === key}
              key={key}
              onSelect={() => void handleChange(key as ProxiesSelectorMode)}
            >
              {label}
            </DropdownMenuCheckboxItem>
          ))}
        </DropdownMenuContent>
      </DropdownMenu>
    </SettingsCard>
  );
}

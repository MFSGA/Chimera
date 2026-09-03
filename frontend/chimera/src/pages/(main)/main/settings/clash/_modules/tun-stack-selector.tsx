import { useSetting, type TunStack } from '@chimera/interface';
import ArrowForwardIosRounded from '~icons/material-symbols/arrow-forward-ios-rounded';
import { Button } from '@/components/ui/button';
import {
  DropdownMenu,
  DropdownMenuCheckboxItem,
  DropdownMenuContent,
  DropdownMenuTrigger,
} from '@/components/ui/dropdown-menu';
import { useTunStackModel } from '@/features/tun-stack/use-tun-stack';
import * as m from '@/paraglide/messages';
import {
  ItemContainer,
  ItemLabel,
  ItemLabelDescription,
  ItemLabelText,
  SettingsCard,
  SettingsCardContent,
} from '../../_modules/settings-card';

export default function TunStackSelector() {
  const coreType = useSetting('clash_core');
  const {
    execute: changeTunStack,
    isPending,
    options: tunStackOptions,
    selected: currentTunStack,
    value: tunStack,
  } = useTunStackModel(coreType.value);

  return (
    <SettingsCard data-slot="tun-stack-selector-card">
      <DropdownMenu>
        <DropdownMenuTrigger asChild>
          <SettingsCardContent data-slot="tun-stack-selector-trigger" asChild>
            <Button className="text-on-surface! h-auto w-full rounded-none px-5 text-left text-base">
              <ItemContainer>
                <ItemLabel>
                  <ItemLabelText>
                    {m.settings_clash_settings_tun_stack_label()}
                  </ItemLabelText>
                  <ItemLabelDescription>
                    {currentTunStack ? tunStackOptions[currentTunStack] : null}
                  </ItemLabelDescription>
                </ItemLabel>
                <ArrowForwardIosRounded />
              </ItemContainer>
            </Button>
          </SettingsCardContent>
        </DropdownMenuTrigger>

        <DropdownMenuContent align="end" sideOffset={-16} alignOffset={16}>
          {Object.entries(tunStackOptions).map(([key, label]) => (
            <DropdownMenuCheckboxItem
              checked={tunStack === key}
              disabled={isPending}
              key={key}
              onSelect={() => void changeTunStack(key as TunStack)}
            >
              {label}
            </DropdownMenuCheckboxItem>
          ))}
        </DropdownMenuContent>
      </DropdownMenu>
    </SettingsCard>
  );
}

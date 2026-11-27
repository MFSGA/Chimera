import {
  fromNetworkStatisticWidgetOption,
  toNetworkStatisticWidgetOption,
  useSetting,
  type NetworkStatisticWidgetOption,
} from '@chimera/interface';
import { Button } from '@chimera/ui/button';
import {
  DropdownMenu,
  DropdownMenuCheckboxItem,
  DropdownMenuContent,
  DropdownMenuTrigger,
} from '@chimera/ui/dropdown-menu';
import ArrowForwardIosRounded from '~icons/material-symbols/arrow-forward-ios-rounded';
import { useLockFn } from '@/hooks/use-lock-fn';
import { m } from '@/paraglide/messages';
import {
  ItemContainer,
  ItemLabel,
  ItemLabelDescription,
  ItemLabelText,
  SettingsCard,
  SettingsCardContent,
} from '../../../_modules/settings-card';

export default function NetworkStatisticWidgetSelector() {
  const { value, upsert } = useSetting('network_statistic_widget');

  const handleChange = useLockFn(async (mode: NetworkStatisticWidgetOption) => {
    await upsert(fromNetworkStatisticWidgetOption(mode));
  });

  const messages = {
    disabled: m.settings_chimera_network_statistic_widget_disabled(),
    large: m.settings_chimera_network_statistic_widget_large(),
    small: m.settings_chimera_network_statistic_widget_small(),
  } satisfies Record<NetworkStatisticWidgetOption, string>;

  const current = value ? toNetworkStatisticWidgetOption(value) : 'disabled';

  return (
    <SettingsCard data-slot="network-statistic-widget-selector">
      <DropdownMenu align="end">
        <DropdownMenuTrigger asChild>
          <SettingsCardContent
            data-slot="network-statistic-widget-selector-trigger"
            asChild
          >
            <Button className="text-on-surface! h-auto w-full rounded-none px-5 text-left text-base">
              <ItemContainer>
                <ItemLabel>
                  <ItemLabelText>
                    {m.settings_chimera_network_statistic_widget_label()}
                  </ItemLabelText>

                  <ItemLabelDescription>
                    {messages[current]}
                  </ItemLabelDescription>
                </ItemLabel>

                <ArrowForwardIosRounded />
              </ItemContainer>
            </Button>
          </SettingsCardContent>
        </DropdownMenuTrigger>

        <DropdownMenuContent sideOffset={-16} alignOffset={16}>
          {Object.entries(messages).map(([key, message]) => (
            <DropdownMenuCheckboxItem
              checked={current === key}
              key={key}
              onSelect={() => handleChange(key as NetworkStatisticWidgetOption)}
            >
              {message}
            </DropdownMenuCheckboxItem>
          ))}
        </DropdownMenuContent>
      </DropdownMenu>
    </SettingsCard>
  );
}

import {
  useSetting,
  type NetworkStatisticWidgetLogoSource,
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

export default function NetworkStatisticWidgetLogoSourceSelector() {
  const { value, upsert } = useSetting('network_statistic_widget_logo');

  const handleChange = useLockFn(
    async (source: NetworkStatisticWidgetLogoSource) => {
      if (!value) return;

      await upsert({ ...value, source });
    },
  );

  const messages = {
    tray: m.settings_chimera_network_statistic_widget_logo_source_tray(),
    widget: m.settings_chimera_network_statistic_widget_logo_source_widget(),
  } satisfies Record<NetworkStatisticWidgetLogoSource, string>;

  const current = value?.source ?? 'tray';

  return (
    <SettingsCard data-slot="network-statistic-widget-logo-source-selector">
      <DropdownMenu align="end">
        <DropdownMenuTrigger asChild>
          <SettingsCardContent
            data-slot="network-statistic-widget-logo-source-selector-trigger"
            asChild
          >
            <Button className="text-on-surface! h-auto w-full rounded-none px-5 text-left text-base">
              <ItemContainer>
                <ItemLabel>
                  <ItemLabelText>
                    {m.settings_chimera_network_statistic_widget_logo_source_label()}
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
              onSelect={() =>
                handleChange(key as NetworkStatisticWidgetLogoSource)
              }
            >
              {message}
            </DropdownMenuCheckboxItem>
          ))}
        </DropdownMenuContent>
      </DropdownMenu>
    </SettingsCard>
  );
}

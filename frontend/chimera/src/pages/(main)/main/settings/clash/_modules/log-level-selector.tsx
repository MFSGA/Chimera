import ArrowForwardIosRounded from '~icons/material-symbols/arrow-forward-ios-rounded';
import { Button } from '@/components/ui/button';
import {
  DropdownMenu,
  DropdownMenuCheckboxItem,
  DropdownMenuContent,
  DropdownMenuTrigger,
} from '@/components/ui/dropdown-menu';
import {
  useClashBaseSettings,
  type ClashLogLevel,
} from '@/features/clash-settings/use-clash-base-settings';
import * as m from '@/paraglide/messages';
import {
  ItemContainer,
  ItemLabel,
  ItemLabelDescription,
  ItemLabelText,
  SettingsCard,
  SettingsCardContent,
} from '../../_modules/settings-card';

export default function LogLevelSelector() {
  const { logLevel, logLevelOptions, setLogLevel } = useClashBaseSettings();

  return (
    <SettingsCard data-slot="log-level-selector-card">
      <DropdownMenu>
        <DropdownMenuTrigger asChild>
          <SettingsCardContent data-slot="log-level-selector-trigger" asChild>
            <Button className="text-on-surface! h-auto w-full rounded-none px-5 text-left text-base">
              <ItemContainer>
                <ItemLabel>
                  <ItemLabelText>
                    {m.settings_clash_settings_log_level_label()}
                  </ItemLabelText>

                  <ItemLabelDescription>
                    {logLevelOptions[logLevel]}
                  </ItemLabelDescription>
                </ItemLabel>

                <ArrowForwardIosRounded />
              </ItemContainer>
            </Button>
          </SettingsCardContent>
        </DropdownMenuTrigger>

        <DropdownMenuContent align="end" sideOffset={-16} alignOffset={16}>
          {Object.entries(logLevelOptions).map(([key, label]) => (
            <DropdownMenuCheckboxItem
              checked={logLevel === key}
              key={key}
              onSelect={() => void setLogLevel(key as ClashLogLevel)}
            >
              {label}
            </DropdownMenuCheckboxItem>
          ))}
        </DropdownMenuContent>
      </DropdownMenu>
    </SettingsCard>
  );
}

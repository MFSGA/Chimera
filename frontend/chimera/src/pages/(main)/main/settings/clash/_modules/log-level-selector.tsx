import { useClashConfig } from '@chimera/interface';
import ArrowForwardIosRounded from '~icons/material-symbols/arrow-forward-ios-rounded';
import { useCallback, useMemo } from 'react';
import { Button } from '@/components/ui/button';
import {
  DropdownMenu,
  DropdownMenuCheckboxItem,
  DropdownMenuContent,
  DropdownMenuTrigger,
} from '@/components/ui/dropdown-menu';
import * as m from '@/paraglide/messages';
import {
  ItemContainer,
  ItemLabel,
  ItemLabelDescription,
  ItemLabelText,
  SettingsCard,
  SettingsCardContent,
} from '../../_modules/settings-card';

const LOG_LEVEL_OPTIONS = {
  debug: 'Debug',
  info: 'Info',
  warning: 'Warn',
  error: 'Error',
  silent: 'Silent',
} as const;

export default function LogLevelSelector() {
  const { query, upsert } = useClashConfig();

  const value = useMemo(
    () => query.data?.['log-level'] as keyof typeof LOG_LEVEL_OPTIONS,
    [query.data],
  );

  const handleLogLevelChange = useCallback(
    async (value: string) => {
      await upsert.mutateAsync({
        'log-level': value,
      });
    },
    [upsert],
  );

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
                    {value ? LOG_LEVEL_OPTIONS[value] : null}
                  </ItemLabelDescription>
                </ItemLabel>

                <ArrowForwardIosRounded />
              </ItemContainer>
            </Button>
          </SettingsCardContent>
        </DropdownMenuTrigger>

        <DropdownMenuContent align="end" sideOffset={-16} alignOffset={16}>
          {Object.entries(LOG_LEVEL_OPTIONS).map(([key, label]) => (
            <DropdownMenuCheckboxItem
              checked={value === key}
              key={key}
              onSelect={() => void handleLogLevelChange(key)}
            >
              {label}
            </DropdownMenuCheckboxItem>
          ))}
        </DropdownMenuContent>
      </DropdownMenu>
    </SettingsCard>
  );
}

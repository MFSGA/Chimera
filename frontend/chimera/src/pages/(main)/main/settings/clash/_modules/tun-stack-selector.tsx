import {
  useRuntimeProfile,
  useSetting,
  type TunStack,
} from '@chimera/interface';
import ArrowForwardIosRounded from '~icons/material-symbols/arrow-forward-ios-rounded';
import { useMemo } from 'react';
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

export default function TunStackSelector() {
  const coreType = useSetting('clash_core');

  const tunStack = useSetting('tun_stack');

  const enableTunMode = useSetting('enable_tun_mode');

  const runtimeProfile = useRuntimeProfile();

  const tunStackOptions = useMemo(() => {
    const options: Record<string, string> = {
      system: 'System',
      gvisor: 'gVisor',
      mixed: 'Mixed',
    };

    if (coreType.value === 'clash') {
      delete options.mixed;
    }

    return options;
  }, [coreType.value]);

  const currentTunStack = useMemo(() => {
    const stack = tunStack.value || 'gvisor';
    return stack in tunStackOptions ? stack : 'gvisor';
  }, [tunStack.value, tunStackOptions]);

  const handleTunStackChange = useLockFn(async (value: string) => {
    try {
      await tunStack.upsert(value as TunStack);

      if (enableTunMode.value) {
        await enableTunMode.upsert(true);
      }

      await runtimeProfile.refetch();
    } catch (error) {
      message(`Change Tun Stack failed ! \n Error: ${formatError(error)}`, {
        title: 'Error',
        kind: 'error',
      });
    }
  });

  const isPending = tunStack.isPending || enableTunMode.isPending;

  return (
    <SettingsCard data-slot="tun-stack-selector-card">
      <DropdownMenu align="end">
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

        <DropdownMenuContent sideOffset={-16} alignOffset={16}>
          {Object.entries(tunStackOptions).map(([key, label]) => (
            <DropdownMenuCheckboxItem
              checked={tunStack.value === key}
              disabled={isPending}
              key={key}
              onSelect={() => void handleTunStackChange(key)}
            >
              {label}
            </DropdownMenuCheckboxItem>
          ))}
        </DropdownMenuContent>
      </DropdownMenu>
    </SettingsCard>
  );
}

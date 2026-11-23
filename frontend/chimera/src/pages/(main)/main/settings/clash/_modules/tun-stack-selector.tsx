import {
  useClashSetting,
  useConfigFeatures,
  useRuntimeProfile,
  type TunStack,
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

const TUN_STACK_LABELS: Record<TunStack, string> = {
  system: 'System',
  gvisor: 'gVisor',
  mixed: 'Mixed',
  mips: 'Mips (Mihomo IP Stack)',
};

export default function TunStackSelector() {
  const features = useConfigFeatures();

  const tunStack = useClashSetting('tun_stack');

  const enableTunMode = useClashSetting('enable_tun_mode');

  const runtimeProfile = useRuntimeProfile();

  // Ref: only the core's capability matrix decides whether the stack exists.
  const stacks = features.data?.tun.stack;

  const currentTunStack =
    stacks?.kind === 'values' &&
    stacks.value.includes(tunStack.value ?? 'gvisor')
      ? (tunStack.value ?? 'gvisor')
      : null;

  const handleTunStackChange = useLockFn(async (value: TunStack) => {
    try {
      await tunStack.upsert(value);

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

  // A core that ignores tun.stack must not show an unsupported control.
  if (stacks?.kind !== 'values') {
    return null;
  }

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
                    {currentTunStack ? TUN_STACK_LABELS[currentTunStack] : null}
                  </ItemLabelDescription>
                </ItemLabel>

                <ArrowForwardIosRounded />
              </ItemContainer>
            </Button>
          </SettingsCardContent>
        </DropdownMenuTrigger>

        <DropdownMenuContent sideOffset={-16} alignOffset={16}>
          {stacks.value.map((stack) => (
            <DropdownMenuCheckboxItem
              checked={tunStack.value === stack}
              disabled={isPending}
              key={stack}
              onSelect={() => void handleTunStackChange(stack)}
            >
              {TUN_STACK_LABELS[stack]}
            </DropdownMenuCheckboxItem>
          ))}
        </DropdownMenuContent>
      </DropdownMenu>
    </SettingsCard>
  );
}
